//! The editor UI shell: the paneled layout (menu bar, toolbar, Hierarchy /
//! Viewport / Inspector / Asset panels) built with `bevy_feathers` and the `bsn!`
//! scene macro, plus the custom splitter widget Feathers doesn't provide.

pub mod bottom_dock;
pub mod command_palette;
pub mod console;
pub mod docking;
pub mod icons;
mod shell;
pub mod shortcuts;
mod splitter;
pub mod status_bar;
pub mod style;
pub mod theme_switch;
pub mod toast;

pub use bottom_dock::{BottomDock, BottomTab, OutputContent, ShowBottomTab};
pub use docking::{DockState, Panel, PanelContent, PanelId};
pub use splitter::{ResizeSide, Splitter};
pub use toast::{ShowToast, ToastLevel};

/// Toggle between the light and dark editor themes. Handled by the theme-switch plugin.
#[derive(Event, Clone, Copy)]
pub struct ToggleTheme;

/// Toggle the console / log panel. Handled by the console plugin.
#[derive(Event, Clone, Copy)]
pub struct ToggleConsole;

/// Open the command palette. Handled by the command-palette plugin.
#[derive(Event, Clone, Copy)]
pub struct OpenCommandPalette;

use bevy_app::{App, Plugin, Startup, Update};
use bevy_ecs::prelude::*;
use bevy_input::keyboard::KeyCode;
use bevy_input::ButtonInput;
use bevy_picking::events::{Click, Pointer};
use bevy_scene::prelude::SpawnListSystem;
use bevy_text::EditableText;

/// Marker on the central panel node that should host the rendered scene. The viewport
/// plugin inserts a `ViewportNode` here once the offscreen camera exists.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ViewportSlot;

/// Marker on the scrollable container that holds the entity-hierarchy rows.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct HierarchyContent;

/// Marker on the container that holds the inspector's per-component sections.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct InspectorContent;

/// Marker on the container that holds the asset-browser entries.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct AssetContent;

/// Marker on the container that holds the scene-tab buttons.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct TabBarContent;

/// Marker on the editor's UI camera (the window camera that renders the panels).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct EditorUiCamera;

/// Seeds a freshly-spawned text input with initial text. Feathers' `FeathersTextInput`
/// builds an empty `EditableText`; place this alongside it (and pair with
/// `bevy_input_focus::AutoFocus` to focus it) to start editing from an existing value.
#[derive(Component, Clone, Default)]
pub struct SeedText(pub String);

/// Place alongside [`SeedText`] to make the seeded input a multi-line editor (Enter inserts
/// a newline instead of submitting), e.g. the script editor.
#[derive(Component, Default, Clone, Copy)]
pub struct MultilineSeed;

/// One-shot: when a seeded text input's `EditableText` first appears, replace it with one
/// containing the seed text (multi-line if [`MultilineSeed`] is present), then drop the marker.
fn seed_text_inputs(
    mut q: Query<(Entity, &SeedText, &mut EditableText, Has<MultilineSeed>), Added<EditableText>>,
    mut commands: Commands,
) {
    for (entity, seed, mut editable, multiline) in q.iter_mut() {
        let mut seeded = EditableText::new(&seed.0);
        if multiline {
            seeded.allow_newlines = true;
            seeded.visible_lines = Some(8.0);
            commands.entity(entity).remove::<MultilineSeed>();
        }
        *editable = seeded;
        commands.entity(entity).remove::<SeedText>();
    }
}

/// Read the current text of a text input entity, if it has one.
pub fn read_text_input(editables: &Query<&EditableText>, entity: Entity) -> Option<String> {
    editables.get(entity).ok().map(|e| e.value().to_string())
}

/// Marks a modal overlay (a full-screen backdrop hosting a floating dialog). Spawned by
/// feature plugins (scene Save/Open, asset import); dismissed centrally via [`CloseOverlay`]
/// or the Escape key.
#[derive(Component, Default, Clone, Copy)]
pub struct EditorOverlay;

/// Request to close any open editor overlay.
#[derive(Event, Clone, Copy)]
pub struct CloseOverlay;

/// Stop a click from bubbling to an overlay backdrop, so clicking inside a dialog panel
/// doesn't dismiss it.
pub fn stop_click(mut click: On<Pointer<Click>>) {
    click.propagate(false);
}

fn on_close_overlay(
    _: On<CloseOverlay>,
    overlays: Query<Entity, With<EditorOverlay>>,
    mut commands: Commands,
) {
    for overlay in overlays.iter() {
        commands.entity(overlay).despawn();
    }
}

fn close_overlay_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    overlays: Query<Entity, With<EditorOverlay>>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Escape) {
        for overlay in overlays.iter() {
            commands.entity(overlay).despawn();
        }
    }
}

/// Installs the editor shell: spawns the paneled layout at startup and wires the
/// splitter drag behavior and shared overlay handling.
pub struct EditorUiPlugin;

impl Plugin for EditorUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            docking::DockingPlugin,
            bottom_dock::BottomDockPlugin,
            status_bar::StatusBarPlugin,
            theme_switch::ThemeSwitchPlugin,
            shortcuts::ShortcutsPlugin,
            toast::ToastPlugin,
            command_palette::CommandPalettePlugin,
            console::ConsolePlugin,
        ))
        .add_systems(Startup, shell::editor_shell.spawn())
        .add_systems(
            Update,
            (
                seed_text_inputs,
                close_overlay_on_escape,
                shell::sync_toolbar_active,
            ),
        )
        .add_observer(on_close_overlay);
    }
}
