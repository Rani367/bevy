//! The hierarchy panel: a live tree of the scene's entities (everything tagged
//! [`SceneEntity`]). It rebuilds when the scene structure changes, highlights the
//! current selection, and handles the spawn / delete action events from the menus.

use bevy_app::{App, Plugin, Update};
use bevy_asset::Assets;
use bevy_ecs::entity::Entity;
use bevy_ecs::hierarchy::{ChildOf, Children};
use bevy_ecs::lifecycle::RemovedComponents;
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use bevy_feathers::cursor::EntityCursor;
use bevy_feathers::display::label;
use bevy_feathers::theme::ThemeBackgroundColor;
use bevy_feathers::tokens;
use bevy_input::keyboard::KeyCode;
use bevy_input::ButtonInput;
use bevy_mesh::Mesh;
use bevy_pbr::StandardMaterial;
use bevy_picking::events::{Click, Pointer};
use bevy_picking::Pickable;
use bevy_scene::prelude::*;
use bevy_transform::components::Transform;
use bevy_ui::{px, AlignItems, Node, UiRect};
use bevy_window::SystemCursorIcon;

use crate::actions::{DeleteSelectedRequest, SpawnRequest};
use crate::markers::SceneEntity;
use crate::spawning::{default_name, spawn_kind};
use crate::state::EditorSelection;
use crate::ui::HierarchyContent;

/// Component placed on a hierarchy row UI node, pointing back at the scene entity it
/// represents.
#[derive(Component, Debug, Clone, Copy)]
pub struct HierarchyRow(pub Entity);

impl Default for HierarchyRow {
    fn default() -> Self {
        Self(Entity::PLACEHOLDER)
    }
}

/// Set when the hierarchy needs to be rebuilt (entity added/removed/renamed).
#[derive(Resource, Default)]
struct HierarchyDirty(bool);

/// Installs the hierarchy panel systems and spawn/delete observers.
pub struct HierarchyPlugin;

impl Plugin for HierarchyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HierarchyDirty>()
            .add_systems(
                Update,
                (mark_hierarchy_dirty, rebuild_hierarchy, highlight_rows),
            )
            .add_observer(on_spawn_request)
            .add_observer(on_delete_selected);
    }
}

// ---------------------------------------------------------------------------
// Tree building
// ---------------------------------------------------------------------------

/// Flag a rebuild when scene entities are added, renamed, or removed.
fn mark_hierarchy_dirty(
    changed: Query<(), (With<SceneEntity>, Or<(Added<SceneEntity>, Changed<Name>)>)>,
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
        push_subtree(root, 0, &children_q, &scene_q, &mut flat);
    }

    let rows: Vec<_> = flat
        .iter()
        .map(|&(entity, depth)| {
            let name = name_q
                .get(entity)
                .map(|n| n.as_str().to_string())
                .unwrap_or_else(|_| format!("{entity:?}"));
            hierarchy_row(entity, name, depth, selection.contains(entity))
        })
        .collect();

    commands.entity(content).despawn_children();
    commands
        .entity(content)
        .queue_spawn_related_scenes::<Children>(rows);
}

/// Depth-first push of an entity and its scene-entity descendants.
fn push_subtree(
    entity: Entity,
    depth: usize,
    children_q: &Query<&Children>,
    scene_q: &Query<(), With<SceneEntity>>,
    out: &mut Vec<(Entity, usize)>,
) {
    out.push((entity, depth));
    if let Ok(children) = children_q.get(entity) {
        for child in children.iter() {
            if scene_q.contains(child) {
                push_subtree(child, depth + 1, children_q, scene_q, out);
            }
        }
    }
}

/// One row of the hierarchy tree: an indented, clickable label for a scene entity.
fn hierarchy_row(entity: Entity, name: String, depth: usize, selected: bool) -> impl Scene {
    let bg = if selected {
        tokens::MENUITEM_BG_FOCUSED
    } else {
        tokens::PANE_BODY_BG
    };
    let indent = px(4.0 + depth as f32 * 14.0);
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
        }
        ThemeBackgroundColor(bg)
        HierarchyRow(entity)
        EntityCursor::System(SystemCursorIcon::Pointer)
        on(on_row_click)
        Children [
            (label(name) Pickable::IGNORE)
        ]
    }
}

/// Click a row to select its entity (Ctrl/Cmd to extend the selection).
fn on_row_click(
    click: On<Pointer<Click>>,
    rows: Query<&HierarchyRow>,
    keys: Res<ButtonInput<KeyCode>>,
    mut selection: ResMut<EditorSelection>,
) {
    let Ok(row) = rows.get(click.entity) else {
        return;
    };
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

/// Recolor rows to reflect the current selection without a full rebuild.
/// [`ThemeBackgroundColor`] is immutable, so we re-insert it via commands (matching how
/// Feathers' own controls update their themed colors).
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
            commands
                .entity(ui_entity)
                .insert(ThemeBackgroundColor(token));
        }
    }
}

// ---------------------------------------------------------------------------
// Spawn / delete
// ---------------------------------------------------------------------------

/// Handle an entity-creation request from the menu: spawn the entity via the shared
/// helper, select it, and flag a rebuild.
fn on_spawn_request(
    spawn: On<SpawnRequest>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut selection: ResMut<EditorSelection>,
    mut dirty: ResMut<HierarchyDirty>,
) {
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

/// Handle a delete request: despawn the selected entities and clear the selection.
fn on_delete_selected(
    _delete: On<DeleteSelectedRequest>,
    mut commands: Commands,
    mut selection: ResMut<EditorSelection>,
    mut dirty: ResMut<HierarchyDirty>,
) {
    for &entity in selection.all.iter() {
        commands.entity(entity).despawn();
    }
    selection.clear();
    dirty.0 = true;
}
