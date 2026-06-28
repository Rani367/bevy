//! Project model: an editor "project" is a directory containing a `project.bevy.ron`
//! config, a Cargo manifest, an `assets/` tree, and `src/` Rust sources. The editor can
//! **create a new Bevy game project from scratch**, **open** an existing one, and remembers
//! **recent** projects (persisted to a small editor-global file in the user's home dir).
//!
//! All path-based subsystems (scene I/O, asset browser, cargo build, the code editor) resolve
//! their locations against the [`ActiveProject`] root, so the editor can manage a project
//! living anywhere on disk without changing its own working directory. The root defaults to the
//! process working directory, so launching the editor inside a project "just works".

use std::path::{Path, PathBuf};

use bevy_app::{App, Plugin, Startup, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::controls::{
    ButtonVariant, FeathersButton, FeathersTextInput, FeathersTextInputContainer,
};
use bevy_feathers::display::{icon, label_dim};
use bevy_feathers::theme::ThemedText;
use bevy_input_focus::AutoFocus;
use bevy_log::{error, info};
use bevy_reflect::Reflect;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_text::EditableText;
use bevy_ui::widget::Text;
use bevy_ui::{px, AlignItems, Display, FlexDirection, JustifyContent, Node, UiRect};
use bevy_ui_widgets::Activate;
use serde::{Deserialize, Serialize};

use crate::ui::icons;
use crate::ui::style::dialog_frame;
use crate::ui::{CloseOverlay, SeedText, ShowToast};

/// Config-file name written at a project's root.
pub const PROJECT_FILE: &str = "project.bevy.ron";
/// The Bevy version a freshly-scaffolded project depends on. Edit the generated `Cargo.toml`
/// to track the engine version you want (e.g. a local path to this fork).
const TEMPLATE_BEVY_VERSION: &str = "0.16";

// ---------------------------------------------------------------------------
// Serialized project configuration
// ---------------------------------------------------------------------------

/// Which cargo profile the project builds/runs with.
#[derive(Reflect, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BuildProfile {
    /// `cargo build` (fast compile, slow runtime) — the default for iteration.
    #[default]
    Debug,
    /// `cargo build --release` (slow compile, fast runtime) — for shipping.
    Release,
}

impl BuildProfile {
    /// The cargo flag for this profile (`""` for debug, `"--release"` for release).
    pub fn cargo_flag(self) -> Option<&'static str> {
        match self {
            BuildProfile::Debug => None,
            BuildProfile::Release => Some("--release"),
        }
    }
}

/// Build/run settings for the project.
#[derive(Reflect, Serialize, Deserialize, Clone, Debug, Default)]
pub struct BuildSettings {
    /// Debug or release.
    pub profile: BuildProfile,
    /// Which binary target to run (`cargo run --bin <bin>`); `None` runs the default binary.
    pub bin: Option<String>,
    /// Cross-compile target triple (`--target <triple>`); `None` builds for the host.
    pub target: Option<String>,
}

/// A named input action mapped to a key — the seed of a Godot-style input-map editor. Stored in
/// the project config so games and the editor can share bindings (wired up in a later phase).
#[derive(Reflect, Serialize, Deserialize, Clone, Debug, Default)]
pub struct InputAction {
    /// Logical action name, e.g. `"jump"`.
    pub name: String,
    /// A key identifier, e.g. `"Space"` (matched to `KeyCode` by name).
    pub key: String,
}

/// The persisted project configuration (`project.bevy.ron`).
#[derive(Reflect, Serialize, Deserialize, Clone, Debug)]
pub struct ProjectConfig {
    /// Display name of the project.
    pub name: String,
    /// Scene loaded by default when the project opens (file name within `assets/scenes`).
    pub default_scene: Option<String>,
    /// Recently-opened scene files within this project (most-recent first).
    pub recent_scenes: Vec<String>,
    /// Build/run settings.
    pub build: BuildSettings,
    /// Input-map actions (seed for the input editor).
    pub input_actions: Vec<InputAction>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: "Untitled Project".to_string(),
            default_scene: None,
            recent_scenes: Vec::new(),
            build: BuildSettings::default(),
            input_actions: Vec::new(),
        }
    }
}

impl ProjectConfig {
    /// Parse a config from RON text.
    pub fn from_ron(text: &str) -> Result<Self, String> {
        ron::from_str(text).map_err(|e| e.to_string())
    }

    /// Serialize to pretty RON text.
    pub fn to_ron(&self) -> Result<String, String> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| e.to_string())
    }

    /// Record `scene` as the most-recently-opened scene (de-duplicated, capped).
    pub fn touch_recent_scene(&mut self, scene: &str) {
        self.recent_scenes.retain(|s| s != scene);
        self.recent_scenes.insert(0, scene.to_string());
        self.recent_scenes.truncate(12);
    }
}

// ---------------------------------------------------------------------------
// Active project (the editor's source of truth for where files live)
// ---------------------------------------------------------------------------

/// The project the editor is currently operating on. Every path-based subsystem resolves
/// against [`root`](Self::root); the default root is the process working directory.
#[derive(Resource, Clone, Debug)]
pub struct ActiveProject {
    /// Absolute (or working-dir-relative) path to the project directory.
    pub root: PathBuf,
    /// The loaded/edited configuration.
    pub config: ProjectConfig,
    /// Set when `config` has unsaved changes.
    pub config_dirty: bool,
}

impl Default for ActiveProject {
    fn default() -> Self {
        Self {
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            config: ProjectConfig::default(),
            config_dirty: false,
        }
    }
}

impl ActiveProject {
    /// Join a project-relative path onto the project root.
    pub fn join(&self, rel: impl AsRef<Path>) -> PathBuf {
        self.root.join(rel)
    }

    /// `<root>/project.bevy.ron`.
    pub fn config_path(&self) -> PathBuf {
        self.join(PROJECT_FILE)
    }

    /// `<root>/Cargo.toml`.
    pub fn manifest_path(&self) -> PathBuf {
        self.join("Cargo.toml")
    }

    /// `<root>/assets`.
    pub fn assets_dir(&self) -> PathBuf {
        self.join("assets")
    }

    /// `<root>/assets/scenes`.
    pub fn scenes_dir(&self) -> PathBuf {
        self.join("assets/scenes")
    }

    /// `<root>/src`.
    pub fn src_dir(&self) -> PathBuf {
        self.join("src")
    }

    /// Does this directory look like an initialized project (has a config file)?
    pub fn is_initialized(&self) -> bool {
        self.config_path().is_file()
    }

    /// Load the config from disk, replacing the in-memory copy. Best-effort: missing/invalid
    /// files leave the current config untouched and return an error string.
    pub fn load_config(&mut self) -> Result<(), String> {
        let path = self.config_path();
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        self.config = ProjectConfig::from_ron(&text)?;
        self.config_dirty = false;
        Ok(())
    }

    /// Write the config to `<root>/project.bevy.ron`.
    pub fn save_config(&mut self) -> Result<(), String> {
        std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let text = self.config.to_ron()?;
        std::fs::write(self.config_path(), text).map_err(|e| e.to_string())?;
        self.config_dirty = false;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Recent projects (editor-global, persisted to the user's home dir)
// ---------------------------------------------------------------------------

/// Editor-global list of recently-opened project roots (most-recent first). Persisted to
/// `~/.bevy_editor/recent_projects.ron` so it survives across sessions and projects.
#[derive(Resource, Default, Serialize, Deserialize, Clone, Debug)]
pub struct RecentProjects(pub Vec<PathBuf>);

impl RecentProjects {
    /// Move `root` to the front (de-duplicated, capped at 12).
    pub fn touch(&mut self, root: &Path) {
        self.0.retain(|p| p != root);
        self.0.insert(0, root.to_path_buf());
        self.0.truncate(12);
    }
}

/// The editor's data directory (`~/.bevy_editor`), if a home dir can be determined. Used for
/// editor-global state that isn't tied to a single project (recent projects, saved layouts).
pub fn editor_data_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| PathBuf::from(h).join(".bevy_editor"))
}

/// Path to the persisted recent-projects file.
fn recent_projects_path() -> Option<PathBuf> {
    editor_data_dir().map(|d| d.join("recent_projects.ron"))
}

fn load_recent_projects() -> RecentProjects {
    let Some(path) = recent_projects_path() else {
        return RecentProjects::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => ron::from_str(&text).unwrap_or_default(),
        Err(_) => RecentProjects::default(),
    }
}

fn save_recent_projects(recent: &RecentProjects) {
    let Some(path) = recent_projects_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = ron::ser::to_string_pretty(recent, ron::ser::PrettyConfig::default()) {
        let _ = std::fs::write(&path, text);
    }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// Open the "New Project" dialog (target directory + name).
#[derive(Event, Clone, Copy)]
pub struct OpenNewProjectDialog;

/// Open the "Open Project" dialog (path field + recent list).
#[derive(Event, Clone, Copy)]
pub struct OpenOpenProjectDialog;

/// Create a new Bevy project at `dir/name` and make it active.
#[derive(Event, Clone)]
pub struct CreateProjectRequest {
    /// Parent directory the new project folder is created in.
    pub parent_dir: PathBuf,
    /// Project (and crate) name; also the new folder's name.
    pub name: String,
}

/// Open an existing project at `root` and make it active.
#[derive(Event, Clone)]
pub struct OpenProjectRequest(pub PathBuf);

/// Persist the active project's config to disk.
#[derive(Event, Clone, Copy)]
pub struct SaveProjectRequest;

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Installs the project model: the [`ActiveProject`]/[`RecentProjects`] resources, the
/// New/Open/Save-Project actions, and their dialogs.
pub struct ProjectPlugin;

impl Plugin for ProjectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveProject>()
            .init_resource::<RecentProjects>()
            .register_type::<ProjectConfig>()
            .add_systems(Startup, load_on_startup)
            .add_systems(Update, sync_profile_buttons)
            .add_observer(on_open_new_project_dialog)
            .add_observer(on_open_open_project_dialog)
            .add_observer(on_create_project)
            .add_observer(on_open_project)
            .add_observer(on_save_project)
            .add_observer(on_new_project_confirm)
            .add_observer(on_open_project_confirm)
            .add_observer(on_recent_project_button)
            .add_observer(on_open_project_settings)
            .add_observer(on_settings_profile_button)
            .add_observer(on_settings_save)
            .add_observer(on_open_input_map)
            .add_observer(on_add_input_action)
            .add_observer(on_remove_input_action);
    }
}

/// On startup, load the working-dir project config (if present) and the recent-projects list.
fn load_on_startup(mut active: ResMut<ActiveProject>, mut recent: ResMut<RecentProjects>) {
    *recent = load_recent_projects();
    if active.is_initialized() {
        let root = active.root.clone();
        match active.load_config() {
            Ok(()) => {
                info!(
                    "Loaded project '{}' from {}",
                    active.config.name,
                    root.display()
                );
                recent.touch(&root);
                save_recent_projects(&recent);
            }
            Err(err) => error!("Failed to load {}: {err}", active.config_path().display()),
        }
    }
}

// ---------------------------------------------------------------------------
// Scaffolding
// ---------------------------------------------------------------------------

/// Create a new Bevy cargo project at `root`: `Cargo.toml`, `src/main.rs`, `assets/scenes/`,
/// and `project.bevy.ron`. Returns an error if `root` already contains a project file.
pub fn scaffold_project(root: &Path, name: &str) -> Result<(), String> {
    if root.join(PROJECT_FILE).exists() {
        return Err(format!("{} already contains a project", root.display()));
    }
    let crate_name = sanitize_crate_name(name);

    std::fs::create_dir_all(root.join("src")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(root.join("assets/scenes")).map_err(|e| e.to_string())?;

    std::fs::write(root.join("Cargo.toml"), cargo_toml_template(&crate_name))
        .map_err(|e| e.to_string())?;
    std::fs::write(root.join("src/main.rs"), main_rs_template()).map_err(|e| e.to_string())?;
    std::fs::write(root.join(".gitignore"), "/target\n/dist\n").map_err(|e| e.to_string())?;

    let config = ProjectConfig {
        name: name.to_string(),
        ..Default::default()
    };
    let text = config.to_ron()?;
    std::fs::write(root.join(PROJECT_FILE), text).map_err(|e| e.to_string())?;
    Ok(())
}

/// Turn a free-form project name into a valid cargo crate name.
fn sanitize_crate_name(name: &str) -> String {
    let mut out: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() || out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert_str(0, "game_");
    }
    out
}

fn cargo_toml_template(crate_name: &str) -> String {
    format!(
        "[package]\n\
         name = \"{crate_name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\
         \n\
         # Change this to track the engine version you want (e.g. a local path to a fork).\n\
         [dependencies]\n\
         bevy = \"{TEMPLATE_BEVY_VERSION}\"\n\
         \n\
         # Fast iterative builds: optimize dependencies even in debug.\n\
         [profile.dev.package.\"*\"]\n\
         opt-level = 3\n"
    )
}

fn main_rs_template() -> &'static str {
    "//! A new Bevy game, scaffolded by the Bevy editor.\n\
     use bevy::prelude::*;\n\
     \n\
     fn main() {\n\
     \x20   App::new()\n\
     \x20       .add_plugins(DefaultPlugins)\n\
     \x20       .add_systems(Startup, setup)\n\
     \x20       .add_systems(Update, spin)\n\
     \x20       .run();\n\
     }\n\
     \n\
     /// A marker for the demo cube so `spin` only rotates it.\n\
     #[derive(Component)]\n\
     struct Spinner;\n\
     \n\
     fn setup(\n\
     \x20   mut commands: Commands,\n\
     \x20   mut meshes: ResMut<Assets<Mesh>>,\n\
     \x20   mut materials: ResMut<Assets<StandardMaterial>>,\n\
     ) {\n\
     \x20   commands.spawn((\n\
     \x20       Camera3d::default(),\n\
     \x20       Transform::from_xyz(0.0, 3.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),\n\
     \x20   ));\n\
     \x20   commands.spawn((\n\
     \x20       DirectionalLight::default(),\n\
     \x20       Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),\n\
     \x20   ));\n\
     \x20   commands.spawn((\n\
     \x20       Mesh3d(meshes.add(Cuboid::default())),\n\
     \x20       MeshMaterial3d(materials.add(Color::srgb(0.4, 0.6, 0.9))),\n\
     \x20       Spinner,\n\
     \x20   ));\n\
     }\n\
     \n\
     fn spin(time: Res<Time>, mut query: Query<&mut Transform, With<Spinner>>) {\n\
     \x20   for mut transform in &mut query {\n\
     \x20       transform.rotate_y(time.delta_secs());\n\
     \x20   }\n\
     }\n"
}

// ---------------------------------------------------------------------------
// Action handlers
// ---------------------------------------------------------------------------

fn on_create_project(
    req: On<CreateProjectRequest>,
    mut active: ResMut<ActiveProject>,
    mut recent: ResMut<RecentProjects>,
    mut commands: Commands,
) {
    let name = req.name.trim();
    if name.is_empty() {
        commands.trigger(ShowToast::warning("Project name is empty"));
        return;
    }
    let root = req.parent_dir.join(sanitize_crate_name(name));
    match scaffold_project(&root, name) {
        Ok(()) => {
            info!("Created project '{name}' at {}", root.display());
            commands.trigger(ShowToast::success(format!("Created {}", root.display())));
            activate(&mut active, &mut recent, root);
        }
        Err(err) => {
            error!("Create project failed: {err}");
            commands.trigger(ShowToast::error(format!("Create failed: {err}")));
        }
    }
}

fn on_open_project(
    req: On<OpenProjectRequest>,
    mut active: ResMut<ActiveProject>,
    mut recent: ResMut<RecentProjects>,
    mut commands: Commands,
) {
    let root = req.0.clone();
    if !root.join(PROJECT_FILE).is_file() {
        commands.trigger(ShowToast::error(format!(
            "No {PROJECT_FILE} in {}",
            root.display()
        )));
        return;
    }
    activate(&mut active, &mut recent, root);
    commands.trigger(ShowToast::success(format!("Opened {}", active.config.name)));
}

/// Point [`ActiveProject`] at `root`, load its config, and record it in recents.
fn activate(active: &mut ActiveProject, recent: &mut RecentProjects, root: PathBuf) {
    active.root = root.clone();
    if let Err(err) = active.load_config() {
        error!("Loaded project but config read failed: {err}");
        active.config = ProjectConfig::default();
    }
    recent.touch(&root);
    save_recent_projects(recent);
}

fn on_save_project(
    _: On<SaveProjectRequest>,
    mut active: ResMut<ActiveProject>,
    mut commands: Commands,
) {
    match active.save_config() {
        Ok(()) => commands.trigger(ShowToast::success("Saved project settings")),
        Err(err) => commands.trigger(ShowToast::error(format!("Save failed: {err}"))),
    }
}

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

/// Directory field in the New Project dialog.
#[derive(Component, Default, Clone, Copy)]
struct NewProjectDirInput;
/// Name field in the New Project dialog.
#[derive(Component, Default, Clone, Copy)]
struct NewProjectNameInput;
/// Confirm button in the New Project dialog.
#[derive(Component, Default, Clone, Copy)]
struct NewProjectConfirm;
/// Path field in the Open Project dialog.
#[derive(Component, Default, Clone, Copy)]
struct OpenProjectPathInput;
/// Confirm button in the Open Project dialog.
#[derive(Component, Default, Clone, Copy)]
struct OpenProjectConfirm;
/// A recent-project button; opens the stored path.
#[derive(Component, Default, Clone)]
struct RecentProjectButton(PathBuf);

fn on_open_new_project_dialog(
    _: On<OpenNewProjectDialog>,
    active: Res<ActiveProject>,
    mut commands: Commands,
) {
    let initial_dir = active
        .root
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| ".".to_string());
    commands.spawn_scene(new_project_dialog(initial_dir));
}

fn on_new_project_confirm(
    act: On<Activate>,
    buttons: Query<(), With<NewProjectConfirm>>,
    dirs: Query<&EditableText, With<NewProjectDirInput>>,
    names: Query<&EditableText, With<NewProjectNameInput>>,
    mut commands: Commands,
) {
    if !buttons.contains(act.entity) {
        return;
    }
    let dir = dirs
        .iter()
        .next()
        .map(|e| e.value().to_string())
        .unwrap_or_default();
    let name = names
        .iter()
        .next()
        .map(|e| e.value().to_string())
        .unwrap_or_default();
    let dir = dir.trim();
    let name = name.trim();
    if dir.is_empty() || name.is_empty() {
        return;
    }
    commands.trigger(CreateProjectRequest {
        parent_dir: PathBuf::from(dir),
        name: name.to_string(),
    });
    commands.trigger(CloseOverlay);
}

fn on_open_open_project_dialog(
    _: On<OpenOpenProjectDialog>,
    recent: Res<RecentProjects>,
    mut commands: Commands,
) {
    let recents = recent.0.clone();
    commands.queue(move |world: &mut World| {
        let _ = world.spawn_scene(open_project_dialog());
        let mut list_q = world.query_filtered::<Entity, With<RecentProjectList>>();
        let Some(list) = list_q.iter(world).next() else {
            return;
        };
        let items: Vec<Box<dyn SceneList>> = if recents.is_empty() {
            vec![Box::new(EntityScene(label_dim("No recent projects")))]
        } else {
            recents
                .into_iter()
                .map(|p| Box::new(EntityScene(recent_project_item(p))) as Box<dyn SceneList>)
                .collect()
        };
        world
            .entity_mut(list)
            .queue_spawn_related_scenes::<Children>(items);
    });
}

/// Container for recent-project buttons in the Open Project dialog.
#[derive(Component, Default, Clone, Copy)]
struct RecentProjectList;

fn on_open_project_confirm(
    act: On<Activate>,
    buttons: Query<(), With<OpenProjectConfirm>>,
    inputs: Query<&EditableText, With<OpenProjectPathInput>>,
    mut commands: Commands,
) {
    if !buttons.contains(act.entity) {
        return;
    }
    let Some(path) = inputs.iter().next().map(|e| e.value().to_string()) else {
        return;
    };
    let path = path.trim();
    if path.is_empty() {
        return;
    }
    commands.trigger(OpenProjectRequest(PathBuf::from(path)));
    commands.trigger(CloseOverlay);
}

fn on_recent_project_button(
    act: On<Activate>,
    buttons: Query<&RecentProjectButton>,
    mut commands: Commands,
) {
    if let Ok(button) = buttons.get(act.entity) {
        commands.trigger(OpenProjectRequest(button.0.clone()));
        commands.trigger(CloseOverlay);
    }
}

fn new_project_dialog(initial_dir: String) -> impl Scene {
    let default_name = String::from("my_game");
    dialog_frame(
        "New Project",
        px(440),
        bsn! {
            (
                Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(8) }
                Children [
                    (label_dim("Parent directory")),
                    (@FeathersTextInputContainer Children [
                        (@FeathersTextInput SeedText(initial_dir) NewProjectDirInput AutoFocus)
                    ]),
                    (label_dim("Project name")),
                    (@FeathersTextInputContainer Children [
                        (@FeathersTextInput SeedText(default_name) NewProjectNameInput)
                    ]),
                    (
                        Node { display: Display::Flex, flex_direction: FlexDirection::Row, justify_content: JustifyContent::End, column_gap: px(8) }
                        Children [
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Cancel") ThemedText } }
                                on(|_: On<Activate>, mut c: Commands| { c.trigger(CloseOverlay); })),
                            (@FeathersButton { @variant: ButtonVariant::Primary, @caption: bsn! { Text("Create") ThemedText } }
                                NewProjectConfirm),
                        ]
                    ),
                ]
            )
        },
    )
}

fn open_project_dialog() -> impl Scene {
    dialog_frame(
        "Open Project",
        px(440),
        bsn! {
            (
                Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(8) }
                Children [
                    (label_dim("Project directory (containing project.bevy.ron)")),
                    (@FeathersTextInputContainer Children [
                        (@FeathersTextInput SeedText(String::new()) OpenProjectPathInput AutoFocus)
                    ]),
                    (
                        Node { display: Display::Flex, flex_direction: FlexDirection::Row, justify_content: JustifyContent::End, column_gap: px(8) }
                        Children [
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Cancel") ThemedText } }
                                on(|_: On<Activate>, mut c: Commands| { c.trigger(CloseOverlay); })),
                            (@FeathersButton { @variant: ButtonVariant::Primary, @caption: bsn! { Text("Open") ThemedText } }
                                OpenProjectConfirm),
                        ]
                    ),
                    (label_dim("Recent")),
                    (Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(2) } RecentProjectList),
                ]
            )
        },
    )
}

fn recent_project_item(path: PathBuf) -> impl Scene {
    let display = path.display().to_string();
    bsn! {
        (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { (Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(8) } Children [ (icon(icons::FOLDER) ThemedText), (Text(display) ThemedText) ]) } }
            RecentProjectButton(path))
    }
}

// ---------------------------------------------------------------------------
// Project settings dialog
// ---------------------------------------------------------------------------

/// Open the Project Settings dialog.
#[derive(Event, Clone, Copy)]
pub struct OpenProjectSettings;

/// Open the Input Map editor dialog.
#[derive(Event, Clone, Copy)]
pub struct OpenInputMap;

#[derive(Component, Default, Clone, Copy)]
struct SettingsNameInput;
#[derive(Component, Default, Clone, Copy)]
struct SettingsSceneInput;
#[derive(Component, Default, Clone, Copy)]
struct SettingsTargetInput;
#[derive(Component, Clone, Copy)]
struct SettingsProfileButton(BuildProfile);
impl Default for SettingsProfileButton {
    fn default() -> Self {
        Self(BuildProfile::Debug)
    }
}
#[derive(Component, Default, Clone, Copy)]
struct SettingsSaveButton;

fn on_open_project_settings(
    _: On<OpenProjectSettings>,
    active: Res<ActiveProject>,
    mut commands: Commands,
) {
    let name = active.config.name.clone();
    let scene = active.config.default_scene.clone().unwrap_or_default();
    let target = active.config.build.target.clone().unwrap_or_default();
    commands.spawn_scene(settings_dialog(name, scene, target));
}

fn settings_dialog(name: String, scene: String, target: String) -> impl Scene {
    dialog_frame(
        "Project Settings",
        px(460),
        bsn! {
            (
                Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(8) }
                Children [
                    (label_dim("Project name")),
                    (@FeathersTextInputContainer Children [
                        (@FeathersTextInput SeedText(name) SettingsNameInput AutoFocus)
                    ]),
                    (label_dim("Default scene (file in assets/scenes)")),
                    (@FeathersTextInputContainer Children [
                        (@FeathersTextInput SeedText(scene) SettingsSceneInput)
                    ]),
                    (label_dim("Build target triple (blank = host)")),
                    (@FeathersTextInputContainer Children [
                        (@FeathersTextInput SeedText(target) SettingsTargetInput)
                    ]),
                    (label_dim("Build profile")),
                    (
                        Node { flex_direction: FlexDirection::Row, column_gap: px(8) }
                        Children [
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Debug") ThemedText } }
                                template_value(SettingsProfileButton(BuildProfile::Debug))),
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Release") ThemedText } }
                                template_value(SettingsProfileButton(BuildProfile::Release))),
                        ]
                    ),
                    (
                        Node { display: Display::Flex, flex_direction: FlexDirection::Row, justify_content: JustifyContent::End, column_gap: px(8) }
                        Children [
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Input Map") ThemedText } }
                                on(|_: On<Activate>, mut c: Commands| { c.trigger(CloseOverlay); c.trigger(OpenInputMap); })),
                            (@FeathersButton { @variant: ButtonVariant::Primary, @caption: bsn! { Text("Save") ThemedText } }
                                SettingsSaveButton),
                        ]
                    ),
                ]
            )
        },
    )
}

/// Light up whichever build-profile button matches the active project's profile.
fn sync_profile_buttons(
    active: Res<ActiveProject>,
    mut buttons: Query<(&SettingsProfileButton, &mut ButtonVariant)>,
) {
    for (button, mut variant) in buttons.iter_mut() {
        let want = if button.0 == active.config.build.profile {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        };
        if *variant != want {
            *variant = want;
        }
    }
}

fn on_settings_profile_button(
    act: On<Activate>,
    buttons: Query<&SettingsProfileButton>,
    mut active: ResMut<ActiveProject>,
) {
    if let Ok(button) = buttons.get(act.entity) {
        active.config.build.profile = button.0;
        active.config_dirty = true;
    }
}

fn on_settings_save(
    act: On<Activate>,
    buttons: Query<(), With<SettingsSaveButton>>,
    names: Query<&EditableText, With<SettingsNameInput>>,
    scenes: Query<&EditableText, With<SettingsSceneInput>>,
    targets: Query<&EditableText, With<SettingsTargetInput>>,
    mut active: ResMut<ActiveProject>,
    mut commands: Commands,
) {
    if !buttons.contains(act.entity) {
        return;
    }
    if let Some(name) = names.iter().next().map(|e| e.value().to_string()) {
        let name = name.trim();
        if !name.is_empty() {
            active.config.name = name.to_string();
        }
    }
    if let Some(scene) = scenes.iter().next().map(|e| e.value().to_string()) {
        let scene = scene.trim();
        active.config.default_scene = (!scene.is_empty()).then(|| scene.to_string());
    }
    if let Some(target) = targets.iter().next().map(|e| e.value().to_string()) {
        let target = target.trim();
        active.config.build.target = (!target.is_empty()).then(|| target.to_string());
    }
    match active.save_config() {
        Ok(()) => commands.trigger(ShowToast::success("Saved project settings")),
        Err(err) => commands.trigger(ShowToast::error(format!("Save failed: {err}"))),
    }
    commands.trigger(CloseOverlay);
}

// ---------------------------------------------------------------------------
// Input map editor dialog
// ---------------------------------------------------------------------------

#[derive(Component, Default, Clone, Copy)]
struct InputActionNameInput;
#[derive(Component, Default, Clone, Copy)]
struct InputActionKeyInput;
#[derive(Component, Default, Clone, Copy)]
struct AddInputActionButton;
#[derive(Component, Default, Clone)]
struct RemoveInputActionButton(usize);
/// Container the existing-action rows are spawned into.
#[derive(Component, Default, Clone, Copy)]
struct InputActionList;

fn on_open_input_map(_: On<OpenInputMap>, active: Res<ActiveProject>, mut commands: Commands) {
    let actions = active.config.input_actions.clone();
    commands.queue(move |world: &mut World| {
        let _ = world.spawn_scene(input_map_dialog());
        let mut list_q = world.query_filtered::<Entity, With<InputActionList>>();
        let Some(list) = list_q.iter(world).next() else {
            return;
        };
        let items: Vec<Box<dyn SceneList>> = if actions.is_empty() {
            vec![Box::new(EntityScene(label_dim("No actions yet")))]
        } else {
            actions
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    Box::new(EntityScene(input_action_row(i, &a.name, &a.key)))
                        as Box<dyn SceneList>
                })
                .collect()
        };
        world
            .entity_mut(list)
            .queue_spawn_related_scenes::<Children>(items);
    });
}

fn input_map_dialog() -> impl Scene {
    dialog_frame(
        "Input Map",
        px(460),
        bsn! {
            (
                Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(8) }
                Children [
                    (Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(2) } InputActionList),
                    (label_dim("Add action (name + key, e.g. jump / Space)")),
                    (
                        Node { flex_direction: FlexDirection::Row, column_gap: px(8) }
                        Children [
                            (@FeathersTextInputContainer Node { flex_grow: 1.0 } Children [
                                (@FeathersTextInput SeedText(String::new()) InputActionNameInput)
                            ]),
                            (@FeathersTextInputContainer Node { flex_grow: 1.0 } Children [
                                (@FeathersTextInput SeedText(String::new()) InputActionKeyInput)
                            ]),
                            (@FeathersButton { @variant: ButtonVariant::Primary, @caption: bsn! { Text("Add") ThemedText } }
                                AddInputActionButton),
                        ]
                    ),
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

fn input_action_row(index: usize, name: &str, key: &str) -> impl Scene {
    let text = format!("{name}  →  {key}");
    bsn! {
        (
            Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(8), padding: UiRect::axes(px(4), px(2)) }
            Children [
                (Node { flex_grow: 1.0 } Children [ (label_dim(text)) ]),
                (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { (icon(icons::X) ThemedText) } }
                    template_value(RemoveInputActionButton(index))),
            ]
        )
    }
}

fn on_add_input_action(
    act: On<Activate>,
    buttons: Query<(), With<AddInputActionButton>>,
    names: Query<&EditableText, With<InputActionNameInput>>,
    keys: Query<&EditableText, With<InputActionKeyInput>>,
    mut active: ResMut<ActiveProject>,
    mut commands: Commands,
) {
    if !buttons.contains(act.entity) {
        return;
    }
    let name = names
        .iter()
        .next()
        .map(|e| e.value().to_string())
        .unwrap_or_default();
    let key = keys
        .iter()
        .next()
        .map(|e| e.value().to_string())
        .unwrap_or_default();
    let name = name.trim();
    let key = key.trim();
    if name.is_empty() || key.is_empty() {
        return;
    }
    active.config.input_actions.push(InputAction {
        name: name.to_string(),
        key: key.to_string(),
    });
    let _ = active.save_config();
    commands.trigger(CloseOverlay);
    commands.trigger(OpenInputMap);
}

fn on_remove_input_action(
    act: On<Activate>,
    buttons: Query<&RemoveInputActionButton>,
    mut active: ResMut<ActiveProject>,
    mut commands: Commands,
) {
    if let Ok(button) = buttons.get(act.entity) {
        if button.0 < active.config.input_actions.len() {
            active.config.input_actions.remove(button.0);
            let _ = active.save_config();
        }
        commands.trigger(CloseOverlay);
        commands.trigger(OpenInputMap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_ron_roundtrip() {
        let mut config = ProjectConfig {
            name: "My Game".to_string(),
            default_scene: Some("level1.scn.ron".to_string()),
            ..Default::default()
        };
        config.build.profile = BuildProfile::Release;
        config.input_actions.push(InputAction {
            name: "jump".to_string(),
            key: "Space".to_string(),
        });
        config.touch_recent_scene("level1.scn.ron");

        let ron = config.to_ron().expect("serialize");
        let back = ProjectConfig::from_ron(&ron).expect("deserialize");
        assert_eq!(back.name, "My Game");
        assert_eq!(back.default_scene.as_deref(), Some("level1.scn.ron"));
        assert_eq!(back.build.profile, BuildProfile::Release);
        assert_eq!(back.input_actions.len(), 1);
        assert_eq!(back.recent_scenes, vec!["level1.scn.ron".to_string()]);
    }

    #[test]
    fn touch_recent_scene_dedups_and_orders() {
        let mut config = ProjectConfig::default();
        config.touch_recent_scene("a");
        config.touch_recent_scene("b");
        config.touch_recent_scene("a");
        assert_eq!(config.recent_scenes, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn sanitize_crate_name_is_valid() {
        assert_eq!(sanitize_crate_name("My Cool Game!"), "my_cool_game_");
        assert_eq!(sanitize_crate_name("123game"), "game_123game");
        assert_eq!(sanitize_crate_name(""), "game_");
        assert_eq!(sanitize_crate_name("already_ok"), "already_ok");
    }

    #[test]
    fn build_profile_cargo_flag() {
        assert_eq!(BuildProfile::Debug.cargo_flag(), None);
        assert_eq!(BuildProfile::Release.cargo_flag(), Some("--release"));
    }

    #[test]
    fn scaffold_writes_a_runnable_project() {
        let base = std::env::temp_dir().join("bevy_editor_scaffold_test");
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("my_game");
        scaffold_project(&root, "My Game").expect("scaffold");

        assert!(root.join("Cargo.toml").is_file());
        assert!(root.join("src/main.rs").is_file());
        assert!(root.join("assets/scenes").is_dir());
        assert!(root.join(PROJECT_FILE).is_file());

        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains("name = \"my_game\""),
            "crate name sanitized"
        );
        assert!(manifest.contains("bevy ="), "depends on bevy");

        let main_rs = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
        assert!(main_rs.contains("DefaultPlugins"));

        let config =
            ProjectConfig::from_ron(&std::fs::read_to_string(root.join(PROJECT_FILE)).unwrap())
                .unwrap();
        assert_eq!(config.name, "My Game");

        // Scaffolding twice in the same dir is rejected.
        assert!(scaffold_project(&root, "My Game").is_err());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn recent_projects_touch_orders_and_caps() {
        let mut recent = RecentProjects::default();
        for i in 0..15 {
            recent.touch(Path::new(&format!("/p/{i}")));
        }
        assert_eq!(recent.0.len(), 12, "capped at 12");
        assert_eq!(recent.0[0], PathBuf::from("/p/14"), "most recent first");
        recent.touch(Path::new("/p/5"));
        assert_eq!(
            recent.0[0],
            PathBuf::from("/p/5"),
            "re-touch moves to front"
        );
    }
}
