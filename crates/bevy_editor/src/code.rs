//! Coding the game *in the editor*, in Rust. The center area can switch between the scene
//! viewport and a **code editor** ([`MainView`]) that browses the active project's `src/**.rs`,
//! edits a file in a multi-line text area, and saves it. Cargo integration runs `cargo check`
//! (streaming clickable diagnostics into the Output dock) and `cargo run` (launching the game as
//! a child process whose output is captured), with a Stop button to kill the running game.
//!
//! This is the "make / run / code a game from scratch" loop: scaffold a project (see
//! [`crate::project`]), open and edit its Rust here, check for errors, and run it — without
//! leaving the editor. (Rich syntax highlighting and rust-analyzer completion are future work;
//! the editor stores plain UTF-8 and relies on `cargo check` for diagnostics.)

use alloc::sync::Arc;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::constants::{fonts, size};
use bevy_feathers::controls::{
    ButtonVariant, FeathersButton, FeathersTextInput, FeathersTextInputContainer,
    FeathersToolButton,
};
use bevy_feathers::display::{icon, label_dim};
use bevy_feathers::theme::{
    ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemeToken, ThemedText,
};
use bevy_feathers::tokens;
use bevy_picking::Pickable;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_text::{EditableText, FontSourceTemplate, FontWeight, TextFont};
use bevy_ui::widget::Text;
use bevy_ui::{px, AlignItems, Display, FlexDirection, Node, Overflow, UiRect};
use bevy_ui_widgets::{Activate, ScrollArea};

use crate::project::{ActiveProject, BuildProfile};
use crate::ui::style::{etokens, sizes, space};
use crate::ui::{
    icons, BottomTab, MultilineSeed, OutputContent, SeedText, ShowBottomTab, ShowToast,
};

// ---------------------------------------------------------------------------
// Main view (scene viewport vs code editor)
// ---------------------------------------------------------------------------

/// Which surface the center area shows.
#[derive(Resource, Default, PartialEq, Eq, Clone, Copy, Debug)]
pub enum MainView {
    /// The 3D/2D scene viewport.
    #[default]
    Scene,
    /// The Rust code editor.
    Code,
}

impl MainView {
    /// Flip between the scene and the code editor.
    pub fn toggle(&mut self) {
        *self = match *self {
            MainView::Scene => MainView::Code,
            MainView::Code => MainView::Scene,
        };
    }
}

/// Marks a center-area surface; shown only when [`MainView`] matches.
#[derive(Component, Clone, Copy)]
pub struct MainViewNode(pub MainView);
impl Default for MainViewNode {
    fn default() -> Self {
        Self(MainView::Scene)
    }
}

fn apply_main_view(view: Res<MainView>, mut nodes: Query<(&MainViewNode, &mut Node)>) {
    if !view.is_changed() {
        return;
    }
    for (tag, mut node) in nodes.iter_mut() {
        node.display = if tag.0 == *view {
            Display::Flex
        } else {
            Display::None
        };
    }
}

// ---------------------------------------------------------------------------
// Code editor state + events
// ---------------------------------------------------------------------------

/// The file currently open in the code editor.
#[derive(Resource, Default)]
pub struct CodeEditorState {
    /// Absolute path of the open file, if any.
    pub current: Option<PathBuf>,
}

/// Open a source file in the code editor.
#[derive(Event, Clone)]
pub struct OpenCodeFileRequest {
    /// Absolute path of the file.
    pub path: PathBuf,
    /// Optional 1-based line to scroll the caret near (from a diagnostic).
    pub line: Option<u32>,
}

/// Save the code editor's buffer back to the open file.
#[derive(Event, Clone, Copy)]
pub struct SaveCodeFileRequest;

/// Rebuild the code editor's file list from the active project.
#[derive(Event, Clone, Copy)]
pub struct RefreshFileListRequest;

/// Run `cargo check` on the active project and stream diagnostics into the Output dock.
#[derive(Event, Clone, Copy)]
pub struct CargoCheckRequest;

/// Build + run the active project (`cargo run`) as a child process.
#[derive(Event, Clone, Copy)]
pub struct RunGameRequest;

/// Kill the running game child process, if any.
#[derive(Event, Clone, Copy)]
pub struct StopGameRequest;

// ---------------------------------------------------------------------------
// Markers
// ---------------------------------------------------------------------------

/// The multi-line text area holding the open file's contents.
#[derive(Component, Default, Clone, Copy)]
struct CodeEditorInput;
/// The container the file-list buttons are spawned into.
#[derive(Component, Default, Clone, Copy)]
struct CodeFileList;
/// The label showing the open file's path.
#[derive(Component, Default, Clone, Copy)]
struct CodePathLabel;
/// A file-list button; opens the stored file.
#[derive(Component, Default, Clone)]
struct OpenFileButton(PathBuf);
/// A clickable Output row that jumps to a source location.
#[derive(Component, Default, Clone)]
struct OutputJump {
    path: PathBuf,
    line: u32,
}

// ---------------------------------------------------------------------------
// Cargo output (shared with the Output dock tab)
// ---------------------------------------------------------------------------

/// Severity of an output line (drives its color).
#[derive(Clone, Copy, PartialEq, Eq)]
enum OutLevel {
    Info,
    Warn,
    Error,
}

impl OutLevel {
    fn token(self) -> ThemeToken {
        match self {
            OutLevel::Error => etokens::ERROR,
            OutLevel::Warn => etokens::WARNING,
            OutLevel::Info => tokens::TEXT_MAIN,
        }
    }
}

/// One line of cargo/program output.
#[derive(Clone)]
struct OutputLine {
    level: OutLevel,
    text: String,
    /// `Some((file, line))` makes the row clickable, jumping to that source location.
    jump: Option<(PathBuf, u32)>,
}

/// Shared, thread-written buffer of cargo/run output drained by the UI.
#[derive(Resource, Clone, Default)]
struct OutputLog(Arc<Mutex<Vec<OutputLine>>>);

impl OutputLog {
    fn clear(&self) {
        if let Ok(mut v) = self.0.lock() {
            v.clear();
        }
    }
    fn push(&self, line: OutputLine) {
        if let Ok(mut v) = self.0.lock() {
            v.push(line);
            // Keep the buffer bounded.
            let len = v.len();
            if len > 2000 {
                v.drain(0..len - 2000);
            }
        }
    }
}

/// Tracks an in-flight `cargo check` so two don't overlap.
#[derive(Resource, Default)]
struct CargoStatus {
    checking: bool,
}

/// Handle to the currently-running game child process (if any).
#[derive(Resource, Default)]
struct RunningGame(Arc<Mutex<Option<Child>>>);

/// Last-rendered output length, so the Output tab only rebuilds on change.
#[derive(Resource, Default)]
struct OutputRendered(usize);

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Installs the code editor + cargo tooling.
pub struct CodePlugin;

impl Plugin for CodePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MainView>()
            .init_resource::<CodeEditorState>()
            .init_resource::<OutputLog>()
            .init_resource::<CargoStatus>()
            .init_resource::<RunningGame>()
            .init_resource::<OutputRendered>()
            .add_systems(
                Update,
                (
                    apply_main_view,
                    refresh_file_list_on_project_change,
                    render_output,
                    poll_cargo_check,
                    reap_finished_game,
                ),
            )
            .add_observer(on_open_code_file)
            .add_observer(on_open_file_button)
            .add_observer(on_save_code_file)
            .add_observer(on_refresh_file_list)
            .add_observer(on_cargo_check)
            .add_observer(on_run_game)
            .add_observer(on_stop_game)
            .add_observer(on_output_jump);
    }
}

// ---------------------------------------------------------------------------
// Code editor panel scene
// ---------------------------------------------------------------------------

/// The code editor surface for the center area: a toolbar (path + Save/Check/Run/Stop), then a
/// file list beside the editor text area. Hidden unless [`MainView::Code`] is active.
pub fn code_panel() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            min_width: px(150),
            display: Display::None,
            flex_direction: FlexDirection::Column,
        }
        template_value(MainViewNode(MainView::Code))
        Children [
            code_header(),
            (
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                }
                Children [
                    (
                        Node {
                            width: px(200),
                            min_width: px(120),
                            display: Display::Flex,
                            flex_direction: FlexDirection::Column,
                            padding: space::SM,
                            row_gap: px(1),
                            overflow: Overflow::scroll_y(),
                            border: UiRect::right(px(1)),
                        }
                        ThemeBackgroundColor(tokens::PANE_BODY_BG)
                        ThemeBorderColor(etokens::PANEL_BORDER)
                        ScrollArea
                        CodeFileList
                    ),
                    (@FeathersTextInputContainer
                        Node { flex_grow: 1.0, min_width: px(0) }
                        Children [
                            (@FeathersTextInput SeedText(String::new()) MultilineSeed CodeEditorInput)
                        ]),
                ]
            ),
        ]
    }
}

fn code_header() -> impl Scene {
    bsn! {
        Node {
            min_height: sizes::PANEL_HEADER_H,
            padding: UiRect::horizontal(px(8)),
            align_items: AlignItems::Center,
            column_gap: px(6),
            border: UiRect::bottom(px(1)),
        }
        ThemeBackgroundColor(tokens::PANE_HEADER_BG)
        ThemeBorderColor(etokens::PANEL_BORDER)
        Children [
            (icon(icons::CODE) ThemedText),
            (Node { flex_grow: 1.0 } Children [ (label_dim("No file open") CodePathLabel) ]),
            (@FeathersToolButton { @caption: bsn! { (icon(icons::SAVE)) } }
                on(|_: On<Activate>, mut c: Commands| { c.trigger(SaveCodeFileRequest); })),
            (@FeathersToolButton { @caption: bsn! { (icon(icons::CHECK)) } }
                on(|_: On<Activate>, mut c: Commands| { c.trigger(CargoCheckRequest); })),
            (@FeathersToolButton { @variant: ButtonVariant::Primary, @caption: bsn! { (icon(icons::PLAY)) } }
                on(|_: On<Activate>, mut c: Commands| { c.trigger(RunGameRequest); })),
            (@FeathersToolButton { @caption: bsn! { (icon(icons::STOP)) } }
                on(|_: On<Activate>, mut c: Commands| { c.trigger(StopGameRequest); })),
        ]
    }
}

// ---------------------------------------------------------------------------
// File list
// ---------------------------------------------------------------------------

/// Collect editable source files (`*.rs`, `*.wgsl`) under `dir`, recursively (absolute paths).
fn list_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            list_rust_files(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs") | Some("wgsl")
        ) {
            out.push(path);
        }
    }
}

fn rebuild_file_list(world: &mut World) {
    let project = world.resource::<ActiveProject>();
    let root = project.root.clone();
    let src_dir = project.src_dir();
    let assets_dir = project.assets_dir();
    let mut files = Vec::new();
    list_rust_files(&src_dir, &mut files); // .rs game code
    list_rust_files(&assets_dir, &mut files); // .wgsl shaders
    files.sort();
    files.dedup();

    let mut list_q = world.query_filtered::<Entity, With<CodeFileList>>();
    let Some(list) = list_q.iter(world).next() else {
        return;
    };

    let rows: Vec<Box<dyn SceneList>> = if files.is_empty() {
        vec![Box::new(EntityScene(label_dim("No .rs / .wgsl files")))]
    } else {
        files
            .into_iter()
            .map(|path| {
                let display = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                Box::new(EntityScene(file_row(display, path))) as Box<dyn SceneList>
            })
            .collect()
    };
    world.entity_mut(list).despawn_children();
    world
        .entity_mut(list)
        .queue_spawn_related_scenes::<Children>(rows);
}

fn file_row(display: String, path: PathBuf) -> impl Scene {
    bsn! {
        (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! {
            (Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(6), padding: UiRect::axes(px(4), px(1)) }
                Children [ (icon(icons::FILE) ThemedText), (Text(display) ThemedText) ])
        } }
            template_value(OpenFileButton(path)))
    }
}

fn on_refresh_file_list(_: On<RefreshFileListRequest>, mut commands: Commands) {
    commands.queue(rebuild_file_list);
}

/// Rebuild the file list automatically when the active project changes (e.g. New/Open Project).
fn refresh_file_list_on_project_change(project: Res<ActiveProject>, mut commands: Commands) {
    if project.is_changed() {
        commands.queue(rebuild_file_list);
    }
}

fn on_open_file_button(act: On<Activate>, buttons: Query<&OpenFileButton>, mut commands: Commands) {
    if let Ok(button) = buttons.get(act.entity) {
        commands.trigger(OpenCodeFileRequest {
            path: button.0.clone(),
            line: None,
        });
    }
}

// ---------------------------------------------------------------------------
// Open / save
// ---------------------------------------------------------------------------

fn on_open_code_file(
    req: On<OpenCodeFileRequest>,
    mut state: ResMut<CodeEditorState>,
    mut inputs: Query<&mut EditableText, With<CodeEditorInput>>,
    mut labels: Query<&mut Text, With<CodePathLabel>>,
    mut view: ResMut<MainView>,
    mut commands: Commands,
) {
    let path = req.path.clone();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(err) => {
            commands.trigger(ShowToast::error(format!("Open failed: {err}")));
            return;
        }
    };
    if let Ok(mut editable) = inputs.single_mut() {
        let mut text = EditableText::new(&content);
        text.allow_newlines = true;
        text.visible_lines = Some(40.0);
        *editable = text;
    }
    if let Ok(mut label) = labels.single_mut() {
        *label = Text(path.display().to_string());
    }
    state.current = Some(path);
    // Switch to the code view so the freshly-opened file is visible.
    *view = MainView::Code;
}

fn on_save_code_file(
    _: On<SaveCodeFileRequest>,
    state: Res<CodeEditorState>,
    inputs: Query<&EditableText, With<CodeEditorInput>>,
    mut commands: Commands,
) {
    let Some(path) = state.current.clone() else {
        commands.trigger(ShowToast::warning("No file open"));
        return;
    };
    let Ok(editable) = inputs.single() else {
        return;
    };
    let content = editable.value().to_string();
    match std::fs::write(&path, content) {
        Ok(()) => commands.trigger(ShowToast::success(format!(
            "Saved {}",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
        ))),
        Err(err) => commands.trigger(ShowToast::error(format!("Save failed: {err}"))),
    }
}

// ---------------------------------------------------------------------------
// cargo check (diagnostics)
// ---------------------------------------------------------------------------

fn on_cargo_check(
    _: On<CargoCheckRequest>,
    mut status: ResMut<CargoStatus>,
    project: Res<ActiveProject>,
    log: Res<OutputLog>,
    mut commands: Commands,
) {
    if status.checking {
        return;
    }
    status.checking = true;
    log.clear();
    log.push(OutputLine {
        level: OutLevel::Info,
        text: "cargo check…".to_string(),
        jump: None,
    });
    commands.trigger(ShowBottomTab(BottomTab::Output));

    let root = project.root.clone();
    let log = log.clone();
    std::thread::spawn(move || {
        let output = Command::new("cargo")
            .args(["check", "--message-format=json"])
            .current_dir(&root)
            .output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let (errors, warnings) = parse_diagnostics(&stdout, &root, &log);
                let level = if errors > 0 {
                    OutLevel::Error
                } else if warnings > 0 {
                    OutLevel::Warn
                } else {
                    OutLevel::Info
                };
                log.push(OutputLine {
                    level,
                    text: format!("cargo check finished: {errors} error(s), {warnings} warning(s)"),
                    jump: None,
                });
            }
            Err(err) => log.push(OutputLine {
                level: OutLevel::Error,
                text: format!("failed to launch cargo: {err}"),
                jump: None,
            }),
        }
        CARGO_CHECK_DONE.store(true, std::sync::atomic::Ordering::SeqCst);
    });
}

/// Set true by the cargo-check worker thread when it finishes (so the main thread can clear the
/// `checking` flag without sharing the resource across the thread boundary).
static CARGO_CHECK_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn poll_cargo_check(mut status: ResMut<CargoStatus>) {
    if status.checking && CARGO_CHECK_DONE.swap(false, std::sync::atomic::Ordering::SeqCst) {
        status.checking = false;
    }
}

/// Parse `cargo --message-format=json` output into clickable [`OutputLine`]s. Returns
/// `(error_count, warning_count)`.
fn parse_diagnostics(stdout: &str, root: &Path, log: &OutputLog) -> (usize, usize) {
    let mut errors = 0;
    let mut warnings = 0;
    for line in stdout.lines() {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if val.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }
        let Some(msg) = val.get("message") else {
            continue;
        };
        let level_str = msg.get("level").and_then(|l| l.as_str()).unwrap_or("note");
        let level = match level_str {
            "error" | "error: internal compiler error" => {
                errors += 1;
                OutLevel::Error
            }
            "warning" => {
                warnings += 1;
                OutLevel::Warn
            }
            // Skip notes/help (usually attached to a parent diagnostic).
            _ => continue,
        };
        let text = msg
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        let jump = primary_span(msg).map(|(file, line)| {
            // Span file names are relative to the manifest dir.
            (root.join(file), line)
        });
        let display = match &jump {
            Some((path, line)) => format!(
                "{level_str}: {text}  ({}:{line})",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("?")
            ),
            None => format!("{level_str}: {text}"),
        };
        log.push(OutputLine {
            level,
            text: display,
            jump,
        });
    }
    (errors, warnings)
}

/// Extract the primary span's `(file_name, line_start)` from a compiler message.
fn primary_span(msg: &serde_json::Value) -> Option<(String, u32)> {
    let spans = msg.get("spans")?.as_array()?;
    let span = spans
        .iter()
        .find(|s| s.get("is_primary").and_then(serde_json::Value::as_bool) == Some(true))
        .or_else(|| spans.first())?;
    let file = span.get("file_name")?.as_str()?.to_string();
    let line = span.get("line_start")?.as_u64()? as u32;
    Some((file, line))
}

// ---------------------------------------------------------------------------
// cargo run / stop (the game as a child process)
// ---------------------------------------------------------------------------

fn on_run_game(
    _: On<RunGameRequest>,
    project: Res<ActiveProject>,
    running: Res<RunningGame>,
    log: Res<OutputLog>,
    mut commands: Commands,
) {
    // Don't launch a second instance.
    if running.0.lock().map(|g| g.is_some()).unwrap_or(false) {
        commands.trigger(ShowToast::warning("Game already running — Stop it first"));
        return;
    }
    log.clear();
    log.push(OutputLine {
        level: OutLevel::Info,
        text: "cargo run…".to_string(),
        jump: None,
    });
    commands.trigger(ShowBottomTab(BottomTab::Output));

    let root = project.root.clone();
    let profile = project.config.build.profile;
    let bin = project.config.build.bin.clone();

    let mut cmd = Command::new("cargo");
    cmd.arg("run");
    if profile == BuildProfile::Release {
        cmd.arg("--release");
    }
    if let Some(bin) = &bin {
        cmd.args(["--bin", bin]);
    }
    cmd.current_dir(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    match cmd.spawn() {
        Ok(mut child) => {
            // Stream stdout + stderr into the Output log on reader threads.
            if let Some(stdout) = child.stdout.take() {
                spawn_reader(stdout, OutLevel::Info, log.clone());
            }
            if let Some(stderr) = child.stderr.take() {
                spawn_reader(stderr, OutLevel::Warn, log.clone());
            }
            *running.0.lock().unwrap() = Some(child);
            commands.trigger(ShowToast::info("Launched game (cargo run)"));
        }
        Err(err) => {
            log.push(OutputLine {
                level: OutLevel::Error,
                text: format!("failed to launch cargo run: {err}"),
                jump: None,
            });
            commands.trigger(ShowToast::error(format!("Run failed: {err}")));
        }
    }
}

/// Spawn a thread that forwards each line from `reader` into the output `log`.
fn spawn_reader<R: std::io::Read + Send + 'static>(reader: R, level: OutLevel, log: OutputLog) {
    std::thread::spawn(move || {
        let buf = BufReader::new(reader);
        for line in buf.lines().map_while(Result::ok) {
            log.push(OutputLine {
                level,
                text: line,
                jump: None,
            });
        }
    });
}

fn on_stop_game(
    _: On<StopGameRequest>,
    running: Res<RunningGame>,
    log: Res<OutputLog>,
    mut commands: Commands,
) {
    let mut guard = running.0.lock().unwrap();
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
        log.push(OutputLine {
            level: OutLevel::Info,
            text: "Game stopped.".to_string(),
            jump: None,
        });
        commands.trigger(ShowToast::info("Stopped game"));
    } else {
        commands.trigger(ShowToast::warning("No game running"));
    }
}

/// Reap the child if it exited on its own, so a later Run isn't blocked by a stale handle.
fn reap_finished_game(running: Res<RunningGame>) {
    let mut guard = running.0.lock().unwrap();
    if let Some(child) = guard.as_mut()
        && matches!(child.try_wait(), Ok(Some(_)))
    {
        *guard = None;
    }
}

// ---------------------------------------------------------------------------
// Output dock rendering
// ---------------------------------------------------------------------------

/// Rebuild the Output tab rows when the buffer changes.
fn render_output(
    log: Res<OutputLog>,
    mut rendered: ResMut<OutputRendered>,
    content: Query<Entity, With<OutputContent>>,
    mut commands: Commands,
) {
    let Ok(container) = content.single() else {
        return;
    };
    let lines: Vec<OutputLine> = {
        let Ok(buf) = log.0.lock() else {
            return;
        };
        if buf.len() == rendered.0 {
            return;
        }
        rendered.0 = buf.len();
        buf.iter().rev().take(400).rev().cloned().collect()
    };
    let rows: Vec<Box<dyn SceneList>> = lines
        .iter()
        .map(|l| {
            if let Some((path, line)) = &l.jump {
                Box::new(EntityScene(output_jump_row(l, path.clone(), *line))) as Box<dyn SceneList>
            } else {
                Box::new(EntityScene(output_text_row(l)))
            }
        })
        .collect();
    commands.entity(container).despawn_children();
    commands
        .entity(container)
        .queue_spawn_related_scenes::<Children>(rows);
}

fn mono(token: ThemeToken, text: String) -> impl Scene {
    bsn! {
        (
            Text(text)
            TextFont {
                font: FontSourceTemplate::Handle(fonts::MONO),
                font_size: size::SMALL_FONT,
                weight: FontWeight::NORMAL,
            }
            bevy_app::PropagateOver<TextFont>
            ThemeTextColor(token)
        )
    }
}

fn output_text_row(line: &OutputLine) -> impl Scene {
    mono(line.level.token(), line.text.clone())
}

fn output_jump_row(line: &OutputLine, path: PathBuf, ln: u32) -> impl Scene {
    let token = line.level.token();
    let text = line.text.clone();
    bsn! {
        (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { (mono(token, text) Pickable::IGNORE) } }
            template_value(OutputJump { path, line: ln }))
    }
}

fn on_output_jump(act: On<Activate>, jumps: Query<&OutputJump>, mut commands: Commands) {
    if let Ok(jump) = jumps.get(act.entity) {
        commands.trigger(OpenCodeFileRequest {
            path: jump.path.clone(),
            line: Some(jump.line),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_view_toggles() {
        let mut v = MainView::Scene;
        v.toggle();
        assert_eq!(v, MainView::Code);
        v.toggle();
        assert_eq!(v, MainView::Scene);
    }

    #[test]
    fn lists_rust_files_recursively() {
        let base = std::env::temp_dir().join("bevy_editor_code_test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("a/b")).unwrap();
        std::fs::write(base.join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(base.join("a/lib.rs"), "// x").unwrap();
        std::fs::write(base.join("a/b/deep.rs"), "// y").unwrap();
        std::fs::write(base.join("a/notes.txt"), "ignored").unwrap();

        let mut files = Vec::new();
        list_rust_files(&base, &mut files);
        files.sort();
        assert_eq!(files.len(), 3, "found all .rs, skipped .txt");
        assert!(files.iter().all(|p| p.extension().unwrap() == "rs"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn parses_cargo_diagnostics_with_spans() {
        let log = OutputLog::default();
        let root = PathBuf::from("/proj");
        let json = concat!(
            r#"{"reason":"compiler-message","message":{"level":"error","message":"mismatched types","spans":[{"is_primary":true,"file_name":"src/main.rs","line_start":7,"column_start":5}]}}"#,
            "\n",
            r#"{"reason":"compiler-message","message":{"level":"warning","message":"unused variable","spans":[{"is_primary":true,"file_name":"src/main.rs","line_start":3}]}}"#,
            "\n",
            r#"{"reason":"compiler-message","message":{"level":"note","message":"ignored note","spans":[]}}"#,
            "\n",
            r#"{"reason":"build-finished","success":false}"#,
        );
        let (errors, warnings) = parse_diagnostics(json, &root, &log);
        assert_eq!((errors, warnings), (1, 1));
        let buf = log.0.lock().unwrap();
        assert_eq!(buf.len(), 2, "note skipped");
        assert_eq!(buf[0].jump, Some((PathBuf::from("/proj/src/main.rs"), 7)));
        assert!(buf[0].text.contains("mismatched types"));
        assert_eq!(buf[1].jump, Some((PathBuf::from("/proj/src/main.rs"), 3)));
    }
}
