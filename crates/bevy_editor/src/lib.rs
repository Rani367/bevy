//! `bevy_editor` is a GUI editor for the Bevy Engine — a Unity/Godot-style visual
//! tool for authoring scenes: a viewport, an entity hierarchy, a reflection-driven
//! component inspector, transform gizmos, scene save/load, and play/pause.
//!
//! It is built entirely on existing Bevy infrastructure: [`bevy_feathers`] for the
//! themed UI, [`bevy_ui`]'s `ViewportNode` to embed the rendered scene in a panel,
//! [`bevy_reflect`] for generic component editing, [`bevy_world_serialization`] for
//! scenes, and [`bevy_picking`]/[`bevy_gizmos`] for selection and manipulation.
//!
//! Add [`EditorPlugins`] to an `App` (alongside `DefaultPlugins` and the picking
//! backends) to launch the editor. See the `editor` example.
//!
//! ## Warning: Experimental!
//! Like the Feathers toolkit it is built on, this crate is experimental and
//! unfinished. APIs will change in breaking ways.

mod actions;
mod markers;
mod play;
mod spawning;
mod state;

pub mod hierarchy;
pub mod inspector;
pub mod scene_io;
pub mod ui;
pub mod viewport;

pub use actions::*;
pub use markers::*;
pub use spawning::*;
pub use state::*;

use bevy_app::{App, Plugin, PluginGroup, PluginGroupBuilder};
use bevy_dev_tools::infinite_grid::InfiniteGridPlugin;
use bevy_feathers::{dark_theme::create_dark_theme, theme::UiTheme, FeathersPlugins};
use bevy_state::app::AppExtStates;

/// Convenient re-exports for editor users.
pub mod prelude {
    pub use crate::{
        markers::{EditorEntity, GameCamera, SceneEntity},
        state::{
            EditorSelected, EditorSelection, EditorState, GizmoMode, GizmoSpace, ViewportMode,
        },
        EditorPlugin, EditorPlugins,
    };
}

/// The full set of plugins that make up the editor. Add this to an `App` that
/// already has `DefaultPlugins`.
pub struct EditorPlugins;

impl PluginGroup for EditorPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            // Themed widgets + tab navigation.
            .add_group(FeathersPlugins)
            // Infinite reference grid in the 3D viewport.
            .add(InfiniteGridPlugin)
            // Core state, selection, reflect registrations.
            .add(EditorPlugin)
            // The paneled editor shell (menu bar, toolbar, panels).
            .add(ui::EditorUiPlugin)
            // Offscreen scene camera bound into the viewport panel.
            .add(viewport::ViewportPlugin)
            // Live entity tree + spawn/delete.
            .add(hierarchy::HierarchyPlugin)
            // Reflection-driven component inspector.
            .add(inspector::InspectorPlugin)
            // Scene save/load + asset browser.
            .add(scene_io::ScenePlugin)
            // Play / pause / stop with snapshot + restore.
            .add(play::PlayPlugin)
    }
}

/// The core editor plugin: installs the dark theme, editor state resources, and
/// reflect registrations. Added automatically by [`EditorPlugins`].
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(UiTheme(create_dark_theme()))
            .init_resource::<EditorSelection>()
            .init_resource::<GizmoMode>()
            .init_resource::<GizmoSpace>()
            .init_resource::<ViewportMode>()
            .init_state::<EditorState>();

        app.register_type::<SceneEntity>();
        app.register_type::<SpawnKind>();
        app.register_type::<SpawnedAs>();
    }
}
