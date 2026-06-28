//! The editor shell layout, authored with the `bsn!` scene macro: a window-filling
//! column of a menu bar, a toolbar, the Hierarchy / Viewport / Inspector body row, an
//! asset row, and a status bar. Menu and toolbar interactions emit editor action events
//! (see [`crate::actions`]) or poke editor-state resources directly.

use bevy_app::AppExit;
use bevy_camera::Camera2d;
use bevy_ecs::message::MessageWriter;
use bevy_ecs::prelude::*;
use bevy_feathers::{
    containers::pane_header_divider,
    controls::*,
    display::{icon, label, label_dim},
    theme::{ThemeBackgroundColor, ThemeBorderColor, ThemedText},
    tokens,
};
use bevy_picking::Pickable;
use bevy_scene::prelude::*;
use bevy_state::state::{NextState, State};
use bevy_ui::widget::Text;
use bevy_ui::{
    percent, px, AlignItems, AlignSelf, Display, FlexDirection, IsDefaultUiCamera, Node, Overflow,
    UiRect, Val,
};
use bevy_ui_widgets::{Activate, ScrollArea};
use bevy_window::SystemCursorIcon;

use crate::actions::{
    DeleteSelectedRequest, DuplicateRequest, OpenImportDialog, OpenOpenDialog, OpenSaveDialog,
    SceneIoRequest, SpawnKind, SpawnRequest,
};
use crate::build_export::{BuildProjectRequest, ExportSceneRequest};
use crate::code::{code_panel, CargoCheckRequest, MainView, MainViewNode, RunGameRequest};
use crate::hierarchy::HierarchySearch;
use crate::markers::EditorEntity;
use crate::project::{
    OpenInputMap, OpenNewProjectDialog, OpenOpenProjectDialog, OpenProjectSettings,
    SaveProjectRequest,
};
use crate::remote::OpenConnectDialog;
use crate::state::{EditorState, GizmoMode, GizmoSnap, ViewportMode};
use crate::ui::icons;
use crate::ui::style::{etokens, sizes, space};
use crate::ui::{BottomTab, ShowBottomTab};
use crate::undo::{RequestRedo, RequestUndo};

use super::splitter::{on_splitter_drag, ResizeSide, SplitAxis, Splitter};
use super::{
    AssetContent, EditorUiCamera, HierarchyContent, InspectorContent, SeedText, TabBarContent,
    ViewportSlot,
};

/// Marks a toolbar button that should light up (Primary) when its mode/state is active.
#[derive(Component, Clone, Copy, Default)]
pub enum ToolbarToggle {
    /// Active when the editor is in this run state (Play / Pause / Stop).
    RunState(EditorState),
    /// Active when this gizmo mode is selected (Move / Rotate / Scale).
    Gizmo(GizmoMode),
    /// Active when the viewport is in 2D mode.
    TwoD,
    /// Active when grid snapping is enabled (default; an inert placeholder for `Default`).
    #[default]
    Snap,
    /// Active when the center area shows the code editor.
    CodeView,
}

/// Light up toolbar buttons whose mode/state is currently active. Runs only when one of the
/// reflected resources changes, so it's effectively free at idle.
pub fn sync_toolbar_active(
    gizmo: Res<GizmoMode>,
    run_state: Res<State<EditorState>>,
    vmode: Res<ViewportMode>,
    snap: Res<GizmoSnap>,
    main_view: Res<MainView>,
    mut buttons: Query<(&ToolbarToggle, &mut ButtonVariant)>,
) {
    if !(gizmo.is_changed()
        || run_state.is_changed()
        || vmode.is_changed()
        || snap.is_changed()
        || main_view.is_changed())
    {
        return;
    }
    for (toggle, mut variant) in buttons.iter_mut() {
        let active = match toggle {
            ToolbarToggle::RunState(s) => run_state.get() == s,
            ToolbarToggle::Gizmo(m) => *gizmo == *m,
            ToolbarToggle::TwoD => *vmode == ViewportMode::TwoD,
            ToolbarToggle::Snap => snap.enabled,
            ToolbarToggle::CodeView => *main_view == MainView::Code,
        };
        let want = if active {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        };
        if *variant != want {
            *variant = want;
        }
    }
}

/// The full editor shell: the UI camera plus the root layout. Spawned once at startup.
pub fn editor_shell() -> impl SceneList {
    bsn_list![
        (Camera2d EditorEntity EditorUiCamera IsDefaultUiCamera),
        editor_root(),
    ]
}

fn editor_root() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: percent(100),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        }
        EditorEntity
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [
            menu_bar(),
            toolbar(),
            tab_bar(),
            main_row(),
            crate::ui::bottom_dock::bottom_dock_panel(),
            crate::ui::status_bar::status_bar(),
        ]
    }
}

// ---------------------------------------------------------------------------
// Menu bar
// ---------------------------------------------------------------------------

fn menu_bar() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: space::XS,
            padding: UiRect::axes(px(4), px(2)),
            min_height: sizes::MENUBAR_H,
            border: UiRect::bottom(px(1)),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        ThemeBorderColor(etokens::PANEL_BORDER)
        Children [
            project_menu(),
            file_menu(),
            edit_menu(),
            entity_menu(),
            gameobject_menu(),
            view_menu(),
            build_menu(),
        ]
    }
}

fn project_menu() -> impl Scene {
    bsn! {
        @FeathersMenu
        Children [
            (@FeathersMenuButton {
                @caption: bsn! { Text("Project") ThemedText },
                @arrow: false,
            }),
            (@FeathersMenuPopup Children [
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::FILE_PLUS, "New Project")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(OpenNewProjectDialog); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::FOLDER_OPEN, "Open Project")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(OpenOpenProjectDialog); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SAVE, "Save Project Settings")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SaveProjectRequest); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SETTINGS, "Project Settings...")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(OpenProjectSettings); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SLIDERS, "Input Map...")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(OpenInputMap); })),
            ]),
        ]
    }
}

/// A menu item with a leading icon and a caption. Keeps menu rows visually scannable.
fn menu_item(icon_path: &'static str, text: &'static str) -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
        }
        Children [
            (icon(icon_path) ThemedText),
            (Text(text) ThemedText),
        ]
    }
}

/// A menu item that also shows its keyboard accelerator right-aligned and dimmed
/// (e.g. `Save … ⌘S`). Use for actions that have a global shortcut in [`crate::ui::shortcuts`].
fn menu_item_accel(icon_path: &'static str, text: &'static str, accel: &'static str) -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
            min_width: px(168),
        }
        Children [
            (icon(icon_path) ThemedText),
            (Text(text) ThemedText),
            (Node { flex_grow: 1.0, min_width: px(20) }),
            (label_dim(accel)),
        ]
    }
}

fn file_menu() -> impl Scene {
    bsn! {
        @FeathersMenu
        Children [
            (@FeathersMenuButton {
                @caption: bsn! { Text("File") ThemedText },
                @arrow: false,
            }),
            (@FeathersMenuPopup Children [
                (@FeathersMenuItem { @caption: bsn! { (menu_item_accel(icons::FILE_PLUS, "New", "⌘N")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SceneIoRequest::New); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item_accel(icons::FOLDER_OPEN, "Open Scene", "⌘O")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(OpenOpenDialog); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item_accel(icons::SAVE, "Save", "⌘S")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SceneIoRequest::Save); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item_accel(icons::SAVE, "Save As", "⇧⌘S")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(OpenSaveDialog); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::IMPORT, "Import Asset")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(OpenImportDialog); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::REMOTE, "Connect to Remote")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(OpenConnectDialog); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::CLOSE, "Quit")) } }
                    on(|_: On<Activate>, mut exit: MessageWriter<AppExit>| {
                        exit.write(AppExit::Success);
                    })),
            ]),
        ]
    }
}

fn edit_menu() -> impl Scene {
    bsn! {
        @FeathersMenu
        Children [
            (@FeathersMenuButton {
                @caption: bsn! { Text("Edit") ThemedText },
                @arrow: false,
            }),
            (@FeathersMenuPopup Children [
                (@FeathersMenuItem { @caption: bsn! { (menu_item_accel(icons::UNDO, "Undo", "⌘Z")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(RequestUndo); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item_accel(icons::REDO, "Redo", "⇧⌘Z")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(RequestRedo); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { (menu_item_accel(icons::DUPLICATE, "Duplicate", "⌘D")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(DuplicateRequest); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item_accel(icons::TRASH, "Delete Selected", "Del")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(DeleteSelectedRequest); })),
            ]),
        ]
    }
}

fn entity_menu() -> impl Scene {
    bsn! {
        @FeathersMenu
        Children [
            (@FeathersMenuButton {
                @caption: bsn! { Text("Entity") ThemedText },
                @arrow: false,
            }),
            (@FeathersMenuPopup Children [
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::CUBE, "Cube")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::Cube)); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SPHERE, "Sphere")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::Sphere)); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SQUARE, "Plane")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::Plane)); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::LIGHT, "Point Light")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::PointLight)); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::DIR_LIGHT, "Directional Light")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::DirectionalLight)); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SPRITE, "Sprite (2D)")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::Sprite)); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::EMPTY, "Empty")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::Empty)); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SQUARE, "UI Node")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::UiNode)); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::LIST, "UI Text")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::UiText)); })),
            ]),
        ]
    }
}

fn gameobject_menu() -> impl Scene {
    bsn! {
        @FeathersMenu
        Children [
            (@FeathersMenuButton {
                @caption: bsn! { Text("GameObject") ThemedText },
                @arrow: false,
            }),
            (@FeathersMenuPopup Children [
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::CUBE, "Physics Cube")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(crate::gameplay::SpawnPhysicsCube); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SUCCESS, "Particle Emitter")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(crate::gameplay::SpawnParticleEmitter); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::GRID, "Tilemap")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(crate::gameplay::SpawnTilemap); })),
            ]),
        ]
    }
}

fn view_menu() -> impl Scene {
    bsn! {
        @FeathersMenu
        Children [
            (@FeathersMenuButton {
                @caption: bsn! { Text("View") ThemedText },
                @arrow: false,
            }),
            (@FeathersMenuPopup Children [
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::CUBE, "Toggle 2D / 3D")) } }
                    on(|_: On<Activate>, mut m: ResMut<ViewportMode>| { m.toggle(); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item_accel(icons::FRAME, "Frame Selection", "F")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(crate::viewport::FrameSelectionRequest); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SUN, "Toggle Light / Dark")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(crate::ui::ToggleTheme); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item_accel(icons::TERMINAL, "Toggle Console", "`")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(crate::ui::ToggleConsole); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item_accel(icons::COMMAND, "Command Palette", "⌘P")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(crate::ui::OpenCommandPalette); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item_accel(icons::INFO, "Keyboard Shortcuts", "?")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(crate::ui::OpenShortcuts); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SLIDERS, "Stats Panel")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(ShowBottomTab(BottomTab::Stats)); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::PLAY_MODE, "Animation Panel")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(ShowBottomTab(BottomTab::Animation)); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SPHERE, "Material Panel")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(ShowBottomTab(BottomTab::Material)); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::GRID, "Tilemap Panel")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(ShowBottomTab(BottomTab::Tilemap)); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SQUARE, "UI Layout Panel")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(ShowBottomTab(BottomTab::Ui)); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::BUILD, "Output Panel")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(ShowBottomTab(BottomTab::Output)); })),
            ]),
        ]
    }
}

fn build_menu() -> impl Scene {
    bsn! {
        @FeathersMenu
        Children [
            (@FeathersMenuButton {
                @caption: bsn! { Text("Build") ThemedText },
                @arrow: false,
            }),
            (@FeathersMenuPopup Children [
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::PLAY, "Run Game")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(RunGameRequest); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::CHECK, "Check (cargo check)")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(CargoCheckRequest); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::EXPORT, "Export Scene")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(ExportSceneRequest); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::BUILD, "Build Project")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(BuildProjectRequest); })),
            ]),
        ]
    }
}

// ---------------------------------------------------------------------------
// Toolbar
// ---------------------------------------------------------------------------

/// A compact toolbar icon button. `toggle` (when set) makes the button light up while its
/// mode/state is active (managed by [`sync_toolbar_active`]).
fn toolbar_group() -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: space::XS,
        }
    }
}

fn toolbar() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: space::MD,
            padding: UiRect::axes(px(6), px(4)),
            min_height: sizes::TOOLBAR_H,
            border: UiRect::bottom(px(1)),
        }
        ThemeBackgroundColor(etokens::TOOLBAR_BG)
        ThemeBorderColor(etokens::PANEL_BORDER)
        Children [
            (toolbar_group() Children [
                (@FeathersToolButton { @variant: ButtonVariant::Primary, @caption: bsn! { (icon(icons::PLAY)) } }
                    template_value(ToolbarToggle::RunState(EditorState::Playing))
                    on(|_: On<Activate>, mut s: ResMut<NextState<EditorState>>| { s.set(EditorState::Playing); })),
                (@FeathersToolButton { @caption: bsn! { (icon(icons::PAUSE)) } }
                    template_value(ToolbarToggle::RunState(EditorState::Paused))
                    on(|_: On<Activate>, mut s: ResMut<NextState<EditorState>>| { s.set(EditorState::Paused); })),
                (@FeathersToolButton { @caption: bsn! { (icon(icons::STOP)) } }
                    template_value(ToolbarToggle::RunState(EditorState::Editing))
                    on(|_: On<Activate>, mut s: ResMut<NextState<EditorState>>| { s.set(EditorState::Editing); })),
            ]),
            pane_header_divider(),
            (toolbar_group() Children [
                (@FeathersToolButton { @caption: bsn! { (icon(icons::GIZMO_MOVE)) } }
                    template_value(ToolbarToggle::Gizmo(GizmoMode::Translate))
                    on(|_: On<Activate>, mut g: ResMut<GizmoMode>| { *g = GizmoMode::Translate; })),
                (@FeathersToolButton { @caption: bsn! { (icon(icons::GIZMO_ROTATE)) } }
                    template_value(ToolbarToggle::Gizmo(GizmoMode::Rotate))
                    on(|_: On<Activate>, mut g: ResMut<GizmoMode>| { *g = GizmoMode::Rotate; })),
                (@FeathersToolButton { @caption: bsn! { (icon(icons::GIZMO_SCALE)) } }
                    template_value(ToolbarToggle::Gizmo(GizmoMode::Scale))
                    on(|_: On<Activate>, mut g: ResMut<GizmoMode>| { *g = GizmoMode::Scale; })),
            ]),
            pane_header_divider(),
            (toolbar_group() Children [
                (@FeathersToolButton { @caption: bsn! { (icon(icons::CUBE)) } }
                    template_value(ToolbarToggle::TwoD)
                    on(|_: On<Activate>, mut m: ResMut<ViewportMode>| { m.toggle(); })),
                (@FeathersToolButton { @caption: bsn! { (icon(icons::SNAP)) } }
                    template_value(ToolbarToggle::Snap)
                    on(|_: On<Activate>, mut s: ResMut<GizmoSnap>| { s.enabled = !s.enabled; })),
            ]),
            pane_header_divider(),
            (toolbar_group() Children [
                (@FeathersToolButton { @caption: bsn! { (icon(icons::CODE)) } }
                    template_value(ToolbarToggle::CodeView)
                    on(|_: On<Activate>, mut v: ResMut<MainView>| { v.toggle(); })),
                (@FeathersToolButton { @variant: ButtonVariant::Primary, @caption: bsn! { (icon(icons::PLAY)) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(RunGameRequest); })),
                (@FeathersToolButton { @caption: bsn! { (icon(icons::CHECK)) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(CargoCheckRequest); })),
            ]),
            pane_header_divider(),
            (toolbar_group() Children [
                (@FeathersToolButton { @caption: bsn! { (icon(icons::UNDO)) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(RequestUndo); })),
                (@FeathersToolButton { @caption: bsn! { (icon(icons::REDO)) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(RequestRedo); })),
            ]),
        ]
    }
}

// ---------------------------------------------------------------------------
// Tab bar (multi-scene tabs)
// ---------------------------------------------------------------------------

fn tab_bar() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: space::XS,
            padding: UiRect::axes(px(4), px(3)),
            min_height: sizes::TABBAR_H,
            border: UiRect::bottom(px(1)),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        ThemeBorderColor(etokens::PANEL_BORDER)
        TabBarContent
    }
}

// ---------------------------------------------------------------------------
// Main row: Hierarchy | (Viewport / Code over Assets) | Inspector
// ---------------------------------------------------------------------------

fn main_row() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            flex_grow: 1.0,
            min_height: px(0),
            align_items: AlignItems::Stretch,
        }
        Children [
            hierarchy_panel(),
            splitter(ResizeSide::Prev, SplitAxis::Horizontal),
            center_column(),
            splitter(ResizeSide::Next, SplitAxis::Horizontal),
            inspector_panel(),
        ]
    }
}

/// The center column: the viewport/code stack on top, the Assets panel below, divided by a
/// horizontal splitter.
fn center_column() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            min_width: px(150),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        }
        Children [
            center_area(),
            splitter(ResizeSide::Next, SplitAxis::Vertical),
            assets_panel(),
        ]
    }
}

/// The scene viewport and the code editor stacked, with only the one matching the current
/// [`MainView`] shown.
fn center_area() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(80),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        }
        Children [
            viewport_panel(),
            code_panel(),
        ]
    }
}

/// The Hierarchy panel: header, entity search, and the entity tree.
fn hierarchy_panel() -> impl Scene {
    bsn! {
        Node {
            width: sizes::HIERARCHY_W,
            min_width: px(140),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        }
        Children [
            panel_header(icons::LIST, "Hierarchy"),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: space::SM,
                    padding: UiRect::axes(space::MD, px(4)),
                }
                ThemeBackgroundColor(tokens::PANE_BODY_BG)
                Children [
                    (icon(icons::SEARCH) ThemedText Pickable::IGNORE),
                    (@FeathersTextInputContainer
                        Node { flex_grow: 1.0 }
                        Children [
                            (@FeathersTextInput SeedText(String::new()) HierarchySearch)
                        ]),
                ]
            ),
            (
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: space::SM,
                    row_gap: px(1),
                    overflow: Overflow::scroll_y(),
                }
                ThemeBackgroundColor(tokens::PANE_BODY_BG)
                ScrollArea
                HierarchyContent
            ),
        ]
    }
}

fn viewport_panel() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            min_width: px(150),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        }
        template_value(MainViewNode(MainView::Scene))
        Children [
            panel_header(icons::CUBE, "Viewport"),
            (
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                }
                ThemeBackgroundColor(tokens::WINDOW_BG)
                ViewportSlot
            ),
        ]
    }
}

/// The Inspector panel: header + the reflection-driven component editor list.
fn inspector_panel() -> impl Scene {
    bsn! {
        Node {
            width: sizes::INSPECTOR_W,
            min_width: px(180),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        }
        Children [
            panel_header(icons::SLIDERS, "Inspector"),
            (
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: space::MD,
                    row_gap: space::SM,
                    overflow: Overflow::scroll_y(),
                }
                ThemeBackgroundColor(tokens::PANE_BODY_BG)
                ScrollArea
                InspectorContent
            ),
        ]
    }
}

/// The Assets panel: header + the asset-browser entries.
fn assets_panel() -> impl Scene {
    bsn! {
        Node {
            height: sizes::ASSET_ROW_H,
            min_height: px(80),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        }
        Children [
            panel_header(icons::FOLDER_TREE, "Assets"),
            (
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: space::SM,
                    row_gap: px(1),
                    align_items: AlignItems::Stretch,
                    overflow: Overflow::scroll_y(),
                }
                ThemeBackgroundColor(tokens::PANE_BODY_BG)
                ScrollArea
                AssetContent
            ),
        ]
    }
}

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

/// A plain (non-dockable) panel header: a leading icon + title with the panel-header look.
fn panel_header(icon_path: &'static str, title: impl Into<String>) -> impl Scene {
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
            (icon(icon_path) ThemedText),
            label(title),
        ]
    }
}

/// A draggable splitter handle that resizes a neighboring panel. Horizontal handles resize a
/// column's width; vertical handles resize a row's height.
fn splitter(side: ResizeSide, axis: SplitAxis) -> impl Scene {
    let (width, height, cursor) = match axis {
        SplitAxis::Horizontal => (sizes::SPLITTER_W, Val::Auto, SystemCursorIcon::EwResize),
        SplitAxis::Vertical => (Val::Auto, sizes::SPLITTER_W, SystemCursorIcon::NsResize),
    };
    bsn! {
        Node {
            width: width,
            height: height,
            align_self: AlignSelf::Stretch,
        }
        ThemeBackgroundColor(etokens::PANEL_BORDER)
        Splitter { resize: side, axis: axis }
        bevy_feathers::cursor::EntityCursor::System(cursor)
        on(on_splitter_drag)
    }
}
