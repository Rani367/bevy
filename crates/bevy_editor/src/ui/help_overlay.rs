//! A keyboard-shortcuts cheat-sheet overlay (`?` or `F1`). The binding table is data-driven —
//! the same `const` drives the rendered sheet, so it can't drift from a hand-written doc. Opened
//! from the View menu and the command palette too.

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::display::label_small;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};
use bevy_feathers::tokens;
use bevy_input::keyboard::KeyCode;
use bevy_input::ButtonInput;
use bevy_input_focus::InputFocus;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_text::EditableText;
use bevy_ui::{px, AlignItems, AlignSelf, BorderRadius, Display, FlexDirection, Node, UiRect};

use crate::ui::style::{dialog_frame, etokens, section_header, sizes};

/// Open the keyboard-shortcuts cheat sheet.
#[derive(Event, Clone, Copy)]
pub struct OpenShortcuts;

/// Marks the container the shortcut rows are spawned into.
#[derive(Component, Default, Clone, Copy)]
struct ShortcutsList;

/// The cheat-sheet content, grouped by category. `(keys, description)` per row.
const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    (
        "File & Project",
        &[
            ("⌘N", "New scene"),
            ("⌘O", "Open scene"),
            ("⌘S", "Save scene"),
            ("⇧⌘S", "Save scene as…"),
            ("⌘P", "Command palette"),
        ],
    ),
    (
        "Edit",
        &[
            ("⌘Z", "Undo"),
            ("⇧⌘Z", "Redo"),
            ("⌘D", "Duplicate selection"),
            ("Del", "Delete selection"),
            ("F2", "Rename selection"),
        ],
    ),
    (
        "Transform Tools",
        &[
            ("W", "Move tool"),
            ("E", "Rotate tool"),
            ("R", "Scale tool"),
            ("F", "Frame selection"),
            ("Ctrl", "Hold to snap while dragging"),
        ],
    ),
    (
        "View & Panels",
        &[
            ("`", "Toggle console"),
            ("? / F1", "This shortcut sheet"),
            ("Drag tab", "Move / tab / float a panel"),
            ("Esc", "Close dialog / overlay"),
        ],
    ),
];

fn on_open_shortcuts(_: On<OpenShortcuts>, mut commands: Commands) {
    commands.spawn_scene(dialog_frame(
        "Keyboard Shortcuts",
        px(460),
        bsn! {
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                }
                ShortcutsList
            )
        },
    ));
}

/// Fill the (initially empty) list once it appears.
fn populate_shortcuts(lists: Query<Entity, Added<ShortcutsList>>, mut commands: Commands) {
    let Ok(list) = lists.single() else {
        return;
    };
    let mut rows: Vec<Box<dyn SceneList>> = Vec::new();
    for (category, entries) in SHORTCUTS {
        rows.push(Box::new(EntityScene(section_header(
            category.to_string(),
            bsn! { Node {} },
        ))) as Box<dyn SceneList>);
        for (keys, desc) in *entries {
            rows.push(Box::new(EntityScene(shortcut_row(keys, desc))) as Box<dyn SceneList>);
        }
    }
    commands
        .entity(list)
        .queue_spawn_related_scenes::<Children>(rows);
}

/// One shortcut row: a keycap chip + a description.
fn shortcut_row(keys: &str, desc: &str) -> impl Scene {
    let keys = keys.to_string();
    let desc = desc.to_string();
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(12),
            padding: UiRect::axes(px(6), px(3)),
            min_height: sizes::ROW_H,
        }
        Children [
            (
                Node {
                    width: px(92),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                }
                Children [ (keycap(keys)) ]
            ),
            (label_small(desc)),
        ]
    }
}

/// A small keycap-styled chip holding a key combo.
fn keycap(keys: String) -> impl Scene {
    bsn! {
        Node {
            padding: UiRect::axes(px(7), px(2)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(4)),
            align_self: AlignSelf::FlexStart,
        }
        ThemeBackgroundColor(tokens::PANE_HEADER_BG)
        ThemeBorderColor(etokens::PANEL_BORDER)
        Children [ (label_small(keys)) ]
    }
}

/// `?` (Shift+/) or `F1` opens the sheet, unless a text field is focused.
fn shortcuts_hotkey(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    editables: Query<(), With<EditableText>>,
    mut commands: Commands,
) {
    let typing = focus.get().is_some_and(|e| editables.contains(e));
    if typing {
        return;
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if keys.just_pressed(KeyCode::F1) || (shift && keys.just_pressed(KeyCode::Slash)) {
        commands.trigger(OpenShortcuts);
    }
}

/// Installs the shortcuts cheat-sheet overlay.
pub struct HelpOverlayPlugin;

impl Plugin for HelpOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (populate_shortcuts, shortcuts_hotkey))
            .add_observer(on_open_shortcuts);
    }
}
