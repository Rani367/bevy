//! Play / Pause / Stop. Entering play mode snapshots the scene with `DynamicWorld`
//! (asset handles stay valid in-memory, so meshes/materials are preserved); stopping
//! restores it. Game-logic systems run only while [`EditorState::Playing`]; a small demo
//! "spin" behavior makes play mode visible so Stop visibly snaps entities back.

use bevy_app::{App, Plugin, Update};
use bevy_ecs::entity::{Entity, EntityHashMap};
use bevy_ecs::prelude::*;
use bevy_ecs::reflect::AppTypeRegistry;
use bevy_log::error;
use bevy_mesh::Mesh3d;
use bevy_state::condition::in_state;
use bevy_state::state::{OnEnter, OnExit};
use bevy_time::Time;
use bevy_transform::components::Transform;
use bevy_world_serialization::{DynamicWorld, DynamicWorldBuilder};

use crate::markers::SceneEntity;
use crate::state::{EditorSelection, EditorState};

/// Holds the scene as it was just before play mode started, so Stop can restore it.
#[derive(Resource)]
struct PlayModeSnapshot(DynamicWorld);

/// Installs play-mode snapshot/restore and the demo game behavior.
pub struct PlayPlugin;

impl Plugin for PlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnExit(EditorState::Editing), take_snapshot)
            .add_systems(OnEnter(EditorState::Editing), restore_snapshot)
            .add_systems(
                Update,
                spin_during_play.run_if(in_state(EditorState::Playing)),
            );
    }
}

/// Snapshot all scene entities when leaving edit mode (i.e. when play starts).
fn take_snapshot(world: &mut World) {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let ids: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
        query.iter(world).collect()
    };

    let snapshot = {
        let registry = registry.read();
        DynamicWorldBuilder::from_world(world, &registry)
            .extract_entities(ids.into_iter())
            .build()
    };
    world.insert_resource(PlayModeSnapshot(snapshot));
}

/// Restore the snapshot when returning to edit mode (Stop): despawn the live scene and
/// write the captured one back.
fn restore_snapshot(world: &mut World) {
    let Some(snapshot) = world.remove_resource::<PlayModeSnapshot>() else {
        return;
    };

    let ids: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
        query.iter(world).collect()
    };
    for entity in ids {
        if let Ok(entity_mut) = world.get_entity_mut(entity) {
            entity_mut.despawn();
        }
    }

    let mut entity_map = EntityHashMap::default();
    if let Err(err) = snapshot.0.write_to_world(world, &mut entity_map) {
        error!("Failed to restore scene after play mode: {err}");
    }

    world.resource_mut::<EditorSelection>().clear();
}

/// Demo game behavior: spin every scene mesh while playing. Because Stop restores the
/// pre-play snapshot, the spin is visibly reverted.
fn spin_during_play(
    time: Res<Time>,
    mut meshes: Query<&mut Transform, (With<SceneEntity>, With<Mesh3d>)>,
) {
    let delta = time.delta_secs();
    for mut transform in meshes.iter_mut() {
        transform.rotate_y(delta);
    }
}
