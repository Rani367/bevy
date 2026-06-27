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
    let base = Color::srgb(0.8, 0.75, 0.7);
    let mut entity = commands.spawn((
        SceneEntity,
        SpawnedAs(kind),
        Name::new(name.into()),
        transform,
    ));
    match kind {
        SpawnKind::Cube => {
            entity.insert((
                Mesh3d(meshes.add(Cuboid::default())),
                MeshMaterial3d(materials.add(base)),
            ));
        }
        SpawnKind::Sphere => {
            entity.insert((
                Mesh3d(meshes.add(Sphere::new(0.5))),
                MeshMaterial3d(materials.add(base)),
            ));
        }
        SpawnKind::Plane => {
            entity.insert((
                Mesh3d(meshes.add(Cuboid::new(5.0, 0.02, 5.0))),
                MeshMaterial3d(materials.add(base)),
            ));
        }
        SpawnKind::PointLight => {
            entity.insert(PointLight {
                intensity: 1_000_000.0,
                ..Default::default()
            });
        }
        SpawnKind::DirectionalLight => {
            entity.insert(DirectionalLight::default());
        }
        SpawnKind::Sprite => {
            entity.insert(Sprite::from_color(
                Color::srgb(0.4, 0.6, 0.9),
                Vec2::splat(100.0),
            ));
        }
        SpawnKind::Empty => {
            entity.insert(Visibility::default());
        }
    }
    entity.id()
}
