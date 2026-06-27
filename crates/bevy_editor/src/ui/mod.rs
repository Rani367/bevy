//! The editor UI shell: the paneled layout (menu bar, toolbar, Hierarchy /
//! Viewport / Inspector / Asset panels) built with `bevy_feathers` and the `bsn!`
//! scene macro, plus the custom splitter widget Feathers doesn't provide.

mod shell;
mod splitter;

pub use splitter::{ResizeSide, Splitter};

use bevy_app::{App, Plugin, Startup};
use bevy_ecs::prelude::*;
use bevy_scene::prelude::SpawnListSystem;

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

/// Marker on the editor's UI camera (the window camera that renders the panels).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct EditorUiCamera;

/// Installs the editor shell: spawns the paneled layout at startup and wires the
/// splitter drag behavior.
pub struct EditorUiPlugin;

impl Plugin for EditorUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, shell::editor_shell.spawn());
    }
}
