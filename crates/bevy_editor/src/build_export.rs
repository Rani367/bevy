//! Build / export actions: export the active scene to disk, and build the host project by
//! shelling out to `cargo`. The cargo build runs on a worker thread (so the editor stays
//! responsive) and reports success/failure in a small modal. This is an honest minimal
//! pipeline — full asset bundling / packaging is future work.

use alloc::sync::Arc;
use std::process::Command;
use std::sync::Mutex;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton};
use bevy_feathers::display::label;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy_feathers::tokens;
use bevy_log::{error, info};
use bevy_picking::events::{Click, Pointer};
use bevy_scene::prelude::*;
use bevy_ui::widget::Text;
use bevy_ui::{
    percent, px, AlignItems, Display, FlexDirection, GlobalZIndex, JustifyContent, Node, Overflow,
    PositionType, UiRect,
};
use bevy_ui_widgets::{Activate, ScrollArea};

use crate::actions::SceneIoRequest;
use crate::markers::EditorEntity;
use crate::ui::{stop_click, CloseOverlay, EditorOverlay};

/// Request to build the host project via `cargo build`.
#[derive(Event, Clone, Copy)]
pub struct BuildProjectRequest;

/// Request to export (save) the active scene to disk.
#[derive(Event, Clone, Copy)]
pub struct ExportSceneRequest;

/// The result of a finished cargo build.
struct BuildOutput {
    success: bool,
    summary: String,
}

/// Tracks an in-flight build. The worker thread parks its result in `result`; `poll_build`
/// picks it up on the main thread.
#[derive(Resource, Default)]
struct BuildStatus {
    running: bool,
    result: Arc<Mutex<Option<BuildOutput>>>,
}

/// Installs the build/export actions.
pub struct BuildExportPlugin;

impl Plugin for BuildExportPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BuildStatus>()
            .add_systems(Update, poll_build)
            .add_observer(on_build_project)
            .add_observer(on_export_scene);
    }
}

/// Export = save the current scene to its file.
fn on_export_scene(_: On<ExportSceneRequest>, mut commands: Commands) {
    commands.trigger(SceneIoRequest::Save);
    commands.spawn_scene(status_overlay("Scene exported to assets/scenes/."));
}

/// Kick off `cargo build --release` on a worker thread (unless one is already running).
fn on_build_project(
    _: On<BuildProjectRequest>,
    mut status: ResMut<BuildStatus>,
    mut commands: Commands,
) {
    if status.running {
        return;
    }
    status.running = true;
    let slot = status.result.clone();
    std::thread::spawn(move || {
        let output = Command::new("cargo").args(["build", "--release"]).output();
        let result = match output {
            Ok(out) => {
                let success = out.status.success();
                let stderr = String::from_utf8_lossy(&out.stderr);
                // The last non-empty stderr line is cargo's summary ("Finished" / error).
                let summary = stderr
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("(no output)")
                    .trim()
                    .to_string();
                BuildOutput { success, summary }
            }
            Err(err) => BuildOutput {
                success: false,
                summary: format!("failed to launch cargo: {err}"),
            },
        };
        *slot.lock().unwrap() = Some(result);
    });
    commands.spawn_scene(status_overlay("Building project (cargo build --release)…"));
}

/// Pick up a finished build result and show it.
fn poll_build(mut status: ResMut<BuildStatus>, mut commands: Commands) {
    if !status.running {
        return;
    }
    let taken = status.result.lock().unwrap().take();
    if let Some(output) = taken {
        status.running = false;
        if output.success {
            info!("Build succeeded: {}", output.summary);
        } else {
            error!("Build failed: {}", output.summary);
        }
        commands.trigger(CloseOverlay);
        let label = if output.success {
            format!("Build succeeded\n{}", output.summary)
        } else {
            format!("Build failed\n{}", output.summary)
        };
        commands.spawn_scene(status_overlay(&label));
    }
}

/// A centered modal showing a status message with a Close button.
fn status_overlay(message: &str) -> impl Scene {
    let message = message.to_string();
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        EditorEntity
        EditorOverlay
        GlobalZIndex(2000)
        on(|_: On<Pointer<Click>>, mut c: Commands| { c.trigger(CloseOverlay); })
        Children [
            (
                Node {
                    width: px(360),
                    max_height: percent(60),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: px(12),
                    row_gap: px(10),
                    overflow: Overflow::scroll_y(),
                }
                EditorEntity
                ScrollArea
                ThemeBackgroundColor(tokens::PANE_HEADER_BG)
                GlobalZIndex(2001)
                on(stop_click)
                Children [
                    (Node { padding: UiRect::axes(px(2), px(2)) } Children [ label(message) ]),
                    (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Close") ThemedText } }
                        on(|_: On<Activate>, mut c: Commands| { c.trigger(CloseOverlay); })),
                ]
            ),
        ]
    }
}
