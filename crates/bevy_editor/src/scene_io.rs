//! Scene file save/load plus the asset browser. Scenes are stored in a small
//! editor-controlled RON format (`assets/scenes/*.ron`): one node per scene entity with
//! its [`SpawnKind`] and transform. On load each node is rebuilt with [`spawn_kind`], so
//! runtime-generated meshes/materials are recreated fresh rather than round-tripped
//! through asset handles (which don't survive a despawn). The in-memory play-mode
//! snapshot uses `DynamicWorld` instead, where asset handles stay valid.

use bevy_app::{App, Plugin, Update};
use bevy_asset::Assets;
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use bevy_feathers::cursor::EntityCursor;
use bevy_feathers::display::{label, label_dim};
use bevy_feathers::theme::ThemeBackgroundColor;
use bevy_feathers::tokens;
use bevy_log::{error, info};
use bevy_math::{Quat, Vec3};
use bevy_mesh::Mesh;
use bevy_pbr::StandardMaterial;
use bevy_picking::events::{Click, Pointer};
use bevy_picking::Pickable;
use bevy_scene::prelude::*;
use bevy_transform::components::Transform;
use bevy_ui::{px, AlignItems, Node, UiRect};
use bevy_window::SystemCursorIcon;
use serde::{Deserialize, Serialize};

use crate::actions::{SceneIoRequest, SpawnKind};
use crate::markers::SceneEntity;
use crate::spawning::{spawn_kind, SpawnedAs};
use crate::state::EditorSelection;
use crate::ui::AssetContent;

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

/// One serialized scene entity.
#[derive(Serialize, Deserialize)]
struct EditorNode {
    name: String,
    kind: SpawnKind,
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
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
            .add_observer(on_asset_click);
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
    save_query: Query<(&Name, &Transform, &SpawnedAs), With<SceneEntity>>,
    mut current: ResMut<CurrentScene>,
    mut selection: ResMut<EditorSelection>,
    mut browser_dirty: ResMut<AssetBrowserDirty>,
) {
    match &*request {
        SceneIoRequest::New => {
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
                clear_scene(&scene_entities, &mut commands);
                selection.clear();
                for node in &scene.nodes {
                    spawn_kind(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        node.kind,
                        node.transform(),
                        node.name.clone(),
                    );
                }
                current.path = Some(name.clone());
                info!("Opened scene '{name}' ({} entities)", scene.nodes.len());
            }
            Err(err) => error!("Failed to open scene '{name}': {err}"),
        },
    }
}

fn clear_scene(scene_entities: &Query<Entity, With<SceneEntity>>, commands: &mut Commands) {
    for entity in scene_entities.iter() {
        commands.entity(entity).despawn();
    }
}

fn save_scene(
    save_query: &Query<(&Name, &Transform, &SpawnedAs), With<SceneEntity>>,
    name: &str,
) -> Result<(), String> {
    let nodes: Vec<EditorNode> = save_query
        .iter()
        .map(|(entity_name, transform, spawned)| EditorNode {
            name: entity_name.as_str().to_string(),
            kind: spawned.0,
            translation: transform.translation.to_array(),
            rotation: transform.rotation.to_array(),
            scale: transform.scale.to_array(),
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
    mut commands: Commands,
) {
    if !dirty.0 {
        return;
    }
    let Ok(content) = content_q.single() else {
        return; // panel not spawned yet; try again next frame
    };
    dirty.0 = false;

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

    commands.entity(content).despawn_children();
    if files.is_empty() {
        commands
            .entity(content)
            .queue_spawn_related_scenes::<Children>(vec![empty_hint()]);
        return;
    }
    let rows: Vec<_> = files.into_iter().map(asset_entry).collect();
    commands
        .entity(content)
        .queue_spawn_related_scenes::<Children>(rows);
}

fn empty_hint() -> impl Scene {
    bsn! {
        Node { padding: UiRect::axes(px(6), px(4)) }
        Children [ label_dim("No saved scenes") ]
    }
}

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

fn on_asset_click(click: On<Pointer<Click>>, entries: Query<&AssetEntry>, mut commands: Commands) {
    if let Ok(entry) = entries.get(click.entity) {
        commands.trigger(SceneIoRequest::Open(entry.name.clone()));
    }
}
