//! Play / Pause / Stop. Entering play mode snapshots the scene with `DynamicWorld`
//! (asset handles stay valid in-memory, so meshes/materials are preserved); stopping
//! restores it. Game-logic systems run only while [`EditorState::Playing`]; behavior is
//! data-driven via [`crate::scripting::BehaviorScript`], so Stop visibly snaps entities
//! back to their pre-play state.

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
use bevy_state::state::{OnEnter, OnExit};
use bevy_world_serialization::DynamicWorld;

use crate::snapshot::{restore_scene_snapshot, take_scene_snapshot};
use crate::state::{EditorSelection, EditorState};

/// Holds the scene as it was just before play mode started, so Stop can restore it.
#[derive(Resource)]
struct PlayModeSnapshot(DynamicWorld);

/// Installs play-mode snapshot/restore.
pub struct PlayPlugin;

impl Plugin for PlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnExit(EditorState::Editing), take_snapshot)
            .add_systems(OnEnter(EditorState::Editing), restore_snapshot);
    }
}

/// Snapshot all scene entities when leaving edit mode (i.e. when play starts).
fn take_snapshot(world: &mut World) {
    let snapshot = take_scene_snapshot(world);
    world.insert_resource(PlayModeSnapshot(snapshot));
}

/// Restore the snapshot when returning to edit mode (Stop): despawn the live scene and
/// write the captured one back.
fn restore_snapshot(world: &mut World) {
    let Some(snapshot) = world.remove_resource::<PlayModeSnapshot>() else {
        return;
    };
    restore_scene_snapshot(world, &snapshot.0);
    world.resource_mut::<EditorSelection>().clear();
}
