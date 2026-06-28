//! Basic UI / Control authoring. UI nodes spawned into the scene (see [`crate::actions::SpawnKind::UiNode`]
//! / `UiText`) are bound to the **viewport's** scene camera so they preview inside the viewport
//! panel instead of overlapping the editor's own chrome. The `UiTargetCamera` link is
//! runtime-only (excluded from scene files) and re-applied here after load or a 2D/3D switch, so
//! authored UI still renders into the right place.

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_ui::{Node, UiTargetCamera};

use crate::markers::{GameCamera, SceneEntity};

/// Installs the UI-edit support systems.
pub struct UiEditPlugin;

impl Plugin for UiEditPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, bind_ui_to_viewport);
    }
}

/// Ensure every scene UI node (`SceneEntity` + `Node`) targets the current viewport camera, so
/// game UI renders into the viewport image. Re-points stale links after a camera rebuild.
fn bind_ui_to_viewport(
    camera: Query<Entity, With<GameCamera>>,
    ui: Query<(Entity, Option<&UiTargetCamera>), (With<SceneEntity>, With<Node>)>,
    mut commands: Commands,
) {
    let Ok(cam) = camera.single() else {
        return;
    };
    for (entity, target) in ui.iter() {
        if target.map(|t| t.0) != Some(cam) {
            commands.entity(entity).insert(UiTargetCamera(cam));
        }
    }
}
