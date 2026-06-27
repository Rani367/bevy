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

extern crate alloc;

mod actions;
mod build_export;
mod markers;
mod play;
mod remote;
mod scripting;
mod snapshot;
mod spawning;
mod state;
mod tabs;
mod undo;

pub mod hierarchy;
pub mod inspector;
pub mod scene_io;
pub mod ui;
pub mod viewport;

pub use actions::*;
pub use markers::*;
pub use remote::{
    brp_despawn, brp_mutate, brp_query_entities, brp_request, brp_spawn, normalize_addr,
    parse_entity_ids,
};
pub use scripting::BehaviorScript;
pub use spawning::*;
pub use state::*;
pub use ui::console::editor_console_layer;

use bevy_app::{App, Plugin, PluginGroup, PluginGroupBuilder};
use bevy_asset::embedded_asset;
use bevy_dev_tools::infinite_grid::InfiniteGridPlugin;
use bevy_feathers::{theme::UiTheme, FeathersPlugins};
use bevy_state::app::AppExtStates;

/// Convenient re-exports for editor users.
pub mod prelude {
    pub use crate::{
        markers::{EditorEntity, GameCamera, SceneEntity},
        state::{
            EditorSelected, EditorSelection, EditorState, GizmoDrag, GizmoMode, GizmoSpace,
            ViewportMode,
        },
        undo::{RequestRedo, RequestUndo, UndoStack},
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
            // Undo / redo via scene snapshots.
            .add(undo::UndoPlugin)
            // Minimal behavior-script interpreter (play mode).
            .add(scripting::ScriptingPlugin)
            // Build / export (cargo build, scene export).
            .add(build_export::BuildExportPlugin)
            // Multi-scene tabs.
            .add(tabs::TabsPlugin)
            // Remote (BRP) inspection + editing over the Bevy Remote Protocol.
            .add(remote::RemotePlugin)
    }
}

/// The core editor plugin: installs the dark theme, editor state resources, and
/// reflect registrations. Added automatically by [`EditorPlugins`].
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        embed_editor_icons(app);

        app.insert_resource(UiTheme(ui::style::create_editor_theme()))
            .init_resource::<EditorSelection>()
            .init_resource::<GizmoMode>()
            .init_resource::<GizmoSpace>()
            .init_resource::<GizmoDrag>()
            .init_resource::<GizmoSnap>()
            .init_resource::<ViewportMode>()
            .init_state::<EditorState>();

        app.register_type::<SceneEntity>();
        app.register_type::<SpawnKind>();
        app.register_type::<SpawnedAs>();

        // Feathers doesn't pull in `ScrollAreaPlugin`, but the editor panels rely on it
        // for wheel-scrolling long hierarchies / inspectors. Add it once.
        if !app.is_plugin_added::<bevy_ui_widgets::ScrollAreaPlugin>() {
            app.add_plugins(bevy_ui_widgets::ScrollAreaPlugin);
        }
    }
}

/// Embed the editor's icon PNGs (`src/assets/icons/*.png`) into the asset registry. The
/// paths must be string literals in this file so `include_bytes!` resolves relative to
/// `src/`. Referenced via the `embedded://bevy_editor/...` constants in [`ui::icons`].
fn embed_editor_icons(app: &mut App) {
    macro_rules! embed {
        ($app:expr, $($stem:literal),* $(,)?) => {{
            $( embedded_asset!($app, concat!("assets/icons/", $stem, ".png")); )*
        }};
    }
    embed!(
        app,
        "play",
        "pause",
        "stop",
        "play-mode",
        "gizmo-move",
        "gizmo-rotate",
        "gizmo-scale",
        "cube",
        "square",
        "grid",
        "snap",
        "frame",
        "eye",
        "eye-off",
        "lock",
        "unlock",
        "sphere",
        "light",
        "dir-light",
        "camera",
        "sprite",
        "empty",
        "chevron-down",
        "chevron-right",
        "float",
        "dock",
        "list",
        "sliders",
        "folder-tree",
        "plus",
        "x",
        "close",
        "duplicate",
        "trash",
        "search",
        "undo",
        "redo",
        "save",
        "folder",
        "folder-open",
        "file",
        "file-plus",
        "image",
        "import",
        "code",
        "terminal",
        "command",
        "sun",
        "moon",
        "remote",
        "info",
        "warning",
        "error",
        "success",
        "check",
        "settings",
        "build",
        "export",
        "menu",
    );
}
