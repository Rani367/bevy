//! Headless behavioral verification for the editor: boots [`EditorPlugins`], spawns a small
//! demo scene, runs it for a fixed number of frames, and asserts core invariants — the shell is
//! built, the hierarchy lists exactly the scene entities, the inspector populates on selection,
//! and a runtime spawn flows through to both. Panics (failing the example) if any invariant is
//! violated; otherwise logs `editor_verify: all invariants held` and exits cleanly.
//!
//! Run with:
//! ```sh
//! cargo run --example editor_verify --features bevy_editor
//! ```
//! This complements the crate's unit tests and the `EDITOR_SCREENSHOT` visual harness with an
//! end-to-end "does the assembled editor actually work" check.

use bevy::{
    editor::{
        editor_console_layer, hierarchy::HierarchyRow, spawn_kind, ui::InspectorContent,
        EditorPlugins, EditorSelection, MainView, SceneEntity, SpawnKind, SpawnRequest,
    },
    log::LogPlugin,
    picking::mesh_picking::MeshPickingPlugin,
    prelude::*,
};

fn main() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Bevy Editor — Verify".into(),
                    resolution: (1280u32, 800u32).into(),
                    ..default()
                }),
                ..default()
            })
            .set(LogPlugin {
                custom_layer: editor_console_layer,
                ..default()
            }),
    )
    .insert_resource(ClearColor(Color::srgb(0.09, 0.10, 0.13)))
    .add_plugins(MeshPickingPlugin)
    .add_plugins(EditorPlugins)
    .init_resource::<VerifyState>()
    .add_systems(Startup, spawn_demo_scene)
    .add_systems(Update, verify_system);

    app.run();
}

/// Frame counter + bookkeeping for the staged verification.
#[derive(Resource, Default)]
struct VerifyState {
    frame: u32,
    scene_before_spawn: usize,
}

/// Drives the staged checks across frames (the hierarchy/inspector rebuild a frame or two after
/// the scene changes, so assertions are spaced out to let them settle).
fn verify_system(
    mut state: ResMut<VerifyState>,
    selection: Res<EditorSelection>,
    mut main_view: ResMut<MainView>,
    scene_q: Query<(), With<SceneEntity>>,
    rows_q: Query<(), With<HierarchyRow>>,
    inspector_q: Query<Entity, With<InspectorContent>>,
    children_q: Query<&Children>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    state.frame += 1;
    match state.frame {
        // The demo scene has been spawned and the shell built.
        40 => {
            assert!(
                inspector_q.single().is_ok(),
                "editor shell not built: no InspectorContent node"
            );
            assert!(
                selection.primary.is_some(),
                "demo scene should leave an entity selected"
            );
            let scene = scene_q.iter().count();
            assert!(scene >= 3, "demo scene should have ≥3 entities, got {scene}");
            state.scene_before_spawn = scene;

            // Spawn another entity at runtime; the hierarchy + selection should follow.
            commands.trigger(SpawnRequest(SpawnKind::Sphere));
        }
        // After the spawn + rebuild settle, check the structural invariants.
        90 => {
            let scene = scene_q.iter().count();
            let rows = rows_q.iter().count();
            assert_eq!(
                rows, scene,
                "hierarchy rows must match scene-entity count ({rows} vs {scene})"
            );
            assert_eq!(
                scene,
                state.scene_before_spawn + 1,
                "runtime SpawnRequest should add exactly one scene entity"
            );

            let inspector = inspector_q.single().expect("InspectorContent node");
            let inspector_rows = children_q
                .get(inspector)
                .map(|c| c.iter().count())
                .unwrap_or(0);
            assert!(
                inspector_rows > 0,
                "inspector should populate for the current selection"
            );

            // Toggle the center view to the code editor.
            *main_view = MainView::Code;
        }
        110 => {
            assert_eq!(
                *main_view,
                MainView::Code,
                "MainView toggle to Code should persist"
            );
            info!("editor_verify: all invariants held");
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

/// A minimal demo scene: two primitives + the editor's default light, with one selected.
fn spawn_demo_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut selection: ResMut<EditorSelection>,
) {
    let cube = spawn_kind(
        &mut commands,
        &mut meshes,
        &mut materials,
        SpawnKind::Cube,
        Transform::from_xyz(-1.0, 0.5, 0.0),
        "Cube",
    );
    spawn_kind(
        &mut commands,
        &mut meshes,
        &mut materials,
        SpawnKind::Sphere,
        Transform::from_xyz(1.0, 0.5, 0.0),
        "Sphere",
    );
    selection.set_single(cube);
}
