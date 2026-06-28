//! First-party gameplay systems that fill engine gaps Bevy doesn't ship: a lightweight
//! **physics** integrator, a **particle** emitter, and a **tilemap** builder. (Mature
//! third-party crates like `avian` / `bevy_hanabi` / `bevy_ecs_tilemap` target *released* Bevy and
//! don't build against this unreleased fork, so these minimal, dependency-free versions give the
//! editor real, demonstrable physics/particles/tilemaps that compile in-tree.)
//!
//! All three are plain reflected components, so they appear in the inspector's Add-Component
//! dialog and serialize with the scene. Physics + particles only run in [`EditorState::Playing`];
//! particles are transient (cleaned up on Stop). Tilemaps rebuild their tile sprites whenever the
//! [`Tilemap`] component changes.

use bevy_app::{App, Plugin, Update};
use bevy_asset::Assets;
use bevy_color::Color;
use bevy_ecs::hierarchy::ChildOf;
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use bevy_math::primitives::Sphere;
use bevy_math::{Vec2, Vec3};
use bevy_mesh::{Mesh, Mesh3d};
use bevy_pbr::{MeshMaterial3d, StandardMaterial};
use bevy_reflect::std_traits::ReflectDefault;
use bevy_reflect::Reflect;
use bevy_sprite::Sprite;
use bevy_state::state::{OnEnter, State};
use bevy_time::Time;
use bevy_transform::components::Transform;

use crate::actions::SpawnKind;
use crate::markers::SceneEntity;
use crate::spawning::spawn_kind;
use crate::state::EditorState;

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

/// A simple dynamic body: integrates velocity + gravity each play frame, with a ground-plane
/// bounce at `y = 0`.
#[derive(Component, Reflect, Debug, Clone)]
#[reflect(Component, Default)]
pub struct RigidBody {
    /// Current linear velocity (world units / second).
    pub velocity: Vec3,
    /// Multiplier on gravity (0 = floats, 1 = normal).
    pub gravity_scale: f32,
    /// Restitution at the ground plane (0 = stops, 1 = full bounce).
    pub bounciness: f32,
}

impl Default for RigidBody {
    fn default() -> Self {
        Self {
            velocity: Vec3::ZERO,
            gravity_scale: 1.0,
            bounciness: 0.4,
        }
    }
}

/// Emits short-lived particles while playing.
#[derive(Component, Reflect, Debug, Clone)]
#[reflect(Component, Default)]
pub struct ParticleEmitter {
    /// Particles spawned per second.
    pub rate: f32,
    /// Seconds each particle lives.
    pub lifetime: f32,
    /// Initial upward/outward speed.
    pub speed: f32,
    /// Internal spawn accumulator (carried between frames).
    pub accumulator: f32,
}

impl Default for ParticleEmitter {
    fn default() -> Self {
        Self {
            rate: 20.0,
            lifetime: 1.5,
            speed: 3.0,
            accumulator: 0.0,
        }
    }
}

/// A transient spawned particle (not part of the scene).
#[derive(Component)]
struct Particle {
    age: f32,
    lifetime: f32,
    velocity: Vec3,
}

/// A grid of tile sprites, rebuilt whenever this component changes.
#[derive(Component, Reflect, Debug, Clone)]
#[reflect(Component, Default)]
pub struct Tilemap {
    /// Number of columns.
    pub width: u32,
    /// Number of rows.
    pub height: u32,
    /// World size of each tile.
    pub tile_size: f32,
}

impl Default for Tilemap {
    fn default() -> Self {
        Self {
            width: 8,
            height: 6,
            tile_size: 32.0,
        }
    }
}

/// Marks a generated tile (child of a [`Tilemap`]); not serialized.
#[derive(Component)]
struct Tile;

const GRAVITY: f32 = -9.81;

// ---------------------------------------------------------------------------
// Spawn actions
// ---------------------------------------------------------------------------

/// Spawn a cube with a [`RigidBody`] that falls when you press play.
#[derive(Event, Clone, Copy)]
pub struct SpawnPhysicsCube;
/// Spawn a [`ParticleEmitter`].
#[derive(Event, Clone, Copy)]
pub struct SpawnParticleEmitter;
/// Spawn a [`Tilemap`].
#[derive(Event, Clone, Copy)]
pub struct SpawnTilemap;

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Installs the first-party physics / particles / tilemap systems.
pub struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<RigidBody>()
            .register_type::<ParticleEmitter>()
            .register_type::<Tilemap>()
            .add_systems(
                Update,
                (
                    integrate_physics,
                    emit_particles,
                    update_particles,
                    rebuild_tilemaps,
                ),
            )
            .add_systems(OnEnter(EditorState::Editing), clear_particles)
            .add_observer(on_spawn_physics_cube)
            .add_observer(on_spawn_emitter)
            .add_observer(on_spawn_tilemap);
    }
}

// ---------------------------------------------------------------------------
// Physics
// ---------------------------------------------------------------------------

fn integrate_physics(
    state: Res<State<EditorState>>,
    time: Res<Time>,
    mut bodies: Query<(&mut Transform, &mut RigidBody)>,
) {
    if *state.get() != EditorState::Playing {
        return;
    }
    let dt = time.delta_secs();
    for (mut transform, mut body) in bodies.iter_mut() {
        let g = GRAVITY * body.gravity_scale;
        body.velocity.y += g * dt;
        let v = body.velocity;
        transform.translation += v * dt;
        if transform.translation.y < 0.0 {
            transform.translation.y = 0.0;
            let b = body.bounciness;
            body.velocity.y = -body.velocity.y * b;
            // Kill tiny residual bounces so bodies settle.
            if body.velocity.y.abs() < 0.05 {
                body.velocity.y = 0.0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Particles
// ---------------------------------------------------------------------------

fn emit_particles(
    state: Res<State<EditorState>>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut emitters: Query<(&GlobalTransformProxy, &mut ParticleEmitter)>,
    mut commands: Commands,
) {
    if *state.get() != EditorState::Playing {
        return;
    }
    let dt = time.delta_secs();
    let mesh = meshes.add(Sphere::new(0.08));
    for (origin, mut emitter) in emitters.iter_mut() {
        emitter.accumulator += emitter.rate * dt;
        let speed = emitter.speed;
        let lifetime = emitter.lifetime;
        let pos = origin.translation;
        while emitter.accumulator >= 1.0 {
            emitter.accumulator -= 1.0;
            // Pseudo-random spread derived from the accumulator + time (no RNG dependency).
            let seed = (emitter.accumulator + time.elapsed_secs()) * 12.9898;
            let a = bevy_math::ops::sin(seed) * 43758.547;
            let angle = a.fract() * std::f32::consts::TAU;
            let vel = Vec3::new(
                bevy_math::ops::cos(angle) * 0.5,
                1.0,
                bevy_math::ops::sin(angle) * 0.5,
            ) * speed;
            let mat = materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.6, 0.1),
                emissive: Color::srgb(1.0, 0.5, 0.0).to_linear() * 4.0,
                ..Default::default()
            });
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat),
                Transform::from_translation(pos),
                Particle {
                    age: 0.0,
                    lifetime,
                    velocity: vel,
                },
            ));
        }
    }
}

/// Lightweight stand-in so the emitter can read its world position without requiring the full
/// transform-propagation type here; falls back to the local `Transform`.
type GlobalTransformProxy = Transform;

fn update_particles(
    state: Res<State<EditorState>>,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut Transform, &mut Particle)>,
    mut commands: Commands,
) {
    if *state.get() != EditorState::Playing {
        return;
    }
    let dt = time.delta_secs();
    for (entity, mut transform, mut particle) in particles.iter_mut() {
        particle.age += dt;
        if particle.age >= particle.lifetime {
            commands.entity(entity).despawn();
            continue;
        }
        let v = particle.velocity;
        transform.translation += v * dt;
        particle.velocity.y += GRAVITY * 0.3 * dt;
    }
}

fn clear_particles(particles: Query<Entity, With<Particle>>, mut commands: Commands) {
    for entity in particles.iter() {
        commands.entity(entity).despawn();
    }
}

// ---------------------------------------------------------------------------
// Tilemap
// ---------------------------------------------------------------------------

fn rebuild_tilemaps(
    changed: Query<(Entity, &Tilemap), Changed<Tilemap>>,
    tiles: Query<(Entity, &ChildOf), With<Tile>>,
    mut commands: Commands,
) {
    for (map_entity, map) in changed.iter() {
        // Remove old generated tiles for this map.
        for (tile, parent) in tiles.iter() {
            if parent.parent() == map_entity {
                commands.entity(tile).despawn();
            }
        }
        let w = map.width.min(128);
        let h = map.height.min(128);
        let size = map.tile_size.max(1.0);
        for y in 0..h {
            for x in 0..w {
                let checker = (x + y) % 2 == 0;
                let color = if checker {
                    Color::srgb(0.35, 0.35, 0.4)
                } else {
                    Color::srgb(0.25, 0.25, 0.3)
                };
                let px = (x as f32 - w as f32 / 2.0) * size;
                let py = (y as f32 - h as f32 / 2.0) * size;
                let tile = commands
                    .spawn((
                        Sprite::from_color(color, Vec2::splat(size * 0.95)),
                        Transform::from_xyz(px, py, 0.0),
                        Tile,
                    ))
                    .id();
                commands.entity(map_entity).add_child(tile);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Spawn observers
// ---------------------------------------------------------------------------

fn on_spawn_physics_cube(
    _: On<SpawnPhysicsCube>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let entity = spawn_kind(
        &mut commands,
        &mut meshes,
        &mut materials,
        SpawnKind::Cube,
        Transform::from_xyz(0.0, 4.0, 0.0),
        "Physics Cube",
    );
    commands.entity(entity).insert(RigidBody::default());
}

fn on_spawn_emitter(_: On<SpawnParticleEmitter>, mut commands: Commands) {
    commands.spawn((
        SceneEntity,
        Name::new("Particle Emitter"),
        Transform::from_xyz(0.0, 1.0, 0.0),
        crate::spawning::SpawnedAs(SpawnKind::Empty),
        ParticleEmitter::default(),
    ));
}

fn on_spawn_tilemap(_: On<SpawnTilemap>, mut commands: Commands) {
    commands.spawn((
        SceneEntity,
        Name::new("Tilemap"),
        Transform::default(),
        crate::spawning::SpawnedAs(SpawnKind::Empty),
        Tilemap::default(),
    ));
}
