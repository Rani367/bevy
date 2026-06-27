//! Shared scene snapshot/restore built on `bevy_world_serialization`'s `DynamicWorld`.
//!
//! Capturing the scene in-memory (rather than to a file) keeps asset handles valid, so
//! runtime-generated meshes/materials survive a round-trip. Both play-mode (snapshot on
//! play, restore on stop) and undo/redo (snapshot before each edit, restore on undo) use
//! these two functions, so the capture rules stay in one place.

use bevy_ecs::entity::{Entity, EntityHashMap};
use bevy_ecs::prelude::*;
use bevy_ecs::reflect::AppTypeRegistry;
use bevy_log::error;
use bevy_world_serialization::{DynamicWorld, DynamicWorldBuilder};

use crate::markers::SceneEntity;

/// Capture every [`SceneEntity`] (and its registered, reflectable components) into a
/// fresh [`DynamicWorld`]. `DynamicWorld` is not `Clone`, so each call builds a new one;
/// callers keep owned snapshots (e.g. on an undo stack).
pub fn take_scene_snapshot(world: &mut World) -> DynamicWorld {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let ids: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
        query.iter(world).collect()
    };
    let registry = registry.read();
    DynamicWorldBuilder::from_world(world, &registry)
        .extract_entities(ids.into_iter())
        .build()
}

/// Replace the live scene with `snapshot`: despawn every current [`SceneEntity`], then
/// write the captured entities back. `write_to_world` assigns fresh entity ids (remapped
/// through the [`EntityHashMap`]), so any [`Entity`] held elsewhere (e.g. a selection) is
/// invalidated and should be cleared by the caller.
pub fn restore_scene_snapshot(world: &mut World, snapshot: &DynamicWorld) {
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
    if let Err(err) = snapshot.write_to_world(world, &mut entity_map) {
        error!("Failed to restore scene snapshot: {err}");
    }
}
