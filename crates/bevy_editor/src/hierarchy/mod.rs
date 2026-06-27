//! The hierarchy panel: a live tree of the scene's entities (everything tagged
//! [`SceneEntity`]). It rebuilds when the scene structure changes, highlights the current
//! selection, and supports the full editing loop: select, collapse/expand, a right-click
//! context menu, inline rename, duplicate, delete, and drag-and-drop reparenting.

use core::any::TypeId;
use std::collections::HashSet;

use bevy_app::{App, Plugin, Update};
use bevy_asset::Assets;
use bevy_ecs::entity::Entity;
use bevy_ecs::hierarchy::{ChildOf, Children};
use bevy_ecs::lifecycle::RemovedComponents;
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use bevy_ecs::reflect::{AppTypeRegistry, ReflectComponent};
use bevy_feathers::controls::{
    ButtonVariant, FeathersButton, FeathersTextInput, FeathersTextInputContainer,
};
use bevy_feathers::cursor::EntityCursor;
use bevy_feathers::display::label;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy_feathers::tokens;
use bevy_input::keyboard::KeyCode;
use bevy_input::ButtonInput;
use bevy_input_focus::AutoFocus;
use bevy_math::{Vec2, Vec3};
use bevy_mesh::Mesh;
use bevy_pbr::StandardMaterial;
use bevy_picking::events::{Click, DragDrop, Pointer};
use bevy_picking::pointer::PointerButton;
use bevy_picking::Pickable;
use bevy_reflect::PartialReflect;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_text::EditableText;
use bevy_transform::components::Transform;
use bevy_ui::widget::Text;
use bevy_ui::{
    percent, px, AlignItems, Display, FlexDirection, GlobalZIndex, Node, PositionType, UiRect,
};
use bevy_ui_widgets::Activate;
use bevy_window::SystemCursorIcon;

use crate::actions::{
    DeleteSelectedRequest, DuplicateRequest, RenameRequest, ReparentRequest, SpawnRequest,
};
use crate::markers::{EditorEntity, SceneEntity};
use crate::spawning::{default_name, spawn_kind};
use crate::state::EditorSelection;
use crate::ui::{read_text_input, HierarchyContent, SeedText};
use crate::undo::push_undo;

/// Component placed on a hierarchy row UI node, pointing back at the scene entity it
/// represents.
#[derive(Component, Debug, Clone, Copy)]
pub struct HierarchyRow(pub Entity);

impl Default for HierarchyRow {
    fn default() -> Self {
        Self(Entity::PLACEHOLDER)
    }
}

/// On a row's disclosure (collapse/expand) toggle; points at the scene entity.
#[derive(Component, Debug, Clone, Copy)]
struct RowDisclosure(Entity);

impl Default for RowDisclosure {
    fn default() -> Self {
        Self(Entity::PLACEHOLDER)
    }
}

/// Marks the inline rename text input.
#[derive(Component, Default, Clone, Copy)]
struct RenameInput;

/// Marks the full-screen backdrop behind a context menu (click it to dismiss).
#[derive(Component, Default, Clone, Copy)]
struct ContextMenuBackdrop;

/// Request to close any open context menu.
#[derive(Event, Clone, Copy)]
struct CloseContextMenu;

/// Set when the hierarchy needs to be rebuilt (entity added/removed/renamed/reparented).
#[derive(Resource, Default)]
struct HierarchyDirty(bool);

/// Scene entities whose children are currently collapsed in the tree.
#[derive(Resource, Default)]
struct CollapsedNodes(HashSet<Entity>);

/// The scene entity currently being renamed inline, if any.
#[derive(Resource, Default)]
struct Renaming(Option<Entity>);

/// Installs the hierarchy panel systems and action observers.
pub struct HierarchyPlugin;

impl Plugin for HierarchyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HierarchyDirty>()
            .init_resource::<CollapsedNodes>()
            .init_resource::<Renaming>()
            .add_systems(
                Update,
                (
                    mark_hierarchy_dirty,
                    rebuild_hierarchy,
                    highlight_rows,
                    commit_rename,
                    close_menu_on_escape,
                ),
            )
            .add_observer(on_spawn_request)
            .add_observer(on_delete_selected)
            .add_observer(on_duplicate_request)
            .add_observer(on_reparent_request)
            .add_observer(on_rename_request)
            .add_observer(on_row_drag_drop)
            .add_observer(on_content_drag_drop)
            .add_observer(on_close_context_menu);
    }
}

// ---------------------------------------------------------------------------
// Tree building
// ---------------------------------------------------------------------------

/// Flag a rebuild when scene entities are added, renamed, reparented, or removed.
fn mark_hierarchy_dirty(
    changed: Query<
        (),
        (
            With<SceneEntity>,
            Or<(Added<SceneEntity>, Changed<Name>, Changed<ChildOf>)>,
        ),
    >,
    mut removed: RemovedComponents<SceneEntity>,
    mut dirty: ResMut<HierarchyDirty>,
) {
    if !changed.is_empty() || removed.read().next().is_some() {
        dirty.0 = true;
    }
}

/// Rebuild the hierarchy rows from the live scene graph when dirty.
fn rebuild_hierarchy(
    mut dirty: ResMut<HierarchyDirty>,
    mut commands: Commands,
    content_q: Query<Entity, With<HierarchyContent>>,
    roots_q: Query<Entity, (With<SceneEntity>, Without<ChildOf>)>,
    children_q: Query<&Children>,
    scene_q: Query<(), With<SceneEntity>>,
    name_q: Query<&Name>,
    selection: Res<EditorSelection>,
    collapsed: Res<CollapsedNodes>,
    renaming: Res<Renaming>,
) {
    if !dirty.0 {
        return;
    }
    dirty.0 = false;

    let Ok(content) = content_q.single() else {
        return;
    };

    let mut flat: Vec<(Entity, usize)> = Vec::new();
    for root in roots_q.iter() {
        push_subtree(root, 0, &children_q, &scene_q, &collapsed, &mut flat);
    }

    let rows: Vec<Box<dyn SceneList>> = flat
        .iter()
        .map(|&(entity, depth)| {
            let name = name_q
                .get(entity)
                .map(|n| n.as_str().to_string())
                .unwrap_or_else(|_| format!("{entity:?}"));
            let has_children = children_q
                .get(entity)
                .map(|c| c.iter().any(|ch| scene_q.contains(ch)))
                .unwrap_or(false);
            if renaming.0 == Some(entity) {
                Box::new(EntityScene(rename_row(entity, name, depth))) as Box<dyn SceneList>
            } else {
                Box::new(EntityScene(normal_row(
                    entity,
                    name,
                    depth,
                    selection.contains(entity),
                    has_children,
                    collapsed.0.contains(&entity),
                )))
            }
        })
        .collect();

    commands.entity(content).despawn_children();
    commands
        .entity(content)
        .queue_spawn_related_scenes::<Children>(rows);
}

/// Depth-first push of an entity and its scene-entity descendants, skipping the children
/// of collapsed nodes.
fn push_subtree(
    entity: Entity,
    depth: usize,
    children_q: &Query<&Children>,
    scene_q: &Query<(), With<SceneEntity>>,
    collapsed: &CollapsedNodes,
    out: &mut Vec<(Entity, usize)>,
) {
    out.push((entity, depth));
    if collapsed.0.contains(&entity) {
        return;
    }
    if let Ok(children) = children_q.get(entity) {
        for child in children.iter() {
            if scene_q.contains(child) {
                push_subtree(child, depth + 1, children_q, scene_q, collapsed, out);
            }
        }
    }
}

/// One row of the hierarchy tree: a disclosure toggle (when it has children) plus an
/// indented, clickable label.
fn normal_row(
    entity: Entity,
    name: String,
    depth: usize,
    selected: bool,
    has_children: bool,
    collapsed: bool,
) -> impl Scene {
    let bg = if selected {
        tokens::MENUITEM_BG_FOCUSED
    } else {
        tokens::PANE_BODY_BG
    };
    let indent = px(4.0 + depth as f32 * 14.0);
    let glyph = if !has_children {
        " "
    } else if collapsed {
        "\u{25B8}" // ▸
    } else {
        "\u{25BE}" // ▾
    };
    bsn! {
        Node {
            min_height: px(20),
            padding: UiRect {
                left: indent,
                right: px(4),
                top: px(2),
                bottom: px(2),
            },
            align_items: AlignItems::Center,
            column_gap: px(2),
        }
        ThemeBackgroundColor(bg)
        HierarchyRow(entity)
        EntityCursor::System(SystemCursorIcon::Pointer)
        on(on_row_click)
        Children [
            (
                Node { min_width: px(14), align_items: AlignItems::Center }
                RowDisclosure(entity)
                on(on_disclosure_click)
                Children [ (label(glyph) Pickable::IGNORE) ]
            ),
            (label(name) Pickable::IGNORE),
        ]
    }
}

/// A row in inline-rename mode: an autofocused text input seeded with the current name.
fn rename_row(entity: Entity, name: String, depth: usize) -> impl Scene {
    let indent = px(4.0 + depth as f32 * 14.0);
    bsn! {
        Node {
            min_height: px(22),
            padding: UiRect {
                left: indent,
                right: px(4),
                top: px(1),
                bottom: px(1),
            },
            align_items: AlignItems::Center,
        }
        ThemeBackgroundColor(tokens::PANE_BODY_BG)
        HierarchyRow(entity)
        Children [
            (@FeathersTextInputContainer Children [
                (@FeathersTextInput SeedText(name) RenameInput AutoFocus)
            ]),
        ]
    }
}

// ---------------------------------------------------------------------------
// Row interaction: select, double-click rename, right-click menu, collapse
// ---------------------------------------------------------------------------

/// Left-click selects (Ctrl/Cmd to extend, double-click renames); right-click opens the
/// context menu.
fn on_row_click(
    click: On<Pointer<Click>>,
    rows: Query<&HierarchyRow>,
    keys: Res<ButtonInput<KeyCode>>,
    existing_menus: Query<Entity, With<ContextMenuBackdrop>>,
    mut selection: ResMut<EditorSelection>,
    mut commands: Commands,
) {
    let Ok(row) = rows.get(click.entity) else {
        return;
    };
    match click.button {
        PointerButton::Secondary => {
            for menu in existing_menus.iter() {
                commands.entity(menu).despawn();
            }
            selection.set_single(row.0);
            commands.spawn_scene(context_menu(click.pointer_location.position));
        }
        PointerButton::Primary => {
            if click.count >= 2 {
                commands.trigger(RenameRequest(row.0));
                return;
            }
            let additive = keys.pressed(KeyCode::ControlLeft)
                || keys.pressed(KeyCode::ControlRight)
                || keys.pressed(KeyCode::SuperLeft)
                || keys.pressed(KeyCode::SuperRight);
            if additive {
                selection.toggle(row.0);
            } else {
                selection.set_single(row.0);
            }
        }
        _ => {}
    }
}

/// Toggle a node's collapsed state when its disclosure glyph is clicked.
fn on_disclosure_click(
    mut click: On<Pointer<Click>>,
    disclosures: Query<&RowDisclosure>,
    children_q: Query<&Children>,
    scene_q: Query<(), With<SceneEntity>>,
    mut collapsed: ResMut<CollapsedNodes>,
    mut dirty: ResMut<HierarchyDirty>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(disclosure) = disclosures.get(click.entity) else {
        return;
    };
    let has_children = children_q
        .get(disclosure.0)
        .map(|c| c.iter().any(|ch| scene_q.contains(ch)))
        .unwrap_or(false);
    if !has_children {
        return; // a leaf's blank spacer falls through to row selection
    }
    // Don't also select the row when toggling.
    click.propagate(false);
    if collapsed.0.contains(&disclosure.0) {
        collapsed.0.remove(&disclosure.0);
    } else {
        collapsed.0.insert(disclosure.0);
    }
    dirty.0 = true;
}

/// Recolor rows to reflect the current selection without a full rebuild.
fn highlight_rows(
    selection: Res<EditorSelection>,
    rows: Query<(Entity, &HierarchyRow, &ThemeBackgroundColor)>,
    mut commands: Commands,
) {
    if !selection.is_changed() {
        return;
    }
    for (ui_entity, row, bg) in rows.iter() {
        let token = if selection.contains(row.0) {
            tokens::MENUITEM_BG_FOCUSED
        } else {
            tokens::PANE_BODY_BG
        };
        if bg.0 != token {
            // `try_insert`: the row may be despawned this same frame by a concurrent
            // `rebuild_hierarchy` (which already colors fresh rows correctly).
            commands
                .entity(ui_entity)
                .try_insert(ThemeBackgroundColor(token));
        }
    }
}

// ---------------------------------------------------------------------------
// Context menu
// ---------------------------------------------------------------------------

/// A right-click context menu: a full-screen click-catching backdrop hosting a small
/// button panel anchored at the pointer.
fn context_menu(pos: Vec2) -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
        }
        EditorEntity
        ContextMenuBackdrop
        GlobalZIndex(1000)
        on(|_: On<Pointer<Click>>, mut c: Commands| { c.trigger(CloseContextMenu); })
        Children [
            (
                Node {
                    position_type: PositionType::Absolute,
                    left: px(pos.x),
                    top: px(pos.y),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    min_width: px(150),
                    padding: px(4),
                    row_gap: px(2),
                }
                EditorEntity
                ThemeBackgroundColor(tokens::PANE_HEADER_BG)
                GlobalZIndex(1001)
                Children [
                    (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { Text("Rename") ThemedText } }
                        on(|_: On<Activate>, sel: Res<EditorSelection>, mut c: Commands| {
                            if let Some(e) = sel.primary { c.trigger(RenameRequest(e)); }
                            c.trigger(CloseContextMenu);
                        })),
                    (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { Text("Duplicate") ThemedText } }
                        on(|_: On<Activate>, mut c: Commands| {
                            c.trigger(DuplicateRequest);
                            c.trigger(CloseContextMenu);
                        })),
                    (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { Text("Delete") ThemedText } }
                        on(|_: On<Activate>, mut c: Commands| {
                            c.trigger(DeleteSelectedRequest);
                            c.trigger(CloseContextMenu);
                        })),
                ]
            ),
        ]
    }
}

/// Despawn any open context menu (its backdrop, which owns the panel as a child).
fn on_close_context_menu(
    _: On<CloseContextMenu>,
    menus: Query<Entity, With<ContextMenuBackdrop>>,
    mut commands: Commands,
) {
    for menu in menus.iter() {
        commands.entity(menu).despawn();
    }
}

/// Close an open context menu when Escape is pressed.
fn close_menu_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    menus: Query<Entity, With<ContextMenuBackdrop>>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Escape) {
        for menu in menus.iter() {
            commands.entity(menu).despawn();
        }
    }
}

// ---------------------------------------------------------------------------
// Rename
// ---------------------------------------------------------------------------

/// Enter inline-rename mode for an entity.
fn on_rename_request(
    req: On<RenameRequest>,
    mut renaming: ResMut<Renaming>,
    mut dirty: ResMut<HierarchyDirty>,
) {
    renaming.0 = Some(req.0);
    dirty.0 = true;
}

/// Commit the inline rename on Enter (writing the new `Name`), or cancel on Escape.
fn commit_rename(
    keys: Res<ButtonInput<KeyCode>>,
    mut renaming: ResMut<Renaming>,
    inputs: Query<Entity, With<RenameInput>>,
    editables: Query<&EditableText>,
    mut dirty: ResMut<HierarchyDirty>,
    mut commands: Commands,
) {
    let Some(entity) = renaming.0 else {
        return;
    };
    if keys.just_pressed(KeyCode::Escape) {
        renaming.0 = None;
        dirty.0 = true;
        return;
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        if let Ok(input) = inputs.single()
            && let Some(text) = read_text_input(&editables, input)
        {
            let text = text.trim().to_string();
            if !text.is_empty() {
                push_undo(&mut commands);
                commands.entity(entity).insert(Name::new(text));
            }
        }
        renaming.0 = None;
        dirty.0 = true;
    }
}

// ---------------------------------------------------------------------------
// Spawn / delete / duplicate / reparent
// ---------------------------------------------------------------------------

/// Handle an entity-creation request: spawn via the shared helper, select it, rebuild.
fn on_spawn_request(
    spawn: On<SpawnRequest>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut selection: ResMut<EditorSelection>,
    mut dirty: ResMut<HierarchyDirty>,
) {
    push_undo(&mut commands);
    let kind = spawn.0;
    let entity = spawn_kind(
        &mut commands,
        &mut meshes,
        &mut materials,
        kind,
        Transform::default(),
        default_name(kind),
    );
    selection.set_single(entity);
    dirty.0 = true;
}

/// Despawn the selected entities and clear the selection.
fn on_delete_selected(
    _delete: On<DeleteSelectedRequest>,
    mut commands: Commands,
    mut selection: ResMut<EditorSelection>,
    mut dirty: ResMut<HierarchyDirty>,
) {
    push_undo(&mut commands);
    for &entity in selection.all.iter() {
        commands.entity(entity).despawn();
    }
    selection.clear();
    dirty.0 = true;
}

/// Duplicate every selected entity (shallow: components only, not descendants) and select
/// the new copies.
fn on_duplicate_request(_: On<DuplicateRequest>, mut commands: Commands) {
    push_undo(&mut commands);
    commands.queue(|world: &mut World| {
        let selected: Vec<Entity> = world.resource::<EditorSelection>().all.clone();
        let mut last = None;
        for src in selected {
            if let Some(dst) = duplicate_entity(world, src) {
                last = Some(dst);
            }
        }
        if let Some(dst) = last {
            world.resource_mut::<EditorSelection>().set_single(dst);
        }
    });
}

/// Clone an entity's registered components (excluding hierarchy relationships) into a new
/// entity, re-parent it under the same parent, and nudge its transform.
fn duplicate_entity(world: &mut World, src: Entity) -> Option<Entity> {
    let registry_arc = world.resource::<AppTypeRegistry>().0.clone();
    let parent = world.get::<ChildOf>(src).map(ChildOf::parent);
    let type_ids: Vec<TypeId> = world
        .inspect_entity(src)
        .ok()?
        .filter_map(bevy_ecs::component::ComponentInfo::type_id)
        .filter(|tid| *tid != TypeId::of::<Children>() && *tid != TypeId::of::<ChildOf>())
        .collect();

    let dst = world.spawn_empty().id();
    {
        let registry = registry_arc.read();
        for tid in type_ids {
            let Some(registration) = registry.get(tid) else {
                continue;
            };
            let Some(reflect_component) = registration.data::<ReflectComponent>() else {
                continue;
            };
            let cloned = {
                let Ok(src_ref) = world.get_entity(src) else {
                    break;
                };
                reflect_component
                    .reflect(src_ref)
                    .map(PartialReflect::to_dynamic)
            };
            if let Some(value) = cloned {
                let mut dst_mut = world.entity_mut(dst);
                reflect_component.insert(&mut dst_mut, &*value, &registry);
            }
        }
    }

    if let Some(parent) = parent {
        world.entity_mut(dst).insert(ChildOf(parent));
    }
    if let Some(mut transform) = world.get_mut::<Transform>(dst) {
        transform.translation += Vec3::new(0.5, 0.0, 0.5);
    }
    Some(dst)
}

/// Reparent a scene entity, guarding against cycles.
fn on_reparent_request(
    req: On<ReparentRequest>,
    children_q: Query<&Children>,
    mut dirty: ResMut<HierarchyDirty>,
    mut commands: Commands,
) {
    let child = req.child;
    if let Some(parent) = req.new_parent
        && (parent == child || is_descendant(parent, child, &children_q))
    {
        return;
    }
    push_undo(&mut commands);
    match req.new_parent {
        Some(parent) => {
            commands.entity(child).insert(ChildOf(parent));
        }
        None => {
            commands.entity(child).remove::<ChildOf>();
        }
    }
    dirty.0 = true;
}

/// Whether `candidate` is `root` itself or one of its descendants.
fn is_descendant(candidate: Entity, root: Entity, children_q: &Query<&Children>) -> bool {
    if let Ok(children) = children_q.get(root) {
        for child in children.iter() {
            if child == candidate || is_descendant(candidate, child, children_q) {
                return true;
            }
        }
    }
    false
}

/// Drop one row onto another → reparent the dragged entity under the target.
fn on_row_drag_drop(
    drop: On<Pointer<DragDrop>>,
    rows: Query<&HierarchyRow>,
    mut commands: Commands,
) {
    let (Ok(target), Ok(dragged)) = (rows.get(drop.entity), rows.get(drop.dropped)) else {
        return;
    };
    if target.0 == dragged.0 {
        return;
    }
    commands.trigger(ReparentRequest {
        child: dragged.0,
        new_parent: Some(target.0),
    });
}

/// Drop a row onto empty hierarchy space → unparent it to the scene root.
fn on_content_drag_drop(
    drop: On<Pointer<DragDrop>>,
    content_q: Query<(), With<HierarchyContent>>,
    rows: Query<&HierarchyRow>,
    mut commands: Commands,
) {
    if !content_q.contains(drop.entity) {
        return;
    }
    if let Ok(dragged) = rows.get(drop.dropped) {
        commands.trigger(ReparentRequest {
            child: dragged.0,
            new_parent: None,
        });
    }
}
