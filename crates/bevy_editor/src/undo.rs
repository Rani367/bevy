//! Undo / redo, built on the in-memory scene snapshots from [`crate::snapshot`].
//!
//! The model is whole-scene snapshot based: before each mutating action the editor
//! captures the current scene onto an undo stack ([`push_undo`]); undo pops that stack,
//! pushes the *current* state onto the redo stack, and restores the popped snapshot.
//! `DynamicWorld` isn't `Clone`, but every capture builds a fresh snapshot, so owning a
//! `Vec` of them works. Restoring assigns new entity ids, so the selection is cleared and
//! the hierarchy / inspector rebuild via their own dirty flags.

use alloc::collections::VecDeque;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_input::keyboard::KeyCode;
use bevy_input::ButtonInput;
use bevy_state::condition::in_state;
use bevy_state::state::State;
use bevy_world_serialization::DynamicWorld;

use crate::snapshot::{restore_scene_snapshot, take_scene_snapshot};
use crate::state::{EditorSelection, EditorState};

/// Cap on retained undo snapshots, to bound memory.
const MAX_UNDO: usize = 100;

/// The undo / redo history. Each entry is a full `SceneEntity` snapshot.
#[derive(Resource, Default)]
pub struct UndoStack {
    undo: VecDeque<DynamicWorld>,
    redo: VecDeque<DynamicWorld>,
    /// Set while a restore is in progress so re-entrant captures are ignored.
    suspended: bool,
}

impl UndoStack {
    /// Whether there is anything to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    /// Whether there is anything to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

/// Request to undo the last action (triggered by the Edit menu and Cmd/Ctrl+Z).
#[derive(Event, Clone, Copy)]
pub struct RequestUndo;

/// Request to redo the last undone action (Edit menu, Cmd/Ctrl+Shift+Z, Ctrl+Y).
#[derive(Event, Clone, Copy)]
pub struct RequestRedo;

/// Installs the undo stack, hotkeys, and undo/redo observers.
pub struct UndoPlugin;

impl Plugin for UndoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UndoStack>()
            .add_systems(
                Update,
                undo_redo_hotkeys.run_if(in_state(EditorState::Editing)),
            )
            .add_observer(on_request_undo)
            .add_observer(on_request_redo);
    }
}

/// Capture the current scene onto the undo stack *before* a mutation. Call this on the
/// caller's `Commands` immediately before queueing the commands that perform the change,
/// so the snapshot (which runs as a queued command, in order) reflects the pre-mutation
/// state. Clears the redo stack, as a new edit invalidates any redo history.
pub fn push_undo(commands: &mut Commands) {
    commands.queue(|world: &mut World| {
        if world.resource::<UndoStack>().suspended {
            return;
        }
        // Only the authored scene is undoable; play mode has its own snapshot.
        if *world.resource::<State<EditorState>>().get() != EditorState::Editing {
            return;
        }
        let snapshot = take_scene_snapshot(world);
        let stack = &mut *world.resource_mut::<UndoStack>();
        stack.undo.push_back(snapshot);
        if stack.undo.len() > MAX_UNDO {
            stack.undo.pop_front();
        }
        stack.redo.clear();
    });
}

fn on_request_undo(_: On<RequestUndo>, state: Res<State<EditorState>>, mut commands: Commands) {
    if *state.get() != EditorState::Editing {
        return;
    }
    commands.queue(apply_undo);
}

fn on_request_redo(_: On<RequestRedo>, state: Res<State<EditorState>>, mut commands: Commands) {
    if *state.get() != EditorState::Editing {
        return;
    }
    commands.queue(apply_redo);
}

/// Pop the undo stack, push the current state onto redo, and restore the snapshot.
fn apply_undo(world: &mut World) {
    let Some(snapshot) = world.resource_mut::<UndoStack>().undo.pop_back() else {
        return;
    };
    let current = take_scene_snapshot(world);
    world.resource_mut::<UndoStack>().redo.push_back(current);
    restore_with_guard(world, &snapshot);
    world.resource_mut::<EditorSelection>().clear();
}

/// Pop the redo stack, push the current state onto undo, and restore the snapshot.
fn apply_redo(world: &mut World) {
    let Some(snapshot) = world.resource_mut::<UndoStack>().redo.pop_back() else {
        return;
    };
    let current = take_scene_snapshot(world);
    world.resource_mut::<UndoStack>().undo.push_back(current);
    restore_with_guard(world, &snapshot);
    world.resource_mut::<EditorSelection>().clear();
}

/// Restore a snapshot with the re-entrancy guard raised so the despawn/respawn churn
/// doesn't capture a spurious undo entry.
fn restore_with_guard(world: &mut World, snapshot: &DynamicWorld) {
    world.resource_mut::<UndoStack>().suspended = true;
    restore_scene_snapshot(world, snapshot);
    world.resource_mut::<UndoStack>().suspended = false;
}

/// Cmd/Ctrl+Z → undo, Cmd/Ctrl+Shift+Z or Ctrl+Y → redo.
fn undo_redo_hotkeys(keys: Res<ButtonInput<KeyCode>>, mut commands: Commands) {
    let ctrl = keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight)
        || keys.pressed(KeyCode::SuperLeft)
        || keys.pressed(KeyCode::SuperRight);
    if !ctrl {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if keys.just_pressed(KeyCode::KeyZ) {
        if shift {
            commands.trigger(RequestRedo);
        } else {
            commands.trigger(RequestUndo);
        }
    }
    if keys.just_pressed(KeyCode::KeyY) {
        commands.trigger(RequestRedo);
    }
}
