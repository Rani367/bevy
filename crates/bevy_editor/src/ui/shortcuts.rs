//! Global editor keyboard shortcuts. Maps keys to the same action events the menus and
//! toolbar emit. Single-key shortcuts are suppressed while a text field is focused so they
//! don't fire mid-typing; modifier combos always work.

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_input::keyboard::KeyCode;
use bevy_input::ButtonInput;
use bevy_input_focus::InputFocus;
use bevy_text::EditableText;

use crate::actions::{
    DeleteSelectedRequest, DuplicateRequest, OpenOpenDialog, OpenSaveDialog, RenameRequest,
    SceneIoRequest,
};
use crate::state::{EditorSelection, GizmoMode};
use crate::ui::{OpenCommandPalette, ToggleConsole};
use crate::undo::{RequestRedo, RequestUndo};
use crate::viewport::FrameSelectionRequest;

fn editor_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    editables: Query<(), With<EditableText>>,
    selection: Res<EditorSelection>,
    mut gizmo: ResMut<GizmoMode>,
    mut commands: Commands,
) {
    let cmd = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);

    // Modifier combos work even while typing.
    if cmd {
        if keys.just_pressed(KeyCode::KeyS) {
            if shift {
                commands.trigger(OpenSaveDialog);
            } else {
                commands.trigger(SceneIoRequest::Save);
            }
        }
        if keys.just_pressed(KeyCode::KeyN) {
            commands.trigger(SceneIoRequest::New);
        }
        if keys.just_pressed(KeyCode::KeyO) {
            commands.trigger(OpenOpenDialog);
        }
        if keys.just_pressed(KeyCode::KeyD) {
            commands.trigger(DuplicateRequest);
        }
        if keys.just_pressed(KeyCode::KeyP) {
            commands.trigger(OpenCommandPalette);
        }
        if keys.just_pressed(KeyCode::KeyZ) {
            if shift {
                commands.trigger(RequestRedo);
            } else {
                commands.trigger(RequestUndo);
            }
        }
    }

    // Single-key shortcuts are disabled while editing a text field.
    let typing = focus.get().is_some_and(|e| editables.contains(e));
    if typing || cmd {
        return;
    }

    if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        commands.trigger(DeleteSelectedRequest);
    }
    if keys.just_pressed(KeyCode::F2)
        && let Some(primary) = selection.primary
    {
        commands.trigger(RenameRequest(primary));
    }
    if keys.just_pressed(KeyCode::KeyW) {
        *gizmo = GizmoMode::Translate;
    }
    if keys.just_pressed(KeyCode::KeyE) {
        *gizmo = GizmoMode::Rotate;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        *gizmo = GizmoMode::Scale;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        commands.trigger(FrameSelectionRequest);
    }
    if keys.just_pressed(KeyCode::Backquote) {
        commands.trigger(ToggleConsole);
    }
}

/// Installs the global keyboard shortcut router.
pub struct ShortcutsPlugin;

impl Plugin for ShortcutsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, editor_shortcuts);
    }
}
