//! The editor shell layout, authored with the `bsn!` scene macro: a window-filling
//! column of a menu bar, a toolbar, the Hierarchy / Viewport / Inspector body row, and
//! an asset row along the bottom. Menu and toolbar interactions emit editor action
//! events (see [`crate::actions`]) or poke editor-state resources directly.

use bevy_app::AppExit;
use bevy_camera::Camera2d;
use bevy_ecs::message::MessageWriter;
use bevy_ecs::prelude::*;
use bevy_feathers::{
    containers::pane_header_divider,
    controls::*,
    display::label,
    theme::{ThemeBackgroundColor, ThemedText},
    tokens,
};
use bevy_scene::prelude::*;
use bevy_state::state::NextState;
use bevy_ui::widget::Text;
use bevy_ui::{
    percent, px, AlignItems, AlignSelf, Display, FlexDirection, FlexWrap, IsDefaultUiCamera, Node,
    Overflow, UiRect,
};
use bevy_ui_widgets::{Activate, ScrollArea};
use bevy_window::SystemCursorIcon;

use crate::actions::{
    DeleteSelectedRequest, OpenImportDialog, OpenOpenDialog, OpenSaveDialog, SceneIoRequest,
    SpawnKind, SpawnRequest,
};
use crate::build_export::{BuildProjectRequest, ExportSceneRequest};
use crate::markers::EditorEntity;
use crate::remote::OpenConnectDialog;
use crate::state::{EditorState, GizmoMode, ViewportMode};
use crate::undo::{RequestRedo, RequestUndo};

use super::splitter::{on_splitter_drag, ResizeSide, Splitter};
use super::{
    AssetContent, EditorUiCamera, HierarchyContent, InspectorContent, TabBarContent, ViewportSlot,
};

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
            column_gap: px(2),
            padding: px(2),
            min_height: px(30),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [
            file_menu(),
            edit_menu(),
            entity_menu(),
            view_menu(),
            build_menu(),
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
                (@FeathersMenuItem { @caption: bsn! { Text("New") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SceneIoRequest::New); })),
                (@FeathersMenuItem { @caption: bsn! { Text("Open Scene") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(OpenOpenDialog); })),
                (@FeathersMenuItem { @caption: bsn! { Text("Save") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SceneIoRequest::Save); })),
                (@FeathersMenuItem { @caption: bsn! { Text("Save As") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(OpenSaveDialog); })),
                (@FeathersMenuItem { @caption: bsn! { Text("Import Asset") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(OpenImportDialog); })),
                (@FeathersMenuItem { @caption: bsn! { Text("Connect to Remote") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(OpenConnectDialog); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { Text("Quit") ThemedText } }
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
                (@FeathersMenuItem { @caption: bsn! { Text("Undo") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(RequestUndo); })),
                (@FeathersMenuItem { @caption: bsn! { Text("Redo") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(RequestRedo); })),
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
                (@FeathersMenuItem { @caption: bsn! { Text("Cube") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::Cube)); })),
                (@FeathersMenuItem { @caption: bsn! { Text("Sphere") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::Sphere)); })),
                (@FeathersMenuItem { @caption: bsn! { Text("Plane") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::Plane)); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { Text("Point Light") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::PointLight)); })),
                (@FeathersMenuItem { @caption: bsn! { Text("Directional Light") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::DirectionalLight)); })),
                @FeathersMenuDivider,
                (@FeathersMenuItem { @caption: bsn! { Text("Sprite (2D)") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(SpawnRequest(SpawnKind::Sprite)); })),
                (@FeathersMenuItem { @caption: bsn! { Text("Empty") ThemedText } }
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
                (@FeathersMenuItem { @caption: bsn! { Text("Toggle 2D / 3D") ThemedText } }
                    on(|_: On<Activate>, mut m: ResMut<ViewportMode>| { m.toggle(); })),
                (@FeathersMenuItem { @caption: bsn! { Text("Delete Selected") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(DeleteSelectedRequest); })),
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
                (@FeathersMenuItem { @caption: bsn! { Text("Export Scene") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(ExportSceneRequest); })),
                (@FeathersMenuItem { @caption: bsn! { Text("Build Project") ThemedText } }
                    on(|_: On<Activate>, mut c: Commands| { c.trigger(BuildProjectRequest); })),
            ]),
        ]
    }
}

// ---------------------------------------------------------------------------
// Toolbar
// ---------------------------------------------------------------------------

fn toolbar() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(4),
            padding: px(4),
            min_height: px(34),
        }
        ThemeBackgroundColor(tokens::PANE_HEADER_BG)
        Children [
            (@FeathersToolButton {
                @variant: ButtonVariant::Primary,
                @caption: bsn! { Text("Play") ThemedText },
            }
                on(|_: On<Activate>, mut s: ResMut<NextState<EditorState>>| { s.set(EditorState::Playing); })),
            (@FeathersToolButton { @caption: bsn! { Text("Pause") ThemedText } }
                on(|_: On<Activate>, mut s: ResMut<NextState<EditorState>>| { s.set(EditorState::Paused); })),
            (@FeathersToolButton { @caption: bsn! { Text("Stop") ThemedText } }
                on(|_: On<Activate>, mut s: ResMut<NextState<EditorState>>| { s.set(EditorState::Editing); })),
            pane_header_divider(),
            (@FeathersToolButton { @caption: bsn! { Text("Move") ThemedText } }
                on(|_: On<Activate>, mut g: ResMut<GizmoMode>| { *g = GizmoMode::Translate; })),
            (@FeathersToolButton { @caption: bsn! { Text("Rotate") ThemedText } }
                on(|_: On<Activate>, mut g: ResMut<GizmoMode>| { *g = GizmoMode::Rotate; })),
            (@FeathersToolButton { @caption: bsn! { Text("Scale") ThemedText } }
                on(|_: On<Activate>, mut g: ResMut<GizmoMode>| { *g = GizmoMode::Scale; })),
            pane_header_divider(),
            (@FeathersToolButton { @caption: bsn! { Text("2D / 3D") ThemedText } }
                on(|_: On<Activate>, mut m: ResMut<ViewportMode>| { m.toggle(); })),
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
            column_gap: px(2),
            padding: px(3),
            min_height: px(26),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
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
            width: px(240),
            min_width: px(120),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        }
        Children [
            panel_header("Hierarchy"),
            (
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: px(4),
                    row_gap: px(2),
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
        Children [
            panel_header("Viewport"),
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
            width: px(300),
            min_width: px(150),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        }
        Children [
            panel_header("Inspector"),
            (
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: px(6),
                    row_gap: px(4),
                    overflow: Overflow::scroll_y(),
                }
                ThemeBackgroundColor(tokens::PANE_BODY_BG)
                ScrollArea
                InspectorContent
            ),
        ]
    }
}

fn asset_row() -> impl Scene {
    bsn! {
        Node {
            min_height: px(140),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        }
        Children [
            panel_header("Assets"),
            (
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    padding: px(6),
                    column_gap: px(6),
                    row_gap: px(6),
                    align_items: AlignItems::Start,
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

fn panel_header(title: impl Into<String>) -> impl Scene {
    bsn! {
        Node {
            min_height: px(28),
            padding: UiRect::horizontal(px(8)),
            align_items: AlignItems::Center,
        }
        ThemeBackgroundColor(tokens::PANE_HEADER_BG)
        Children [
            label(title)
        ]
    }
}

fn splitter_v(side: ResizeSide) -> impl Scene {
    bsn! {
        Node {
            width: px(5),
            align_self: AlignSelf::Stretch,
        }
        ThemeBackgroundColor(tokens::PANE_HEADER_DIVIDER)
        Splitter { resize: side }
        bevy_feathers::cursor::EntityCursor::System(SystemCursorIcon::EwResize)
        on(on_splitter_drag)
    }
}
