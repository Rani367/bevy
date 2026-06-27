//! The bottom status bar: a compact, always-visible readout of the editor's live state —
//! viewport mode, selection count, active gizmo, snap state, current scene, and FPS.

use bevy_app::{App, Plugin, PropagateOver, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::{
    constants::{fonts, size},
    display::icon,
    theme::{InheritableThemeTextColor, ThemeBackgroundColor, ThemeBorderColor, ThemedText},
};
use bevy_scene::{bsn, template_value, Scene};
use bevy_text::{FontSourceTemplate, FontWeight, TextFont};
use bevy_time::Time;
use bevy_ui::{px, widget::Text, AlignItems, Display, FlexDirection, Node, UiRect};

use crate::markers::EditorEntity;
use crate::scene_io::CurrentScene;
use crate::state::{EditorSelection, GizmoMode, GizmoSnap, ViewportMode};
use crate::ui::icons;
use crate::ui::style::{etokens, sizes};

/// Identifies which live value a status-bar text span displays.
#[derive(Component, Clone, Copy, PartialEq, Eq, Default)]
enum StatusField {
    /// Viewport 2D/3D mode.
    #[default]
    Mode,
    /// Selection count.
    Selection,
    /// Active gizmo mode.
    Gizmo,
    /// Snap on/off.
    Snap,
    /// Current scene name + dirty marker.
    Scene,
    /// Smoothed FPS.
    Fps,
}

/// Smoothed frames-per-second, so the readout doesn't jitter every frame.
#[derive(Resource, Default)]
struct FpsSmooth(f32);

/// A small themed text span for a status segment (inherits the status-bar text color).
fn seg_text(text: impl Into<String>) -> impl Scene {
    bsn! {
        Text(text)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: size::SMALL_FONT,
            weight: FontWeight::NORMAL,
        }
        PropagateOver<TextFont>
        ThemedText
    }
}

/// The status bar row, placed at the very bottom of the editor shell.
pub fn status_bar() -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            min_height: sizes::STATUS_BAR_H,
            padding: UiRect::horizontal(px(4)),
            column_gap: px(2),
            border: UiRect::top(px(1)),
        }
        EditorEntity
        ThemeBackgroundColor(etokens::STATUS_BAR_BG)
        ThemeBorderColor(etokens::PANEL_BORDER)
        InheritableThemeTextColor(etokens::STATUS_BAR_TEXT)
        Children [
            (seg() Children [ (icon(icons::CUBE) ThemedText), (seg_text("3D") template_value(StatusField::Mode)) ]),
            (seg() Children [ (icon(icons::EMPTY) ThemedText), (seg_text("0 selected") template_value(StatusField::Selection)) ]),
            (seg() Children [ (icon(icons::GIZMO_MOVE) ThemedText), (seg_text("Move") template_value(StatusField::Gizmo)) ]),
            (seg() Children [ (icon(icons::SNAP) ThemedText), (seg_text("Snap off") template_value(StatusField::Snap)) ]),
            // Spacer pushes the scene + FPS segments to the right edge.
            (Node { flex_grow: 1.0 }),
            (seg() Children [ (icon(icons::FILE) ThemedText), (seg_text("Untitled") template_value(StatusField::Scene)) ]),
            (seg() Children [ (icon(icons::SETTINGS) ThemedText), (seg_text("-- fps") template_value(StatusField::Fps)) ]),
        ]
    }
}

/// One status-bar segment container (icon + label laid out in a row).
fn seg() -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(4),
            padding: UiRect::horizontal(px(8)),
        }
    }
}

/// Keep the status-bar text in sync with editor state. Each span only rewrites its `Text`
/// when the displayed string actually changes, so this never triggers needless re-layout.
fn update_status_bar(
    time: Res<Time>,
    mut fps: ResMut<FpsSmooth>,
    selection: Res<EditorSelection>,
    gizmo: Res<GizmoMode>,
    snap: Res<GizmoSnap>,
    vmode: Res<ViewportMode>,
    scene: Option<Res<CurrentScene>>,
    mut spans: Query<(&mut Text, &StatusField)>,
) {
    // Smoothed FPS (exponential moving average).
    let dt = time.delta_secs();
    if dt > 0.0 {
        let inst = 1.0 / dt;
        fps.0 = if fps.0 <= 0.0 {
            inst
        } else {
            fps.0 * 0.9 + inst * 0.1
        };
    }

    for (mut text, field) in spans.iter_mut() {
        let value = match field {
            StatusField::Mode => match *vmode {
                ViewportMode::TwoD => "2D".to_string(),
                ViewportMode::ThreeD => "3D".to_string(),
            },
            StatusField::Selection => match selection.all.len() {
                0 => "Nothing selected".to_string(),
                1 => "1 selected".to_string(),
                n => format!("{n} selected"),
            },
            StatusField::Gizmo => match *gizmo {
                GizmoMode::Translate => "Move",
                GizmoMode::Rotate => "Rotate",
                GizmoMode::Scale => "Scale",
            }
            .to_string(),
            StatusField::Snap => if snap.enabled { "Snap on" } else { "Snap off" }.to_string(),
            StatusField::Scene => scene
                .as_ref()
                .map(|s| s.display_name())
                .unwrap_or_else(|| "Untitled".to_string()),
            StatusField::Fps => format!("{:.0} fps", fps.0),
        };
        if text.0 != value {
            text.0 = value;
        }
    }
}

/// Installs the status-bar update system.
pub struct StatusBarPlugin;

impl Plugin for StatusBarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FpsSmooth>()
            .add_systems(Update, update_status_bar);
    }
}
