//! Viewport selection: clicking a scene entity (forwarded through the `ViewportNode`
//! into the offscreen render target by `bevy_ui`'s `viewport_picking`) selects it, with
//! Ctrl/Cmd for additive selection. The selection is mirrored onto an [`EditorSelected`]
//! marker so other systems (hierarchy highlight, gizmo) can react.

use bevy_ecs::prelude::*;
use bevy_input::keyboard::KeyCode;
use bevy_input::ButtonInput;
use bevy_picking::events::{Click, Pointer};

use crate::markers::SceneEntity;
use crate::state::{EditorSelected, EditorSelection};

/// Global observer: when a scene entity is clicked in the viewport, select it.
pub fn select_on_click(
    click: On<Pointer<Click>>,
    scene_q: Query<(), With<SceneEntity>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut selection: ResMut<EditorSelection>,
) {
    let entity = click.entity;
    if !scene_q.contains(entity) {
        return;
    }
    let additive = keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight)
        || keys.pressed(KeyCode::SuperLeft)
        || keys.pressed(KeyCode::SuperRight);
    if additive {
        selection.toggle(entity);
    } else {
        selection.set_single(entity);
    }
}

/// Clear the selection when the user presses Escape.
pub fn clear_on_escape(keys: Res<ButtonInput<KeyCode>>, mut selection: ResMut<EditorSelection>) {
    if keys.just_pressed(KeyCode::Escape) {
        selection.clear();
    }
}

/// Keep the [`EditorSelected`] marker components in sync with the [`EditorSelection`]
/// resource so other systems can query selection directly.
pub fn sync_selected_marker(
    selection: Res<EditorSelection>,
    selected_q: Query<Entity, With<EditorSelected>>,
    mut commands: Commands,
) {
    if !selection.is_changed() {
        return;
    }
    for entity in selected_q.iter() {
        commands.entity(entity).remove::<EditorSelected>();
    }
    for &entity in selection.all.iter() {
        commands.entity(entity).insert(EditorSelected);
    }
}
