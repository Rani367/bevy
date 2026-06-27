//! Build / export actions: export the active scene to disk, and build + **package** the host
//! project by shelling out to `cargo`. The cargo build runs on a worker thread (so the editor
//! stays responsive); on success the built binary and the `assets/` directory are bundled into
//! a `dist/<binary>/` folder ready to ship, and the result is reported in a small modal.

use alloc::sync::Arc;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton};
use bevy_feathers::display::label;
use bevy_feathers::theme::ThemedText;
use bevy_log::{error, info};
use bevy_scene::prelude::*;
use bevy_ui::widget::Text;
use bevy_ui::{px, Display, FlexDirection, JustifyContent, Node};
use bevy_ui_widgets::Activate;

use crate::actions::SceneIoRequest;
use crate::ui::style::dialog_frame;
use crate::ui::{CloseOverlay, ShowToast};

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
    // Save emits its own success/failure toast.
    commands.trigger(SceneIoRequest::Save);
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
        // `--message-format=json` so we can find the produced binary's path.
        let output = Command::new("cargo")
            .args(["build", "--release", "--message-format=json"])
            .output();
        let result = match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                match last_executable(&stdout) {
                    Some(exe) => {
                        let exe = PathBuf::from(exe);
                        let out_dir = dist_dir(&exe);
                        match package_dist(&exe, Path::new("assets"), &out_dir) {
                            Ok(dir) => BuildOutput {
                                success: true,
                                summary: format!("Packaged to {}", dir.display()),
                            },
                            Err(err) => BuildOutput {
                                success: false,
                                summary: format!("build ok, packaging failed: {err}"),
                            },
                        }
                    }
                    None => BuildOutput {
                        success: true,
                        summary: "Build succeeded (no binary artifact to package)".into(),
                    },
                }
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let summary = stderr
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("(no output)")
                    .trim()
                    .to_string();
                BuildOutput {
                    success: false,
                    summary,
                }
            }
            Err(err) => BuildOutput {
                success: false,
                summary: format!("failed to launch cargo: {err}"),
            },
        };
        *slot.lock().unwrap() = Some(result);
    });
    commands.spawn_scene(status_overlay(
        "Building + packaging project (cargo build --release)…",
    ));
}

/// Pick up a finished build result and show it.
fn poll_build(mut status: ResMut<BuildStatus>, mut commands: Commands) {
    if !status.running {
        return;
    }
    let taken = status.result.lock().unwrap().take();
    if let Some(output) = taken {
        status.running = false;
        commands.trigger(CloseOverlay);
        if output.success {
            info!("Build succeeded: {}", output.summary);
            commands.trigger(ShowToast::success(format!(
                "Build succeeded — {}",
                output.summary
            )));
        } else {
            error!("Build failed: {}", output.summary);
            commands.trigger(ShowToast::error(format!(
                "Build failed — {}",
                output.summary
            )));
        }
    }
}

// ---------------------------------------------------------------------------
// Packaging
// ---------------------------------------------------------------------------

/// Find the path of the last produced executable in `cargo`'s JSON build output.
fn last_executable(json_lines: &str) -> Option<String> {
    const KEY: &str = "\"executable\":\"";
    json_lines
        .lines()
        .filter_map(|line| {
            let start = line.find(KEY)? + KEY.len();
            let rest = &line[start..];
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
        .next_back()
}

/// The output bundle directory for a built `binary` (`dist/<binary-stem>`).
fn dist_dir(binary: &Path) -> PathBuf {
    let stem = binary.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
    Path::new("dist").join(stem)
}

/// Bundle a built `binary` and the `assets` directory into `out` (a shippable folder).
fn package_dist(binary: &Path, assets: &Path, out: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let file_name = binary
        .file_name()
        .ok_or_else(|| "binary has no file name".to_string())?;
    std::fs::copy(binary, out.join(file_name)).map_err(|e| e.to_string())?;
    if assets.is_dir() {
        copy_dir_recursive(assets, &out.join("assets")).map_err(|e| e.to_string())?;
    }
    Ok(out.to_path_buf())
}

/// Recursively copy `src` into `dst`.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &dest)?;
        } else {
            std::fs::copy(&path, &dest)?;
        }
    }
    Ok(())
}

/// A centered modal showing a status message with a Close button.
fn status_overlay(message: &str) -> impl Scene {
    let message = message.to_string();
    dialog_frame(
        "Build",
        px(420),
        bsn! {
            (
                Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(10) }
                Children [
                    (label(message)),
                    (
                        Node { display: Display::Flex, flex_direction: FlexDirection::Row, justify_content: JustifyContent::End }
                        Children [
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Close") ThemedText } }
                                on(|_: On<Activate>, mut c: Commands| { c.trigger(CloseOverlay); })),
                        ]
                    ),
                ]
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{copy_dir_recursive, dist_dir, last_executable, package_dist};
    use std::path::Path;

    #[test]
    fn last_executable_finds_the_binary() {
        let json = concat!(
            "{\"reason\":\"compiler-artifact\",\"executable\":null}\n",
            "{\"reason\":\"compiler-artifact\",\"executable\":\"/p/target/release/game\"}\n",
            "{\"reason\":\"build-finished\",\"success\":true}\n"
        );
        assert_eq!(
            last_executable(json).as_deref(),
            Some("/p/target/release/game")
        );
        assert_eq!(last_executable("{}").as_deref(), None);
    }

    #[test]
    fn dist_dir_uses_binary_stem() {
        assert_eq!(
            dist_dir(Path::new("/x/target/release/mygame")),
            Path::new("dist/mygame")
        );
    }

    #[test]
    fn package_bundles_binary_and_assets() {
        let base = std::env::temp_dir().join("bevy_editor_pkg_test");
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("src");
        std::fs::create_dir_all(src.join("assets/sub")).unwrap();
        let bin = src.join("mygame");
        std::fs::write(&bin, b"ELF...").unwrap();
        std::fs::write(src.join("assets/a.txt"), b"a").unwrap();
        std::fs::write(src.join("assets/sub/b.txt"), b"b").unwrap();

        let out = base.join("dist/mygame");
        let result = package_dist(&bin, &src.join("assets"), &out).unwrap();
        assert_eq!(result, out);
        assert!(out.join("mygame").is_file(), "binary copied");
        assert!(out.join("assets/a.txt").is_file(), "top-level asset copied");
        assert!(
            out.join("assets/sub/b.txt").is_file(),
            "nested asset copied"
        );

        // copy_dir_recursive is exercised indirectly above; sanity-check it directly too.
        let dst2 = base.join("copy");
        copy_dir_recursive(&src.join("assets"), &dst2).unwrap();
        assert!(dst2.join("sub/b.txt").is_file());

        let _ = std::fs::remove_dir_all(&base);
    }
}
