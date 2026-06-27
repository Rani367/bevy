//! Shared scene-entity spawning, used by both the *Entity* menu (via the hierarchy
//! plugin) and the scene loader. Centralizing it means the editor scene format only has
//! to store a [`SpawnKind`] + transform per node; the geometry/material is rebuilt fresh
//! on load (so runtime-generated meshes don't need to round-trip through asset handles).

use bevy_asset::Assets;
use bevy_camera::visibility::Visibility;
use bevy_color::Color;
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use bevy_light::{DirectionalLight, PointLight};
use bevy_math::primitives::{Cuboid, Sphere};
use bevy_math::Vec2;
use bevy_mesh::{Mesh, Mesh3d};
use bevy_pbr::{MeshMaterial3d, StandardMaterial};
use bevy_reflect::Reflect;
use bevy_sprite::Sprite;
use bevy_transform::components::Transform;

use crate::actions::SpawnKind;
use crate::markers::SceneEntity;

/// Records which [`SpawnKind`] an entity was created from, so the scene serializer can
/// store it and the loader can rebuild it.
#[derive(Component, Reflect, Debug, Clone, Copy)]
#[reflect(Component)]
pub struct SpawnedAs(pub SpawnKind);

impl Default for SpawnedAs {
    fn default() -> Self {
        Self(SpawnKind::Empty)
    }
}

/// A human-friendly default name for a spawn kind.
pub fn default_name(kind: SpawnKind) -> &'static str {
    match kind {
        SpawnKind::Cube => "Cube",
        SpawnKind::Sphere => "Sphere",
        SpawnKind::Plane => "Plane",
        SpawnKind::PointLight => "Point Light",
        SpawnKind::DirectionalLight => "Directional Light",
        SpawnKind::Sprite => "Sprite",
        SpawnKind::Empty => "Empty",
    }
}

/// Spawn a scene entity of the given kind at `transform`, tagged so it appears in the
/// hierarchy and can be serialized. Returns the new entity.
pub fn spawn_kind(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    kind: SpawnKind,
    transform: Transform,
    name: impl Into<String>,
) -> Entity {
    let entity = commands
        .spawn((
            SceneEntity,
            SpawnedAs(kind),
            Name::new(name.into()),
            transform,
        ))
        .id();
    match kind {
        SpawnKind::PointLight => {
            commands.entity(entity).insert(PointLight {
                intensity: 1_000_000.0,
                ..Default::default()
            });
        }
        SpawnKind::DirectionalLight => {
            commands.entity(entity).insert(DirectionalLight::default());
        }
        SpawnKind::Empty => {
            commands.entity(entity).insert(Visibility::default());
        }
        // Mesh / sprite kinds get their runtime-built visuals from the shared helper.
        SpawnKind::Cube | SpawnKind::Sphere | SpawnKind::Plane | SpawnKind::Sprite => {
            apply_kind_visuals(commands, meshes, materials, kind, entity);
        }
    }
    entity
}

/// Re-create the runtime mesh / material / sprite for a [`SpawnKind`] on an existing entity.
///
/// Lights and empties carry only plain reflected data (which round-trips through scene
/// files), so they're handled by [`spawn_kind`]; this helper builds the parts that *can't*
/// round-trip — procedural mesh/material handles and the sprite — and so is reused by the
/// scene loader to rebuild visuals after deserializing an entity's `SpawnedAs`.
pub fn apply_kind_visuals(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    kind: SpawnKind,
    entity: Entity,
) {
    let base = Color::srgb(0.8, 0.75, 0.7);
    let mut e = commands.entity(entity);
    match kind {
        SpawnKind::Cube => {
            e.insert((
                Mesh3d(meshes.add(Cuboid::default())),
                MeshMaterial3d(materials.add(base)),
            ));
        }
        SpawnKind::Sphere => {
            e.insert((
                Mesh3d(meshes.add(Sphere::new(0.5))),
                MeshMaterial3d(materials.add(base)),
            ));
        }
        SpawnKind::Plane => {
            e.insert((
                Mesh3d(meshes.add(Cuboid::new(5.0, 0.02, 5.0))),
                MeshMaterial3d(materials.add(base)),
            ));
        }
        SpawnKind::Sprite => {
            e.insert(Sprite::from_color(
                Color::srgb(0.4, 0.6, 0.9),
                Vec2::splat(100.0),
            ));
        }
        SpawnKind::PointLight | SpawnKind::DirectionalLight | SpawnKind::Empty => {}
    }
}
