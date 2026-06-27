//! A VSCode-style command palette (`Cmd/Ctrl+P`): a fuzzy-filtered list of every editor
//! action, runnable by name. Each command maps to the same event/resource poke the menus and
//! toolbar use, so the palette stays in sync automatically.

use bevy_app::{App, Plugin, Startup, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::controls::{
    ButtonVariant, FeathersButton, FeathersTextInput, FeathersTextInputContainer,
};
use bevy_feathers::display::icon;
use bevy_feathers::theme::ThemedText;
use bevy_input_focus::AutoFocus;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_state::state::NextState;
use bevy_text::EditableText;
use bevy_ui::widget::Text;
use bevy_ui::{px, AlignItems, Display, FlexDirection, Node, UiRect};
use bevy_ui_widgets::Activate;

use crate::actions::{
    DuplicateRequest, OpenImportDialog, OpenOpenDialog, OpenSaveDialog, SceneIoRequest, SpawnKind,
    SpawnRequest,
};
use crate::build_export::{BuildProjectRequest, ExportSceneRequest};
use crate::remote::OpenConnectDialog;
use crate::state::{EditorState, GizmoMode, GizmoSnap, ViewportMode};
use crate::ui::style::dialog_frame;
use crate::ui::{icons, CloseOverlay, OpenCommandPalette, SeedText, ToggleConsole, ToggleTheme};
use crate::undo::{RequestRedo, RequestUndo};
use crate::viewport::FrameSelectionRequest;

/// What a palette entry does when chosen. Each variant maps to an existing editor action.
#[derive(Clone, Copy)]
enum PaletteAction {
    Spawn(SpawnKind),
    Gizmo(GizmoMode),
    Play,
    Pause,
    Stop,
    Toggle2D3D,
    ToggleSnap,
    FrameSelection,
    Save,
    SaveAs,
    Open,
    New,
    Import,
    Connect,
    Undo,
    Redo,
    Duplicate,
    Build,
    Export,
    ToggleTheme,
    ToggleConsole,
}

/// One palette entry: a label, an icon, and the action to run.
#[derive(Clone)]
struct EditorCommand {
    label: &'static str,
    icon: &'static str,
    action: PaletteAction,
}

/// All editor commands, populated at startup.
#[derive(Resource, Default)]
struct CommandRegistry(Vec<EditorCommand>);

/// The palette's search input.
#[derive(Component, Default, Clone, Copy)]
struct PaletteSearch;

/// The container the filtered result rows are spawned into.
#[derive(Component, Default, Clone, Copy)]
struct PaletteResults;

/// A result row; carries the action to run on click.
#[derive(Component, Clone, Copy)]
struct PaletteItem(PaletteAction);

impl Default for PaletteItem {
    fn default() -> Self {
        Self(PaletteAction::New)
    }
}

fn build_registry(mut commands: Commands) {
    use PaletteAction::*;
    let cmds = vec![
        ("Spawn Cube", icons::CUBE, Spawn(SpawnKind::Cube)),
        ("Spawn Sphere", icons::SPHERE, Spawn(SpawnKind::Sphere)),
        ("Spawn Plane", icons::SQUARE, Spawn(SpawnKind::Plane)),
        (
            "Spawn Point Light",
            icons::LIGHT,
            Spawn(SpawnKind::PointLight),
        ),
        (
            "Spawn Directional Light",
            icons::DIR_LIGHT,
            Spawn(SpawnKind::DirectionalLight),
        ),
        ("Spawn Sprite (2D)", icons::SPRITE, Spawn(SpawnKind::Sprite)),
        ("Spawn Empty", icons::EMPTY, Spawn(SpawnKind::Empty)),
        ("Move Tool", icons::GIZMO_MOVE, Gizmo(GizmoMode::Translate)),
        ("Rotate Tool", icons::GIZMO_ROTATE, Gizmo(GizmoMode::Rotate)),
        ("Scale Tool", icons::GIZMO_SCALE, Gizmo(GizmoMode::Scale)),
        ("Play", icons::PLAY, Play),
        ("Pause", icons::PAUSE, Pause),
        ("Stop", icons::STOP, Stop),
        ("Toggle 2D / 3D", icons::CUBE, Toggle2D3D),
        ("Toggle Snap", icons::SNAP, ToggleSnap),
        ("Frame Selection", icons::FRAME, FrameSelection),
        ("Duplicate Selection", icons::DUPLICATE, Duplicate),
        ("New Scene", icons::FILE_PLUS, New),
        ("Open Scene...", icons::FOLDER_OPEN, Open),
        ("Save Scene", icons::SAVE, Save),
        ("Save Scene As...", icons::SAVE, SaveAs),
        ("Import Asset...", icons::IMPORT, Import),
        ("Undo", icons::UNDO, Undo),
        ("Redo", icons::REDO, Redo),
        ("Build Project", icons::BUILD, Build),
        ("Export Scene", icons::EXPORT, Export),
        ("Toggle Light / Dark Theme", icons::SUN, ToggleTheme),
        ("Toggle Console", icons::TERMINAL, ToggleConsole),
        ("Connect to Remote", icons::REMOTE, Connect),
    ];
    commands.insert_resource(CommandRegistry(
        cmds.into_iter()
            .map(|(label, icon, action)| EditorCommand {
                label,
                icon,
                action,
            })
            .collect(),
    ));
}

fn on_open_palette(_: On<OpenCommandPalette>, mut commands: Commands) {
    commands.spawn_scene(palette_overlay());
}

fn palette_overlay() -> impl Scene {
    dialog_frame(
        "Command Palette",
        px(480),
        bsn! {
            (
                Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(8) }
                Children [
                    (@FeathersTextInputContainer Children [
                        (@FeathersTextInput SeedText(String::new()) PaletteSearch AutoFocus)
                    ]),
                    (Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(2) } PaletteResults),
                ]
            )
        },
    )
}

/// Rebuild the result list whenever the search text changes (and once when it first appears).
fn update_palette_results(
    search: Query<&EditableText, (With<PaletteSearch>, Changed<EditableText>)>,
    results: Query<Entity, With<PaletteResults>>,
    registry: Res<CommandRegistry>,
    mut commands: Commands,
) {
    let Ok(text) = search.single() else {
        return;
    };
    let Ok(container) = results.single() else {
        return;
    };
    let query = text.value().to_string().to_lowercase();
    let rows: Vec<Box<dyn SceneList>> = registry
        .0
        .iter()
        .filter(|c| query.is_empty() || c.label.to_lowercase().contains(&query))
        .take(12)
        .map(|c| Box::new(EntityScene(palette_row(c))) as Box<dyn SceneList>)
        .collect();
    commands.entity(container).despawn_children();
    commands
        .entity(container)
        .queue_spawn_related_scenes::<Children>(rows);
}

fn palette_row(cmd: &EditorCommand) -> impl Scene {
    let label_text = cmd.label.to_string();
    let icon_path = cmd.icon;
    let action = cmd.action;
    bsn! {
        (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { (palette_caption(icon_path, label_text)) } }
            template_value(PaletteItem(action)))
    }
}

fn palette_caption(icon_path: &'static str, text: String) -> impl Scene {
    bsn! {
        (
            Node { display: Display::Flex, flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(10), padding: UiRect::axes(px(4), px(2)) }
            Children [
                (icon(icon_path) ThemedText),
                (Text(text) ThemedText),
            ]
        )
    }
}

fn on_palette_activate(
    act: On<Activate>,
    items: Query<&PaletteItem>,
    mut gizmo: ResMut<GizmoMode>,
    mut vmode: ResMut<ViewportMode>,
    mut snap: ResMut<GizmoSnap>,
    mut state: ResMut<NextState<EditorState>>,
    mut commands: Commands,
) {
    let Ok(item) = items.get(act.entity) else {
        return;
    };
    match item.0 {
        PaletteAction::Spawn(kind) => commands.trigger(SpawnRequest(kind)),
        PaletteAction::Gizmo(mode) => *gizmo = mode,
        PaletteAction::Play => state.set(EditorState::Playing),
        PaletteAction::Pause => state.set(EditorState::Paused),
        PaletteAction::Stop => state.set(EditorState::Editing),
        PaletteAction::Toggle2D3D => vmode.toggle(),
        PaletteAction::ToggleSnap => snap.enabled = !snap.enabled,
        PaletteAction::FrameSelection => commands.trigger(FrameSelectionRequest),
        PaletteAction::Save => commands.trigger(SceneIoRequest::Save),
        PaletteAction::SaveAs => commands.trigger(OpenSaveDialog),
        PaletteAction::Open => commands.trigger(OpenOpenDialog),
        PaletteAction::New => commands.trigger(SceneIoRequest::New),
        PaletteAction::Import => commands.trigger(OpenImportDialog),
        PaletteAction::Connect => commands.trigger(OpenConnectDialog),
        PaletteAction::Undo => commands.trigger(RequestUndo),
        PaletteAction::Redo => commands.trigger(RequestRedo),
        PaletteAction::Duplicate => commands.trigger(DuplicateRequest),
        PaletteAction::Build => commands.trigger(BuildProjectRequest),
        PaletteAction::Export => commands.trigger(ExportSceneRequest),
        PaletteAction::ToggleTheme => commands.trigger(ToggleTheme),
        PaletteAction::ToggleConsole => commands.trigger(ToggleConsole),
    }
    commands.trigger(CloseOverlay);
}

/// Installs the command palette.
pub struct CommandPalettePlugin;

impl Plugin for CommandPalettePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CommandRegistry>()
            .add_systems(Startup, build_registry)
            .add_systems(Update, update_palette_results)
            .add_observer(on_open_palette)
            .add_observer(on_palette_activate);
    }
}
