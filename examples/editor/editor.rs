//! A Unity/Godot-style GUI editor for Bevy, built on the Feathers widget toolkit.
//!
//! Run with:
//! ```sh
//! cargo run --example editor --features bevy_editor
//! ```
//!
//! The editor opens a paneled window — a menu bar, toolbar, and scene-tab strip across the
//! top, an entity **Hierarchy** on the left, the rendered **Viewport** in the center, a
//! reflection-driven **Inspector** on the right, and an **Asset** browser along the bottom.
//! Use the *Entity* menu to spawn primitives, click them in the viewport or hierarchy to
//! select, edit their components in the inspector, drag them to move (or use the Move /
//! Rotate / Scale gizmo), then *Save* the scene and *Play* to run it. Cmd/Ctrl+Z undoes.

use bevy::{
    camera::RenderTarget,
    editor::{
        editor_console_layer, spawn_kind, BehaviorScript, EditorPlugins, EditorSelection, SpawnKind,
    },
    image::Image,
    log::LogPlugin,
    picking::mesh_picking::MeshPickingPlugin,
    prelude::*,
    render::{
        render_resource::TextureFormat,
        view::screenshot::{save_to_disk, Screenshot},
    },
    winit::{UpdateMode, WinitSettings},
};

fn main() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Bevy Editor".into(),
                    resolution: (1440u32, 900u32).into(),
                    ..default()
                }),
                ..default()
            })
            // Capture log records into the in-editor console panel.
            .set(LogPlugin {
                custom_layer: editor_console_layer,
                ..default()
            }),
    )
    // Keep the editor updating even when the window isn't focused, so layout and
    // viewport rendering stay live.
    .insert_resource(WinitSettings {
        focused_mode: UpdateMode::Continuous,
        unfocused_mode: UpdateMode::Continuous,
    })
    // A calm neutral background for the 3D viewport.
    .insert_resource(ClearColor(Color::srgb(0.09, 0.10, 0.13)))
    // Mesh picking lets us select 3D scene entities by clicking them in the viewport
    // (the viewport forwards pointer input into the offscreen render target). Sprite
    // and UI picking backends are already provided by `DefaultPlugins`.
    .add_plugins(MeshPickingPlugin)
    .add_plugins(EditorPlugins)
    // Start with a little demo content so the editor isn't empty on launch.
    .add_systems(Startup, spawn_demo_scene);

    // Headless capture mode for visual verification: when `EDITOR_SCREENSHOT` is set, the
    // app renders a fixed number of frames, saves a PNG of the primary window, and exits.
    if let Ok(path) = std::env::var("EDITOR_SCREENSHOT") {
        app.insert_resource(ScreenshotReq {
            path,
            frame: 0,
            target: None,
        })
        .add_systems(Update, take_screenshot);
    }

    app.run();
}

/// Drives the `EDITOR_SCREENSHOT` capture-and-exit flow. The editor's UI camera is
/// redirected to an offscreen image so capture works headlessly (a background window has no
/// live drawable on macOS and would read back black).
#[derive(Resource)]
struct ScreenshotReq {
    path: String,
    frame: u32,
    target: Option<Handle<Image>>,
}

fn take_screenshot(
    mut req: ResMut<ScreenshotReq>,
    mut images: ResMut<Assets<Image>>,
    mut targets: Query<&mut RenderTarget, With<IsDefaultUiCamera>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    req.frame += 1;
    // Redirect the editor's UI camera to an offscreen image once it exists.
    if req.target.is_none() {
        if let Ok(mut target) = targets.single_mut() {
            let image = Image::new_target_texture(1440, 900, TextureFormat::Bgra8UnormSrgb, None);
            let handle = images.add(image);
            *target = RenderTarget::Image(handle.clone().into());
            req.target = Some(handle);
        }
        return;
    }
    // Optionally open a dialog/overlay before capturing, to screenshot it.
    if req.frame == 60 && let Ok(open) = std::env::var("EDITOR_SHOT_OPEN") {
        match open.as_str() {
            "save" => commands.trigger(bevy::editor::OpenSaveDialog),
            "import" => commands.trigger(bevy::editor::OpenImportDialog),
            "newproject" => commands.trigger(bevy::editor::project::OpenNewProjectDialog),
            "openproject" => commands.trigger(bevy::editor::project::OpenOpenProjectDialog),
            "code" => {
                commands.queue(|world: &mut World| {
                    if let Some(mut view) = world.get_resource_mut::<bevy::editor::MainView>() {
                        *view = bevy::editor::MainView::Code;
                    }
                });
            }
            // Open a real source file to show syntax highlighting (cwd = workspace root).
            // `EDITOR_SHOT_FILE` overrides the path for a custom capture.
            "codehl" => {
                let path = std::env::var("EDITOR_SHOT_FILE")
                    .unwrap_or_else(|_| "crates/bevy_editor/src/code_highlight.rs".to_string());
                commands.trigger(bevy::editor::code::OpenCodeFileRequest {
                    path: std::path::PathBuf::from(path),
                    line: None,
                });
            }
            "stats" => commands.trigger(bevy::editor::ui::ShowBottomTab(
                bevy::editor::ui::BottomTab::Stats,
            )),
            "material" => commands.trigger(bevy::editor::ui::ShowBottomTab(
                bevy::editor::ui::BottomTab::Material,
            )),
            "animation" => commands.trigger(bevy::editor::ui::ShowBottomTab(
                bevy::editor::ui::BottomTab::Animation,
            )),
            "settings" => commands.trigger(bevy::editor::project::OpenProjectSettings),
            "uinode" => commands.trigger(bevy::editor::SpawnRequest(SpawnKind::UiNode)),
            "audio" => commands.trigger(bevy::editor::ui::ShowBottomTab(
                bevy::editor::ui::BottomTab::Audio,
            )),
            "themeeditor" => commands.trigger(bevy::editor::ui::ShowBottomTab(
                bevy::editor::ui::BottomTab::Theme,
            )),
            "localization" => commands.trigger(bevy::editor::ui::ShowBottomTab(
                bevy::editor::ui::BottomTab::Localization,
            )),
            "physics" => commands.trigger(bevy::editor::gameplay::SpawnPhysicsCube),
            "palette" => commands.trigger(bevy::editor::ui::OpenCommandPalette),
            "console" => commands.trigger(bevy::editor::ui::ToggleConsole),
            "theme" => commands.trigger(bevy::editor::ui::ToggleTheme),
            "toast" => {
                commands.trigger(bevy::editor::ui::ShowToast::success("Scene saved"));
                commands.trigger(bevy::editor::ui::ShowToast::error(
                    "Build failed — see console",
                ));
            }
            _ => {}
        }
    }
    // Let icons (embedded assets) and the viewport settle before capturing.
    if req.frame == 90 && let Some(handle) = req.target.clone() {
        let path = req.path.clone();
        commands
            .spawn(Screenshot(RenderTarget::Image(handle.into())))
            .observe(save_to_disk(path));
    }
    // Give the async save a few frames to flush, then quit.
    if req.frame >= 110 {
        exit.write(AppExit::Success);
    }
}

/// Populate the scene with a couple of primitives and select one, so the hierarchy,
/// inspector, and viewport all have something to show immediately.
fn spawn_demo_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut selection: ResMut<EditorSelection>,
) {
    let cube = spawn_kind(
        &mut commands,
        &mut meshes,
        &mut materials,
        SpawnKind::Cube,
        Transform::from_xyz(-1.2, 0.5, 0.0),
        "Cube",
    );
    // Attach a behavior script so the cube spins in play mode — edit it live in the
    // inspector, or add scripts to other entities via the inspector's "Add Component".
    commands.entity(cube).insert(BehaviorScript {
        source: "spin 1.0".into(),
    });
    spawn_kind(
        &mut commands,
        &mut meshes,
        &mut materials,
        SpawnKind::Sphere,
        Transform::from_xyz(1.2, 0.5, 0.0),
        "Sphere",
    );
    selection.set_single(cube);
}
