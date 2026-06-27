//! A minimal, built-in behavior-scripting subsystem.
//!
//! Bevy has no scripting runtime, so this is an explicit minimal scaffold rather than a
//! full embedded language: a [`BehaviorScript`] component holds a tiny line-based program
//! that animates the entity's [`Transform`] while the editor is in play mode. Each line is
//! one command, separated by newlines or `;` (the inspector's text input is single-line, so
//! `;` is handy there):
//!
//! - `spin <speed>`            — rotate about the world Y axis (radians/second)
//! - `rotate <x|y|z> <speed>`  — rotate about a world axis (radians/second)
//! - `translate <x> <y> <z>`   — move at the given velocity (units/second)
//! - `scale <factor>`          — set a uniform scale
//!
//! Because [`BehaviorScript`] is a reflected, serializable component, the reflection-driven
//! inspector renders its `source` field as an editable text box automatically, and it can be
//! attached to any entity via the inspector's "Add Component" dialog. This is intentionally
//! small; a real embedding (e.g. `rhai`) is future work.

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_math::Vec3;
use bevy_reflect::std_traits::ReflectDefault;
use bevy_reflect::Reflect;
use bevy_state::condition::in_state;
use bevy_time::Time;
use bevy_transform::components::Transform;

use crate::markers::SceneEntity;
use crate::state::EditorState;

/// A small behavior program that animates its entity's transform during play mode. See the
/// module docs for the command language.
#[derive(Component, Reflect, Default, Clone, Debug)]
#[reflect(Component, Default)]
pub struct BehaviorScript {
    /// The script source: one command per line (or per `;`).
    pub source: String,
}

/// Installs the behavior-script interpreter (runs only in play mode).
pub struct ScriptingPlugin;

impl Plugin for ScriptingPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<BehaviorScript>()
            .add_systems(Update, run_scripts.run_if(in_state(EditorState::Playing)));
    }
}

/// Execute every scene entity's behavior script for this frame.
fn run_scripts(
    time: Res<Time>,
    mut scripts: Query<(&BehaviorScript, &mut Transform), With<SceneEntity>>,
) {
    let dt = time.delta_secs();
    for (script, mut transform) in scripts.iter_mut() {
        for line in script.source.split([';', '\n']) {
            run_command(line.trim(), dt, &mut transform);
        }
    }
}

/// Run a single command line against a transform.
fn run_command(line: &str, dt: f32, transform: &mut Transform) {
    let mut parts = line.split_whitespace();
    match parts.next() {
        Some("spin") => {
            let speed = parse(parts.next(), 1.0);
            transform.rotate_y(speed * dt);
        }
        Some("rotate") => {
            let axis = parts.next().unwrap_or("y");
            let speed = parse(parts.next(), 1.0);
            let angle = speed * dt;
            match axis {
                "x" => transform.rotate_x(angle),
                "z" => transform.rotate_z(angle),
                _ => transform.rotate_y(angle),
            }
        }
        Some("translate") => {
            let v = Vec3::new(
                parse(parts.next(), 0.0),
                parse(parts.next(), 0.0),
                parse(parts.next(), 0.0),
            );
            transform.translation += v * dt;
        }
        Some("scale") => {
            let factor = parse(parts.next(), 1.0);
            transform.scale = Vec3::splat(factor);
        }
        _ => {}
    }
}

fn parse(token: Option<&str>, default: f32) -> f32 {
    token.and_then(|t| t.parse().ok()).unwrap_or(default)
}
