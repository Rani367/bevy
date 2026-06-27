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
    display::{icon, label},
    theme::{ThemeBackgroundColor, ThemeBorderColor, ThemedText},
    tokens,
};
use bevy_picking::Pickable;
use bevy_scene::prelude::*;
use bevy_state::state::{NextState, State};
use bevy_ui::widget::Text;
use bevy_ui::{
    percent, px, AlignItems, AlignSelf, Display, FlexDirection, FlexWrap, GlobalZIndex,
    IsDefaultUiCamera, Node, Overflow, UiRect,
};
use bevy_ui_widgets::{Activate, ScrollArea};
use bevy_window::SystemCursorIcon;

use crate::actions::{
    DeleteSelectedRequest, DuplicateRequest, OpenImportDialog, OpenOpenDialog, OpenSaveDialog,
    SceneIoRequest, SpawnKind, SpawnRequest,
};
use crate::build_export::{BuildProjectRequest, ExportSceneRequest};
use crate::markers::EditorEntity;
use crate::remote::OpenConnectDialog;
use crate::state::{EditorState, GizmoMode, GizmoSnap, ViewportMode};
use crate::ui::icons;
use crate::ui::style::{etokens, sizes, space};
use crate::undo::{RequestRedo, RequestUndo};

use super::docking::{
    Panel, PanelCollapseButton, PanelContent, PanelFloatButton, PanelHeader, PanelId,
};
use super::splitter::{on_splitter_drag, ResizeSide, Splitter};
use super::{
    AssetContent, EditorUiCamera, HierarchyContent, InspectorContent, TabBarContent, ViewportSlot,
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
}

/// Light up toolbar buttons whose mode/state is currently active. Runs only when one of the
/// reflected resources changes, so it's effectively free at idle.
pub fn sync_toolbar_active(
    gizmo: Res<GizmoMode>,
    run_state: Res<State<EditorState>>,
    vmode: Res<ViewportMode>,
    snap: Res<GizmoSnap>,
    mut buttons: Query<(&ToolbarToggle, &mut ButtonVariant)>,
) {
    if !(gizmo.is_changed() || run_state.is_changed() || vmode.is_changed() || snap.is_changed()) {
        return;
    }
    for (toggle, mut variant) in buttons.iter_mut() {
        let active = match toggle {
            ToolbarToggle::RunState(s) => run_state.get() == s,
            ToolbarToggle::Gizmo(m) => *gizmo == *m,
            ToolbarToggle::TwoD => *vmode == ViewportMode::TwoD,
            ToolbarToggle::Snap => snap.enabled,
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
            body_row(),
            asset_row(),
            crate::ui::console::console_panel(),
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
            file_menu(),
            edit_menu(),
            entity_menu(),
            view_menu(),
            build_menu(),
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

fn file_menu() -> impl Scene {
    bsn! {
        @FeathersMenu
        Children [
            (@FeathersMenuButton {
                @caption: bsn! { Text("File") ThemedText },
                @arrow: false,
            }),
            (@FeathersMenuPopup Children [
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::FILE_PLUS, "New")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SceneIoRequest::New); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::FOLDER_OPEN, "Open Scene")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(OpenOpenDialog); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SAVE, "Save")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SceneIoRequest::Save); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SAVE, "Save As")) } }
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
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::UNDO, "Undo")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(RequestUndo); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::REDO, "Redo")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(RequestRedo); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::DUPLICATE, "Duplicate")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(DuplicateRequest); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::TRASH, "Delete Selected")) } }
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
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::FRAME, "Frame Selection")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(crate::viewport::FrameSelectionRequest); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::SUN, "Toggle Light / Dark")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(crate::ui::ToggleTheme); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::TERMINAL, "Toggle Console")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(crate::ui::ToggleConsole); })),
                (@FeathersMenuItem { @caption: bsn! { (menu_item(icons::COMMAND, "Command Palette")) } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(crate::ui::OpenCommandPalette); })),
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
// Body row: Hierarchy | Viewport | Inspector
// ---------------------------------------------------------------------------

fn body_row() -> impl Scene {
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
            splitter_v(ResizeSide::Prev),
            viewport_panel(),
            splitter_v(ResizeSide::Next),
            inspector_panel(),
        ]
    }
}

fn hierarchy_panel() -> impl Scene {
    bsn! {
        Node {
            width: sizes::HIERARCHY_W,
            min_width: px(140),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        }
        Panel(PanelId::Hierarchy)
        GlobalZIndex(0)
        Children [
            dockable_header(icons::LIST, "Hierarchy", PanelId::Hierarchy),
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
                PanelContent(PanelId::Hierarchy)
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

fn inspector_panel() -> impl Scene {
    bsn! {
        Node {
            width: sizes::INSPECTOR_W,
            min_width: px(180),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        }
        Panel(PanelId::Inspector)
        GlobalZIndex(0)
        Children [
            dockable_header(icons::SLIDERS, "Inspector", PanelId::Inspector),
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
                PanelContent(PanelId::Inspector)
            ),
        ]
    }
}

fn asset_row() -> impl Scene {
    bsn! {
        Node {
            min_height: sizes::ASSET_ROW_H,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            border: UiRect::top(px(1)),
        }
        ThemeBorderColor(etokens::PANEL_BORDER)
        Panel(PanelId::Assets)
        GlobalZIndex(0)
        Children [
            dockable_header(icons::FOLDER_TREE, "Assets", PanelId::Assets),
            (
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    padding: space::LG,
                    column_gap: space::LG,
                    row_gap: space::LG,
                    align_items: AlignItems::Start,
                    overflow: Overflow::scroll_y(),
                }
                ThemeBackgroundColor(tokens::PANE_BODY_BG)
                ScrollArea
                AssetContent
                PanelContent(PanelId::Assets)
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

/// A header for a dockable panel: a leading icon + draggable title (drag to float/move the
/// panel) with collapse and float toggle buttons (icons).
fn dockable_header(icon_path: &'static str, title: impl Into<String>, id: PanelId) -> impl Scene {
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
        PanelHeader(id)
        Children [
            (icon(icon_path) ThemedText Pickable::IGNORE),
            // The title area ignores picking so drags fall through to the header itself.
            (Node { flex_grow: 1.0 } Pickable::IGNORE Children [ (label(title) Pickable::IGNORE) ]),
            (@FeathersToolButton { @variant: ButtonVariant::Plain, @caption: bsn! { (icon(icons::CHEVRON_DOWN)) } }
                PanelCollapseButton(id)),
            (@FeathersToolButton { @variant: ButtonVariant::Plain, @caption: bsn! { (icon(icons::FLOAT)) } }
                PanelFloatButton(id)),
        ]
    }
}

fn splitter_v(side: ResizeSide) -> impl Scene {
    bsn! {
        Node {
            width: sizes::SPLITTER_W,
            align_self: AlignSelf::Stretch,
        }
        ThemeBackgroundColor(etokens::PANEL_BORDER)
        Splitter { resize: side }
        bevy_feathers::cursor::EntityCursor::System(SystemCursorIcon::EwResize)
        on(on_splitter_drag)
    }
}
