//! Scene file save/load plus the asset browser. Scenes are full `DynamicWorld`
//! serializations (`assets/scenes/*.scn.ron`): every [`SceneEntity`]'s reflected components
//! **and parent links** are written, except the runtime-built mesh/material/sprite and the
//! computed transform/visibility components, which can't round-trip through asset handles and
//! are rebuilt from each entity's [`SpawnedAs`] on load. So arbitrary reflected components and
//! the scene hierarchy persist to disk; only the procedural geometry is regenerated. (The
//! in-memory play-mode/undo snapshot keeps the live `DynamicWorld` with handles intact.)

use bevy_app::{App, Plugin, Update};
use bevy_asset::{AssetServer, Assets, Handle};
use bevy_camera::visibility::{InheritedVisibility, ViewVisibility, VisibilityClass};
use bevy_ecs::entity::EntityHashMap;
use bevy_ecs::prelude::*;
use bevy_ecs::reflect::AppTypeRegistry;
use bevy_ecs::world::CommandQueue;
use bevy_feathers::controls::{
    ButtonVariant, FeathersButton, FeathersTextInput, FeathersTextInputContainer,
};
use bevy_feathers::cursor::EntityCursor;
use bevy_feathers::display::{label, label_dim};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy_feathers::tokens;
use bevy_image::Image;
use bevy_input_focus::AutoFocus;
use bevy_log::{error, info};
use bevy_math::Vec3;
use bevy_mesh::{Mesh, Mesh3d};
use bevy_pbr::{MeshMaterial3d, StandardMaterial};
use bevy_picking::events::{Click, Pointer};
use bevy_picking::Pickable;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_sprite::Sprite;
use bevy_text::EditableText;
use bevy_transform::components::{GlobalTransform, Transform};
use bevy_ui::widget::{ImageNode, Text};
use bevy_ui::{
    percent, px, AlignItems, Display, FlexDirection, GlobalZIndex, JustifyContent, Node, Overflow,
    PositionType, UiRect,
};
use bevy_ui_widgets::{Activate, ScrollArea};
use bevy_window::SystemCursorIcon;
use bevy_world_serialization::serde::WorldDeserializer;
use bevy_world_serialization::DynamicWorldBuilder;
use serde::de::DeserializeSeed;

use crate::actions::{OpenImportDialog, OpenOpenDialog, OpenSaveDialog, SceneIoRequest, SpawnKind};
use crate::markers::{EditorEntity, SceneEntity};
use crate::spawning::{apply_kind_visuals, spawn_kind, SpawnedAs};
use crate::state::EditorSelection;
use crate::ui::{stop_click, AssetContent, CloseOverlay, EditorOverlay, SeedText};
use crate::undo::push_undo;

/// Directory (relative to the working dir) where scene files live.
const SCENES_DIR: &str = "assets/scenes";
/// Scene-file extension. Scenes are full `DynamicWorld` serializations.
const SCENE_EXT: &str = ".scn.ron";
/// Fallback file name used by *Save* when no scene has been named yet.
const DEFAULT_SCENE: &str = "scene.scn.ron";

/// The currently-open scene file, if any.
#[derive(Resource, Default)]
pub struct CurrentScene {
    /// File name (within [`SCENES_DIR`]) of the open scene.
    pub path: Option<String>,
}

/// Set when the asset browser list should be rebuilt (startup, or after a save).
#[derive(Resource)]
struct AssetBrowserDirty(bool);

/// Marks an asset-browser entry node; stores the scene file it opens.
#[derive(Component, Default, Clone)]
struct AssetEntry {
    name: String,
}

/// Installs scene save/load and the asset browser.
pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurrentScene>()
            .insert_resource(AssetBrowserDirty(true))
            .add_systems(Update, rebuild_asset_browser)
            .add_observer(on_scene_io)
            .add_observer(on_asset_click)
            .add_observer(on_open_save_dialog)
            .add_observer(on_open_open_dialog)
            .add_observer(on_save_confirm)
            .add_observer(on_open_scene_button)
            .add_observer(on_open_import_dialog)
            .add_observer(on_import_confirm);
    }
}

// ---------------------------------------------------------------------------
// Save / load / new
// ---------------------------------------------------------------------------

fn on_scene_io(
    request: On<SceneIoRequest>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene_entities: Query<Entity, With<SceneEntity>>,
    mut current: ResMut<CurrentScene>,
    mut selection: ResMut<EditorSelection>,
    mut browser_dirty: ResMut<AssetBrowserDirty>,
) {
    match &*request {
        SceneIoRequest::New => {
            push_undo(&mut commands);
            clear_scene(&scene_entities, &mut commands);
            selection.clear();
            current.path = None;
            spawn_kind(
                &mut commands,
                &mut meshes,
                &mut materials,
                SpawnKind::DirectionalLight,
                Transform::from_xyz(6.0, 10.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
                "Directional Light",
            );
            info!("New scene");
        }
        SceneIoRequest::Save => {
            let name = ensure_ext(
                current
                    .path
                    .clone()
                    .unwrap_or_else(|| DEFAULT_SCENE.to_string()),
            );
            current.path = Some(name.clone());
            browser_dirty.0 = true;
            commands.queue(move |world: &mut World| {
                if let Err(err) = write_scene(world, &name) {
                    error!("Save failed: {err}");
                }
            });
        }
        SceneIoRequest::SaveAs(name) => {
            let name = ensure_ext(name.clone());
            current.path = Some(name.clone());
            browser_dirty.0 = true;
            commands.queue(move |world: &mut World| {
                if let Err(err) = write_scene(world, &name) {
                    error!("Save failed: {err}");
                }
            });
        }
        SceneIoRequest::Open(name) => {
            push_undo(&mut commands);
            current.path = Some(name.clone());
            let name = name.clone();
            commands.queue(
                move |world: &mut World| match open_scene(world, &name, true) {
                    Ok(n) => info!("Opened scene '{name}' ({n} entities)"),
                    Err(err) => error!("Failed to open scene '{name}': {err}"),
                },
            );
        }
        SceneIoRequest::Instantiate(name) => {
            push_undo(&mut commands);
            let name = name.clone();
            commands.queue(
                move |world: &mut World| match open_scene(world, &name, false) {
                    Ok(n) => info!("Instantiated '{name}' (+{n} entities)"),
                    Err(err) => error!("Failed to instantiate '{name}': {err}"),
                },
            );
        }
    }
}

/// Append the scene extension if `name` doesn't already end in it (tolerating a bare `.ron`).
fn ensure_ext(mut name: String) -> String {
    if !name.ends_with(SCENE_EXT) {
        if let Some(stripped) = name.strip_suffix(".ron") {
            name = stripped.to_string();
        }
        name.push_str(SCENE_EXT);
    }
    name
}

fn clear_scene(scene_entities: &Query<Entity, With<SceneEntity>>, commands: &mut Commands) {
    for entity in scene_entities.iter() {
        commands.entity(entity).despawn();
    }
}

/// Serialize every `SceneEntity` — its reflected components *and parent links* — to a
/// `.scn.ron` file via [`DynamicWorld`]. The runtime-built mesh/material/sprite and the
/// computed transform/visibility components are denied (they can't round-trip through asset
/// handles and are rebuilt from each entity's [`SpawnedAs`] on load).
fn write_scene(world: &mut World, name: &str) -> Result<(), String> {
    let ron = scene_to_ron(world)?;
    std::fs::create_dir_all(SCENES_DIR).map_err(|e| e.to_string())?;
    let path = format!("{SCENES_DIR}/{name}");
    std::fs::write(&path, ron).map_err(|e| e.to_string())?;
    info!("Saved scene to {path}");
    Ok(())
}

/// Serialize the scene (all `SceneEntity` components + parent links, minus runtime visuals)
/// to a RON string. Split out from [`write_scene`] so the round-trip is unit-testable.
fn scene_to_ron(world: &mut World) -> Result<String, String> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let ids: Vec<Entity> = {
        let mut q = world.query_filtered::<Entity, With<SceneEntity>>();
        q.iter(world).collect()
    };
    let registry = registry.read();
    let dynamic = DynamicWorldBuilder::from_world(world, &registry)
        .allow_all_components()
        .deny_component::<Mesh3d>()
        .deny_component::<MeshMaterial3d<StandardMaterial>>()
        .deny_component::<Sprite>()
        .deny_component::<GlobalTransform>()
        .deny_component::<InheritedVisibility>()
        .deny_component::<ViewVisibility>()
        // Computed render bookkeeping that holds non-serializable `TypeId`s.
        .deny_component::<VisibilityClass>()
        .extract_entities(ids.into_iter())
        .build();
    dynamic.serialize(&registry).map_err(|e| e.to_string())
}

/// Load a `.scn.ron` file, restoring components + parent links and rebuilding mesh/sprite
/// visuals from each entity's `SpawnedAs`. `clear` despawns the current scene first (Open);
/// otherwise the loaded entities are added to it (Instantiate / prefab drop). Returns the
/// number of entities written.
fn open_scene(world: &mut World, name: &str, clear: bool) -> Result<usize, String> {
    let path = format!("{SCENES_DIR}/{name}");
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let registry = world.resource::<AppTypeRegistry>().clone();

    let dynamic = {
        let registry = registry.read();
        // The deny-filter means no asset handles are stored; the `AssetServer` is the correct
        // general handle loader regardless.
        let mut asset_server = world.resource::<AssetServer>().clone();
        let seed = WorldDeserializer {
            type_registry: &registry,
            load_from_path: &mut asset_server,
        };
        let mut de = ron::Deserializer::from_str(&content).map_err(|e| e.to_string())?;
        seed.deserialize(&mut de).map_err(|e| e.to_string())?
    };

    if clear {
        let ids: Vec<Entity> = {
            let mut q = world.query_filtered::<Entity, With<SceneEntity>>();
            q.iter(world).collect()
        };
        for entity in ids {
            if let Ok(entity_mut) = world.get_entity_mut(entity) {
                entity_mut.despawn();
            }
        }
    }

    let mut map = EntityHashMap::default();
    dynamic
        .write_to_world(world, &mut map)
        .map_err(|e| format!("{e:?}"))?;

    let specs: Vec<(Entity, SpawnKind)> = map
        .values()
        .copied()
        .filter_map(|e| world.get::<SpawnedAs>(e).map(|s| (e, s.0)))
        .collect();
    rebuild_visuals(world, &specs);

    world.resource_mut::<EditorSelection>().clear();
    Ok(specs.len())
}

/// Re-apply runtime mesh/material/sprite visuals for freshly-loaded entities.
fn rebuild_visuals(world: &mut World, specs: &[(Entity, SpawnKind)]) {
    world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
        world.resource_scope(|world, mut materials: Mut<Assets<StandardMaterial>>| {
            let mut queue = CommandQueue::default();
            {
                let mut commands = Commands::new(&mut queue, world);
                for &(entity, kind) in specs {
                    apply_kind_visuals(&mut commands, &mut meshes, &mut materials, kind, entity);
                }
            }
            queue.apply(world);
        });
    });
}

// ---------------------------------------------------------------------------
// Asset browser
// ---------------------------------------------------------------------------

fn rebuild_asset_browser(
    mut dirty: ResMut<AssetBrowserDirty>,
    content_q: Query<Entity, With<AssetContent>>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    if !dirty.0 {
        return;
    }
    let Ok(content) = content_q.single() else {
        return; // panel not spawned yet; try again next frame
    };
    dirty.0 = false;

    let scenes = list_scene_files();
    let images = list_image_files();

    commands.entity(content).despawn_children();
    if scenes.is_empty() && images.is_empty() {
        commands
            .entity(content)
            .queue_spawn_related_scenes::<Children>(vec![empty_hint()]);
        return;
    }

    let mut rows: Vec<Box<dyn SceneList>> = Vec::new();
    for name in scenes {
        rows.push(Box::new(EntityScene(asset_entry(name))));
    }
    for name in images {
        let handle = asset_server.load(name.clone());
        rows.push(Box::new(EntityScene(image_thumb(name, handle))));
    }
    commands
        .entity(content)
        .queue_spawn_related_scenes::<Children>(rows);
}

fn empty_hint() -> impl Scene {
    bsn! {
        Node { padding: UiRect::axes(px(6), px(4)) }
        Children [ label_dim("No assets") ]
    }
}

/// A saved scene / prefab entry. Clicking it instantiates the scene into the current one.
fn asset_entry(name: String) -> impl Scene {
    let display = name.clone();
    bsn! {
        Node {
            min_height: px(22),
            padding: UiRect::axes(px(8), px(3)),
            align_items: AlignItems::Center,
        }
        ThemeBackgroundColor(tokens::BUTTON_BG)
        AssetEntry { name: name }
        EntityCursor::System(SystemCursorIcon::Pointer)
        Children [ (label(display) Pickable::IGNORE) ]
    }
}

/// An image asset entry, shown as a small live thumbnail with its filename.
fn image_thumb(name: String, handle: Handle<Image>) -> impl Scene {
    bsn! {
        Node {
            width: px(76),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(2),
        }
        Children [
            (ImageNode { image: handle } Node { width: px(60), height: px(60) } Pickable::IGNORE),
            (label_dim(name) Pickable::IGNORE),
        ]
    }
}

/// List `*.ron` scene files in the scenes directory, sorted.
fn list_scene_files() -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(SCENES_DIR) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && name.ends_with(SCENE_EXT)
            {
                files.push(name.to_string());
            }
        }
    }
    files.sort();
    files
}

/// List top-level image files in `assets/`, as paths relative to the asset root (so they
/// can be loaded via `AssetServer`), sorted.
fn list_image_files() -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir("assets") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                let lower = name.to_ascii_lowercase();
                if lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
                    files.push(name.to_string());
                }
            }
        }
    }
    files.sort();
    files
}

fn on_asset_click(click: On<Pointer<Click>>, entries: Query<&AssetEntry>, mut commands: Commands) {
    if let Ok(entry) = entries.get(click.entity) {
        commands.trigger(SceneIoRequest::Instantiate(entry.name.clone()));
    }
}

// ---------------------------------------------------------------------------
// Save / Open dialogs
// ---------------------------------------------------------------------------

/// The text input in the Save-As dialog.
#[derive(Component, Default, Clone, Copy)]
struct SaveNameInput;
/// The confirm button in the Save-As dialog.
#[derive(Component, Default, Clone, Copy)]
struct SaveConfirmButton;
/// The scrollable list container in the Open dialog.
#[derive(Component, Default, Clone, Copy)]
struct OpenDialogList;
/// A scene-file button in the Open dialog; opens the named scene.
#[derive(Component, Default, Clone)]
struct OpenSceneButton(String);

fn on_open_save_dialog(_: On<OpenSaveDialog>, current: Res<CurrentScene>, mut commands: Commands) {
    let initial = current
        .path
        .clone()
        .unwrap_or_else(|| DEFAULT_SCENE.to_string());
    commands.spawn_scene(save_dialog(initial));
}

fn on_save_confirm(
    act: On<Activate>,
    buttons: Query<(), With<SaveConfirmButton>>,
    inputs: Query<&EditableText, With<SaveNameInput>>,
    mut commands: Commands,
) {
    if !buttons.contains(act.entity) {
        return;
    }
    let Some(text) = inputs.iter().next().map(|e| e.value().to_string()) else {
        return;
    };
    let name = text.trim().to_string();
    if name.is_empty() {
        return;
    }
    commands.trigger(SceneIoRequest::SaveAs(ensure_ext(name)));
    commands.trigger(CloseOverlay);
}

fn on_open_open_dialog(_: On<OpenOpenDialog>, mut commands: Commands) {
    let files = list_scene_files();
    commands.queue(move |world: &mut World| {
        let _ = world.spawn_scene(open_dialog_overlay());
        let mut list_q = world.query_filtered::<Entity, With<OpenDialogList>>();
        let Some(list) = list_q.iter(world).next() else {
            return;
        };
        let items: Vec<Box<dyn SceneList>> = if files.is_empty() {
            vec![Box::new(EntityScene(empty_hint()))]
        } else {
            files
                .into_iter()
                .map(|f| Box::new(EntityScene(open_dialog_item(f))) as Box<dyn SceneList>)
                .collect()
        };
        world
            .entity_mut(list)
            .queue_spawn_related_scenes::<Children>(items);
    });
}

fn on_open_scene_button(
    act: On<Activate>,
    buttons: Query<&OpenSceneButton>,
    mut commands: Commands,
) {
    if let Ok(button) = buttons.get(act.entity) {
        commands.trigger(SceneIoRequest::Open(button.0.clone()));
        commands.trigger(CloseOverlay);
    }
}

fn save_dialog(initial: String) -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        EditorEntity
        EditorOverlay
        GlobalZIndex(2000)
        on(|_: On<Pointer<Click>>, mut c: Commands| { c.trigger(CloseOverlay); })
        Children [
            (
                Node {
                    width: px(320),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: px(10),
                    row_gap: px(8),
                }
                EditorEntity
                ThemeBackgroundColor(tokens::PANE_HEADER_BG)
                GlobalZIndex(2001)
                on(stop_click)
                Children [
                    (Node { padding: UiRect::axes(px(2), px(2)) } Children [ label("Save Scene As") ]),
                    (@FeathersTextInputContainer Children [
                        (@FeathersTextInput SeedText(initial) SaveNameInput AutoFocus)
                    ]),
                    (
                        Node { display: Display::Flex, flex_direction: FlexDirection::Row, column_gap: px(8) }
                        Children [
                            (@FeathersButton { @variant: ButtonVariant::Primary, @caption: bsn! { Text("Save") ThemedText } }
                                SaveConfirmButton),
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Cancel") ThemedText } }
                                on(|_: On<Activate>, mut c: Commands| { c.trigger(CloseOverlay); })),
                        ]
                    ),
                ]
            ),
        ]
    }
}

fn open_dialog_overlay() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        EditorEntity
        EditorOverlay
        GlobalZIndex(2000)
        on(|_: On<Pointer<Click>>, mut c: Commands| { c.trigger(CloseOverlay); })
        Children [
            (
                Node {
                    width: px(300),
                    max_height: percent(70),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: px(8),
                    row_gap: px(4),
                    overflow: Overflow::scroll_y(),
                }
                EditorEntity
                OpenDialogList
                ScrollArea
                ThemeBackgroundColor(tokens::PANE_HEADER_BG)
                GlobalZIndex(2001)
                on(stop_click)
                Children [
                    (Node { padding: UiRect::axes(px(2), px(2)) } Children [ label("Open Scene") ]),
                ]
            ),
        ]
    }
}

fn open_dialog_item(name: String) -> impl Scene {
    let display = name.clone();
    bsn! {
        (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { Text(display) ThemedText } }
            OpenSceneButton(name))
    }
}

// ---------------------------------------------------------------------------
// Import dialog (copy a file into assets/)
// ---------------------------------------------------------------------------

/// The path input in the Import dialog.
#[derive(Component, Default, Clone, Copy)]
struct ImportPathInput;
/// The confirm button in the Import dialog.
#[derive(Component, Default, Clone, Copy)]
struct ImportConfirmButton;

fn on_open_import_dialog(_: On<OpenImportDialog>, mut commands: Commands) {
    commands.spawn_scene(import_dialog());
}

fn on_import_confirm(
    act: On<Activate>,
    buttons: Query<(), With<ImportConfirmButton>>,
    inputs: Query<&EditableText, With<ImportPathInput>>,
    mut browser_dirty: ResMut<AssetBrowserDirty>,
    mut commands: Commands,
) {
    if !buttons.contains(act.entity) {
        return;
    }
    let Some(src) = inputs.iter().next().map(|e| e.value().to_string()) else {
        return;
    };
    let src = src.trim().to_string();
    if src.is_empty() {
        return;
    }
    match import_file(&src) {
        Ok(dest) => {
            info!("Imported asset to {dest}");
            browser_dirty.0 = true;
        }
        Err(err) => error!("Import failed: {err}"),
    }
    commands.trigger(CloseOverlay);
}

/// Copy a source file into `assets/`, returning the destination path.
fn import_file(src: &str) -> Result<String, String> {
    let file_name = std::path::Path::new(src)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "source has no file name".to_string())?;
    std::fs::create_dir_all("assets").map_err(|e| e.to_string())?;
    let dest = format!("assets/{file_name}");
    std::fs::copy(src, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

fn import_dialog() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        EditorEntity
        EditorOverlay
        GlobalZIndex(2000)
        on(|_: On<Pointer<Click>>, mut c: Commands| { c.trigger(CloseOverlay); })
        Children [
            (
                Node {
                    width: px(380),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: px(10),
                    row_gap: px(8),
                }
                EditorEntity
                ThemeBackgroundColor(tokens::PANE_HEADER_BG)
                GlobalZIndex(2001)
                on(stop_click)
                Children [
                    (Node { padding: UiRect::axes(px(2), px(2)) } Children [ label("Import Asset (path to a file)") ]),
                    (@FeathersTextInputContainer Children [
                        (@FeathersTextInput SeedText(String::new()) ImportPathInput AutoFocus)
                    ]),
                    (
                        Node { display: Display::Flex, flex_direction: FlexDirection::Row, column_gap: px(8) }
                        Children [
                            (@FeathersButton { @variant: ButtonVariant::Primary, @caption: bsn! { Text("Import") ThemedText } }
                                ImportConfirmButton),
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Cancel") ThemedText } }
                                on(|_: On<Activate>, mut c: Commands| { c.trigger(CloseOverlay); })),
                        ]
                    ),
                ]
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_asset::{AssetPath, LoadFromPath, UntypedHandle};
    use bevy_ecs::hierarchy::ChildOf;
    use bevy_reflect::Reflect;
    use core::any::TypeId;

    /// A custom, user-style reflected component to prove arbitrary components round-trip.
    #[derive(Component, Reflect, Default, Debug, PartialEq)]
    #[reflect(Component)]
    struct Tag {
        value: i32,
    }

    /// Deny-filtered scenes never store asset handles, so the loader is never called.
    struct NoLoader;
    impl LoadFromPath for NoLoader {
        fn load_from_path_erased(&mut self, _t: TypeId, _p: AssetPath<'static>) -> UntypedHandle {
            unreachable!("a deny-filtered scene contains no asset handles")
        }
    }

    fn registry() -> AppTypeRegistry {
        let reg = AppTypeRegistry::default();
        {
            let mut w = reg.write();
            w.register::<Transform>();
            w.register::<ChildOf>();
            w.register::<SceneEntity>();
            w.register::<SpawnedAs>();
            w.register::<Tag>();
        }
        reg
    }

    #[test]
    fn roundtrips_parent_links_and_arbitrary_components() {
        let reg = registry();

        let mut src = World::new();
        src.insert_resource(reg.clone());
        let parent = src
            .spawn((
                SceneEntity,
                SpawnedAs(SpawnKind::Empty),
                Transform::from_xyz(1.0, 2.0, 3.0),
            ))
            .id();
        src.spawn((
            SceneEntity,
            SpawnedAs(SpawnKind::Cube),
            Transform::default(),
            Tag { value: 42 },
            ChildOf(parent),
        ));

        let ron = scene_to_ron(&mut src).expect("serialize");

        // Deserialize into a completely fresh world (fresh entity ids).
        let mut dst = World::new();
        dst.insert_resource(reg.clone());
        let dynamic = {
            let registry = reg.read();
            let mut loader = NoLoader;
            let seed = WorldDeserializer {
                type_registry: &registry,
                load_from_path: &mut loader,
            };
            let mut de = ron::Deserializer::from_str(&ron).expect("ron");
            seed.deserialize(&mut de).expect("deserialize")
        };
        let mut map = EntityHashMap::default();
        dynamic.write_to_world(&mut dst, &mut map).expect("write");

        // Two scene entities restored.
        let mut scene_q = dst.query_filtered::<Entity, With<SceneEntity>>();
        assert_eq!(scene_q.iter(&dst).count(), 2);

        // The child's parent link survived and was remapped to the new parent entity.
        let mut child_q = dst.query_filtered::<&ChildOf, With<SceneEntity>>();
        let parents: Vec<Entity> = child_q.iter(&dst).map(ChildOf::parent).collect();
        assert_eq!(parents.len(), 1, "exactly one entity is parented");
        let restored_parent = parents[0];
        assert_eq!(
            dst.get::<Transform>(restored_parent).unwrap().translation,
            Vec3::new(1.0, 2.0, 3.0),
            "parent's saved transform survived"
        );

        // The arbitrary custom component value survived.
        let mut tag_q = dst.query::<&Tag>();
        let values: Vec<i32> = tag_q.iter(&dst).map(|t| t.value).collect();
        assert_eq!(values, vec![42]);
    }
}
