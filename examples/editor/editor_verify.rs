//! Headed verification harness for `bevy_editor` (hidden dev example).
//!
//! Runs the full editor, drives its real event API through a
//! spawn → reparent → duplicate → delete → undo×4 → redo → save → New → Open sequence, and
//! asserts the scene's entity count / parenting at each step, then exits. A panic (nonzero
//! exit) means a regression. This is a *behavioral* check of the editor's mutation, undo,
//! and scene-IO machinery; it does not validate rendering or pointer-driven gestures.
//!
//! ```sh
//! cargo run --example editor_verify --features bevy_editor
//! ```

use bevy::{
    ecs::hierarchy::ChildOf,
    editor::{
        spawn_kind, DeleteSelectedRequest, DuplicateRequest, EditorPlugins, EditorSelection,
        ReparentRequest, SceneEntity, SceneIoRequest, SpawnKind, SpawnRequest,
    },
    picking::mesh_picking::MeshPickingPlugin,
    prelude::*,
    winit::{UpdateMode, WinitSettings},
};

const VERIFY_SCENE: &str = "__editor_verify.ron";

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bevy Editor (verify)".into(),
                resolution: (1280u32, 800u32).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        })
        .insert_resource(ClearColor(Color::srgb(0.09, 0.10, 0.13)))
        .add_plugins(MeshPickingPlugin)
        .add_plugins(EditorPlugins)
        .add_systems(Startup, spawn_demo_scene)
        .add_systems(Update, verify_harness)
        .run();
}

/// The same demo content the `editor` example starts with: a cube + sphere (the viewport
/// plugin also spawns a directional light), so the initial scene has 3 entities.
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
        Transform::from_xyz(-1.2, 0.5, 0.0),
        "Cube",
    );
    spawn_kind(
        &mut commands,
        &mut meshes,
        &mut materials,
        SpawnKind::Sphere,
        Transform::from_xyz(1.2, 0.5, 0.0),
        "Sphere",
    );
    selection.set_single(cube);
}

/// Scripted assertion sequence. Steps are spaced 20 frames apart so each triggered action
/// (queued commands + observers) is fully applied before the next checkpoint reads state.
fn verify_harness(
    mut frame: Local<u32>,
    scene_q: Query<Entity, With<SceneEntity>>,
    childof_q: Query<(), (With<SceneEntity>, With<ChildOf>)>,
    named: Query<(Entity, &Name), With<SceneEntity>>,
    mut selection: ResMut<EditorSelection>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::editor::prelude::{RequestRedo, RequestUndo};

    *frame += 1;
    let count = scene_q.iter().count();
    let parented = childof_q.iter().count();

    match *frame {
        30 => assert_eq!(count, 3, "init: expected 3 scene entities"),
        40 => {
            commands.trigger(SpawnRequest(SpawnKind::Sphere));
        }
        60 => {
            assert_eq!(count, 4, "after spawn");
            let cube = named.iter().find(|(_, n)| n.as_str() == "Cube").map(|(e, _)| e);
            let sphere = named
                .iter()
                .find(|(_, n)| n.as_str() == "Sphere")
                .map(|(e, _)| e);
            if let (Some(cube), Some(sphere)) = (cube, sphere) {
                commands.trigger(ReparentRequest {
                    child: sphere,
                    new_parent: Some(cube),
                });
            }
        }
        80 => {
            assert_eq!(parented, 1, "after reparent: one entity should be parented");
            commands.trigger(DuplicateRequest);
        }
        100 => {
            assert_eq!(count, 5, "after duplicate");
            commands.trigger(DeleteSelectedRequest);
        }
        120 => {
            assert_eq!(count, 4, "after delete");
            commands.trigger(RequestUndo);
        }
        140 => {
            assert_eq!(count, 5, "undo delete should restore the deleted entity");
            commands.trigger(RequestUndo);
        }
        160 => {
            assert_eq!(count, 4, "undo duplicate");
            commands.trigger(RequestUndo);
        }
        180 => {
            assert_eq!(parented, 0, "undo reparent should unparent");
            commands.trigger(RequestUndo);
        }
        200 => {
            assert_eq!(count, 3, "undo spawn should return to the initial scene");
            commands.trigger(RequestRedo);
        }
        220 => {
            assert_eq!(count, 4, "redo spawn");
            selection.clear();
            // Scene I/O round-trip: save → new → open.
            commands.trigger(SceneIoRequest::SaveAs(VERIFY_SCENE.into()));
        }
        250 => {
            commands.trigger(SceneIoRequest::New);
        }
        270 => {
            assert_eq!(count, 1, "New should clear to a single directional light");
            commands.trigger(SceneIoRequest::Open(VERIFY_SCENE.into()));
        }
        300 => {
            assert_eq!(count, 4, "Open should restore the saved scene");
        }
        320 => {
            let _ = std::fs::remove_file(format!("assets/scenes/{VERIFY_SCENE}"));
            let _ = std::fs::remove_dir("assets/scenes");
            println!("editor_verify: all checks passed");
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}
