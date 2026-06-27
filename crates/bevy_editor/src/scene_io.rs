//! Scene file save/load plus the asset browser. Scenes are stored in a small
//! editor-controlled RON format (`assets/scenes/*.ron`): one node per scene entity with
//! its [`SpawnKind`] and transform. On load each node is rebuilt with [`spawn_kind`], so
//! runtime-generated meshes/materials are recreated fresh rather than round-tripped
//! through asset handles (which don't survive a despawn). The in-memory play-mode
//! snapshot uses `DynamicWorld` instead, where asset handles stay valid.

use bevy_app::{App, Plugin, Update};
use bevy_asset::{AssetServer, Assets, Handle};
use bevy_camera::visibility::Visibility;
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
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
use bevy_math::{Quat, Vec3};
use bevy_mesh::Mesh;
use bevy_pbr::StandardMaterial;
use bevy_picking::events::{Click, Pointer};
use bevy_picking::Pickable;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_text::EditableText;
use bevy_transform::components::Transform;
use bevy_ui::widget::{ImageNode, Text};
use bevy_ui::{
    percent, px, AlignItems, Display, FlexDirection, GlobalZIndex, JustifyContent, Node, Overflow,
    PositionType, UiRect,
};
use bevy_ui_widgets::{Activate, ScrollArea};
use bevy_window::SystemCursorIcon;
use serde::{Deserialize, Serialize};

use crate::actions::{OpenImportDialog, OpenOpenDialog, OpenSaveDialog, SceneIoRequest, SpawnKind};
use crate::markers::{EditorEntity, SceneEntity};
use crate::spawning::{spawn_kind, SpawnedAs};
use crate::state::EditorSelection;
use crate::ui::{stop_click, AssetContent, CloseOverlay, EditorOverlay, SeedText};
use crate::undo::push_undo;

/// Directory (relative to the working dir) where scene files live.
const SCENES_DIR: &str = "assets/scenes";
/// Fallback file name used by *Save* when no scene has been named yet.
const DEFAULT_SCENE: &str = "scene.ron";

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

/// Serialized scene: a flat list of nodes.
#[derive(Serialize, Deserialize)]
struct EditorScene {
    nodes: Vec<EditorNode>,
}

/// One serialized scene entity. Beyond the spawn kind + transform, editor-editable
/// component data that the loader can re-apply is stored too (currently visibility).
#[derive(Serialize, Deserialize)]
struct EditorNode {
    name: String,
    kind: SpawnKind,
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
    #[serde(default = "default_visible")]
    visible: bool,
}

fn default_visible() -> bool {
    true
}

impl EditorNode {
    fn transform(&self) -> Transform {
        Transform {
            translation: Vec3::from_array(self.translation),
            rotation: Quat::from_array(self.rotation),
            scale: Vec3::from_array(self.scale),
        }
    }
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
    save_query: Query<(&Name, &Transform, &SpawnedAs, Option<&Visibility>), With<SceneEntity>>,
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
            let name = current
                .path
                .clone()
                .unwrap_or_else(|| DEFAULT_SCENE.to_string());
            if save_scene(&save_query, &name).is_ok() {
                current.path = Some(name);
                browser_dirty.0 = true;
            }
        }
        SceneIoRequest::SaveAs(name) => {
            if save_scene(&save_query, name).is_ok() {
                current.path = Some(name.clone());
                browser_dirty.0 = true;
            }
        }
        SceneIoRequest::Open(name) => match load_scene(name) {
            Ok(scene) => {
                push_undo(&mut commands);
                clear_scene(&scene_entities, &mut commands);
                selection.clear();
                for node in &scene.nodes {
                    let entity = spawn_kind(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        node.kind,
                        node.transform(),
                        node.name.clone(),
                    );
                    if !node.visible {
                        commands.entity(entity).insert(Visibility::Hidden);
                    }
                }
                current.path = Some(name.clone());
                info!("Opened scene '{name}' ({} entities)", scene.nodes.len());
            }
            Err(err) => error!("Failed to open scene '{name}': {err}"),
        },
        SceneIoRequest::Instantiate(name) => match load_scene(name) {
            Ok(scene) => {
                push_undo(&mut commands);
                let mut last = None;
                for node in &scene.nodes {
                    let entity = spawn_kind(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        node.kind,
                        node.transform(),
                        node.name.clone(),
                    );
                    if !node.visible {
                        commands.entity(entity).insert(Visibility::Hidden);
                    }
                    last = Some(entity);
                }
                if let Some(entity) = last {
                    selection.set_single(entity);
                }
                info!("Instantiated '{name}' (+{} entities)", scene.nodes.len());
            }
            Err(err) => error!("Failed to instantiate '{name}': {err}"),
        },
    }
}

fn clear_scene(scene_entities: &Query<Entity, With<SceneEntity>>, commands: &mut Commands) {
    for entity in scene_entities.iter() {
        commands.entity(entity).despawn();
    }
}

fn save_scene(
    save_query: &Query<(&Name, &Transform, &SpawnedAs, Option<&Visibility>), With<SceneEntity>>,
    name: &str,
) -> Result<(), String> {
    let nodes: Vec<EditorNode> = save_query
        .iter()
        .map(|(entity_name, transform, spawned, visibility)| EditorNode {
            name: entity_name.as_str().to_string(),
            kind: spawned.0,
            translation: transform.translation.to_array(),
            rotation: transform.rotation.to_array(),
            scale: transform.scale.to_array(),
            visible: !matches!(visibility, Some(Visibility::Hidden)),
        })
        .collect();

    let ron = ron::ser::to_string_pretty(&EditorScene { nodes }, ron::ser::PrettyConfig::default())
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(SCENES_DIR).map_err(|e| e.to_string())?;
    let path = format!("{SCENES_DIR}/{name}");
    std::fs::write(&path, ron).map_err(|e| e.to_string())?;
    info!("Saved scene to {path}");
    Ok(())
}

fn load_scene(name: &str) -> Result<EditorScene, String> {
    let path = format!("{SCENES_DIR}/{name}");
    let ron = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    ron::from_str::<EditorScene>(&ron).map_err(|e| e.to_string())
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
                && name.ends_with(".ron")
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
    let mut name = text.trim().to_string();
    if name.is_empty() {
        return;
    }
    if !name.ends_with(".ron") {
        name.push_str(".ron");
    }
    commands.trigger(SceneIoRequest::SaveAs(name));
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
