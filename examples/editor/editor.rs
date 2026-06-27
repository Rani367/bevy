//! A Unity/Godot-style GUI editor for Bevy, built on the Feathers widget toolkit.
//!
//! Run with:
//! ```sh
//! cargo run --example editor --features bevy_editor
//! ```
//!
//! The editor opens a paneled window — a menu bar and toolbar across the top, an
//! entity **Hierarchy** on the left, the rendered **Viewport** in the center, a
//! reflection-driven **Inspector** on the right, and an **Asset** browser along the
//! bottom. Use the *Entity* menu to spawn primitives, click them in the viewport to
//! select, edit their components in the inspector, drag them to move, then *Save* the
//! scene and *Play* to run it.

use bevy::{
    editor::{spawn_kind, EditorPlugins, EditorSelection, SpawnKind},
    picking::mesh_picking::MeshPickingPlugin,
    prelude::*,
    winit::{UpdateMode, WinitSettings},
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bevy Editor".into(),
                resolution: (1440u32, 900u32).into(),
                ..default()
            }),
            ..default()
        }))
        // Keep the editor updating even when the window isn't focused, so layout and
        // viewport rendering stay live.
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        })
        // A calm neutral background for the 3D viewport.
        .insert_resource(ClearColor(Color::srgb(0.09, 0.10, 0.13)))
        // Mesh picking lets us select 3D scene entities by clicking them in the viewport
        // (the viewport forwards pointer input into the offscreen render target). Sprite
        // and UI picking backends are already provided by `DefaultPlugins`.
        .add_plugins(MeshPickingPlugin)
        .add_plugins(EditorPlugins)
        // Start with a little demo content so the editor isn't empty on launch.
        .add_systems(Startup, spawn_demo_scene)
        .run();
}

/// Populate the scene with a couple of primitives and select one, so the hierarchy,
/// inspector, and viewport all have something to show immediately.
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
