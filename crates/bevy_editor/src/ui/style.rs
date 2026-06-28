//! The editor's design system: named spacing/size/z-layer constants, editor-specific
//! theme tokens (registered on top of the Feathers theme), and small reusable scene
//! builders. Everything visual in the editor should pull from here rather than
//! hand-rolling magic numbers, so the look stays consistent and re-themable.

use bevy_color::{Alpha, Color, Luminance, Srgba};
use bevy_ecs::hierarchy::Children;
use bevy_ecs::prelude::{Commands, On};
use bevy_feathers::{
    dark_theme::create_dark_theme,
    display::{label, label_small},
    palette,
    theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeProps, ThemedText},
    tokens,
};
use bevy_picking::events::{Click, Pointer};
use bevy_scene::{bsn, on, Scene};
use bevy_ui::{
    px, AlignItems, BorderRadius, BoxShadow, Display, FlexDirection, GlobalZIndex, JustifyContent,
    Node, Overflow, PositionType, UiRect, Val,
};

use crate::markers::EditorEntity;
use crate::ui::{stop_click, CloseOverlay, EditorOverlay};

/// Spacing rhythm (paddings, gaps). Use these instead of raw `px(...)` so the editor
/// has a consistent visual cadence.
pub mod space {
    use bevy_ui::Val;
    /// 2px — tightest gap (e.g. between menu buttons).
    pub const XS: Val = Val::Px(2.0);
    /// 4px — compact gap (rows, small lists).
    pub const SM: Val = Val::Px(4.0);
    /// 6px — default panel padding / field gap.
    pub const MD: Val = Val::Px(6.0);
    /// 8px — comfortable gap (dialog rows, button groups).
    pub const LG: Val = Val::Px(8.0);
    /// 12px — section separation.
    pub const XL: Val = Val::Px(12.0);
}

/// Fixed element sizes shared across the shell.
pub mod sizes {
    use bevy_ui::Val;
    /// Menu bar height.
    pub const MENUBAR_H: Val = Val::Px(30.0);
    /// Toolbar height.
    pub const TOOLBAR_H: Val = Val::Px(38.0);
    /// Scene-tab strip height.
    pub const TABBAR_H: Val = Val::Px(30.0);
    /// Panel header height (matches Feathers `HEADER_HEIGHT`).
    pub const PANEL_HEADER_H: Val = Val::Px(30.0);
    /// Status bar height.
    pub const STATUS_BAR_H: Val = Val::Px(24.0);
    /// Default tree/inspector row height.
    pub const ROW_H: Val = Val::Px(22.0);
    /// Inspector field label column width.
    pub const LABEL_COL: Val = Val::Px(108.0);
    /// Hierarchy panel default width.
    pub const HIERARCHY_W: Val = Val::Px(248.0);
    /// Inspector panel default width.
    pub const INSPECTOR_W: Val = Val::Px(320.0);
    /// Asset row default height.
    pub const ASSET_ROW_H: Val = Val::Px(156.0);
    /// Splitter handle thickness.
    pub const SPLITTER_W: Val = Val::Px(6.0);
    /// Square icon-button edge length.
    pub const ICON_BTN: Val = Val::Px(26.0);
    /// Per-depth indentation step in the hierarchy tree (px).
    pub const INDENT_STEP: f32 = 14.0;
}

/// `GlobalZIndex` layers, lowest to highest. Keeps overlay stacking coherent.
pub mod z {
    /// A panel torn off to float over the docked layout.
    pub const FLOATING_PANEL: i32 = 50;
    /// Viewport HUD overlays (mode/snap/camera hints).
    pub const HUD: i32 = 100;
    /// Modal/floating dialogs.
    pub const DIALOG: i32 = 1000;
    /// The command palette (above dialogs).
    pub const COMMAND_PALETTE: i32 = 1200;
    /// Right-click context menus.
    pub const CONTEXT_MENU: i32 = 1500;
    /// Transient toast notifications (always on top).
    pub const TOAST: i32 = 3000;
}

/// Editor-specific theme tokens, layered on top of the Feathers token set. Registered by
/// [`create_editor_theme`]; named `editor.*` so they never collide with `feathers.*`.
pub mod etokens {
    use bevy_feathers::theme::ThemeToken;
    /// Background of a selected hierarchy/list row (accent tinted).
    pub const ROW_SELECTED: ThemeToken = ThemeToken::new_static("editor.row.selected");
    /// Background of a hovered hierarchy/list row.
    pub const ROW_HOVER: ThemeToken = ThemeToken::new_static("editor.row.hover");
    /// Toolbar background.
    pub const TOOLBAR_BG: ThemeToken = ThemeToken::new_static("editor.toolbar.bg");
    /// Status bar background.
    pub const STATUS_BAR_BG: ThemeToken = ThemeToken::new_static("editor.statusbar.bg");
    /// Status bar text/icon color.
    pub const STATUS_BAR_TEXT: ThemeToken = ThemeToken::new_static("editor.statusbar.text");
    /// Toast / floating-card background.
    pub const TOAST_BG: ThemeToken = ThemeToken::new_static("editor.toast.bg");
    /// Viewport HUD panel background (semi-transparent).
    pub const HUD_BG: ThemeToken = ThemeToken::new_static("editor.hud.bg");
    /// HUD text/icon color.
    pub const HUD_TEXT: ThemeToken = ThemeToken::new_static("editor.hud.text");
    /// Subtle border between panels / around cards.
    pub const PANEL_BORDER: ThemeToken = ThemeToken::new_static("editor.panel.border");
    /// Success accent (saved, build ok).
    pub const SUCCESS: ThemeToken = ThemeToken::new_static("editor.status.success");
    /// Warning accent.
    pub const WARNING: ThemeToken = ThemeToken::new_static("editor.status.warning");
    /// Error accent.
    pub const ERROR: ThemeToken = ThemeToken::new_static("editor.status.error");
    /// Informational accent (mirrors the Feathers accent blue).
    pub const INFO: ThemeToken = ThemeToken::new_static("editor.status.info");
    /// Dimming scrim drawn behind modal dialogs.
    pub const SCRIM: ThemeToken = ThemeToken::new_static("editor.scrim");

    // Code-editor syntax highlighting colors.
    /// Keywords (`fn`, `let`, …).
    pub const SYNTAX_KEYWORD: ThemeToken = ThemeToken::new_static("editor.syntax.keyword");
    /// Type names.
    pub const SYNTAX_TYPE: ThemeToken = ThemeToken::new_static("editor.syntax.type");
    /// Function-call identifiers.
    pub const SYNTAX_FUNCTION: ThemeToken = ThemeToken::new_static("editor.syntax.function");
    /// Macro invocations.
    pub const SYNTAX_MACRO: ThemeToken = ThemeToken::new_static("editor.syntax.macro");
    /// String / char literals.
    pub const SYNTAX_STRING: ThemeToken = ThemeToken::new_static("editor.syntax.string");
    /// Numeric literals.
    pub const SYNTAX_NUMBER: ThemeToken = ThemeToken::new_static("editor.syntax.number");
    /// Comments.
    pub const SYNTAX_COMMENT: ThemeToken = ThemeToken::new_static("editor.syntax.comment");
    /// Punctuation / operators.
    pub const SYNTAX_PUNCT: ThemeToken = ThemeToken::new_static("editor.syntax.punct");
    /// Plain identifiers / default code text.
    pub const SYNTAX_NORMAL: ThemeToken = ThemeToken::new_static("editor.syntax.normal");
}

/// The editor's status/level colors, independent of light/dark (tuned to read on both).
pub mod accent {
    use bevy_color::Color;
    /// Success green.
    pub const SUCCESS: Color = Color::oklch(0.70, 0.16, 152.0);
    /// Warning amber.
    pub const WARNING: Color = Color::oklch(0.80, 0.15, 80.0);
    /// Error red.
    pub const ERROR: Color = Color::oklch(0.64, 0.21, 22.0);
}

/// Build the editor's dark theme: the Feathers dark theme plus the `editor.*` tokens.
pub fn create_editor_theme() -> ThemeProps {
    let mut theme = create_dark_theme();
    install_editor_tokens(&mut theme, false);
    theme
}

/// Build a light theme by flipping the lightness of the dark theme's neutral (low-chroma)
/// colors while preserving accent/axis hues. A cheap, consistent derivation that keeps the
/// two themes structurally identical, then layers the light `editor.*` tokens on top.
pub fn create_light_theme() -> ThemeProps {
    use bevy_color::Oklcha;
    let mut theme = create_dark_theme();
    for color in theme.color.values_mut() {
        let c = Oklcha::from(*color);
        let lightness = if c.chroma < 0.045 {
            // Neutral (grays/text/window): invert lightness into the light range.
            (1.0 - c.lightness).clamp(0.0, 1.0)
        } else {
            // Accent / axis: keep the hue, nudge darker so it reads on a light background.
            (c.lightness * 0.92).clamp(0.0, 1.0)
        };
        *color = Oklcha::new(lightness, c.chroma, c.hue, c.alpha).into();
    }
    install_editor_tokens(&mut theme, true);
    theme
}

/// Insert (or overwrite) the `editor.*` tokens into `theme`. `light` selects neutrals that
/// read well on a light vs. dark base; the accent/level colors are shared.
pub fn install_editor_tokens(theme: &mut ThemeProps, light: bool) {
    let c = &mut theme.color;
    // Selection / hover row tints derive from the accent so they read as "active".
    c.insert(etokens::ROW_SELECTED, palette::ACCENT.with_alpha(0.30));
    c.insert(etokens::ROW_HOVER, palette::ACCENT.with_alpha(0.12));
    // Level colors (shared across themes).
    c.insert(etokens::SUCCESS, accent::SUCCESS);
    c.insert(etokens::WARNING, accent::WARNING);
    c.insert(etokens::ERROR, accent::ERROR);
    c.insert(etokens::INFO, palette::ACCENT);

    c.insert(
        etokens::SCRIM,
        palette::BLACK.with_alpha(if light { 0.35 } else { 0.5 }),
    );

    if light {
        c.insert(etokens::TOOLBAR_BG, palette::LIGHT_GRAY_1.darker(0.04));
        c.insert(etokens::STATUS_BAR_BG, palette::ACCENT.darker(0.05));
        c.insert(etokens::STATUS_BAR_TEXT, palette::WHITE.with_alpha(0.9));
        c.insert(etokens::TOAST_BG, palette::WHITE.darker(0.06));
        c.insert(etokens::HUD_BG, palette::WHITE.with_alpha(0.78));
        c.insert(etokens::HUD_TEXT, palette::GRAY_0);
        c.insert(etokens::PANEL_BORDER, palette::GRAY_3.lighter(0.2));
        // Syntax colors tuned to read on a light background (darker, saturated).
        c.insert(etokens::SYNTAX_KEYWORD, Color::oklch(0.45, 0.16, 310.0));
        c.insert(etokens::SYNTAX_TYPE, Color::oklch(0.48, 0.10, 90.0));
        c.insert(etokens::SYNTAX_FUNCTION, Color::oklch(0.45, 0.14, 250.0));
        c.insert(etokens::SYNTAX_MACRO, Color::oklch(0.48, 0.15, 30.0));
        c.insert(etokens::SYNTAX_STRING, Color::oklch(0.42, 0.13, 145.0));
        c.insert(etokens::SYNTAX_NUMBER, Color::oklch(0.45, 0.12, 60.0));
        c.insert(etokens::SYNTAX_COMMENT, Color::oklch(0.55, 0.03, 150.0));
        c.insert(etokens::SYNTAX_PUNCT, Color::oklch(0.40, 0.02, 250.0));
        c.insert(etokens::SYNTAX_NORMAL, Color::oklch(0.25, 0.01, 250.0));
    } else {
        c.insert(etokens::TOOLBAR_BG, palette::GRAY_1.lighter(0.02));
        c.insert(etokens::STATUS_BAR_BG, palette::ACCENT.darker(0.18));
        c.insert(etokens::STATUS_BAR_TEXT, palette::WHITE.with_alpha(0.85));
        c.insert(etokens::TOAST_BG, palette::GRAY_2.lighter(0.03));
        c.insert(etokens::HUD_BG, palette::GRAY_0.with_alpha(0.72));
        c.insert(etokens::HUD_TEXT, palette::LIGHT_GRAY_1);
        c.insert(etokens::PANEL_BORDER, palette::WARM_GRAY_1);
        // Syntax colors tuned to read on a dark background (brighter).
        c.insert(etokens::SYNTAX_KEYWORD, Color::oklch(0.74, 0.13, 310.0));
        c.insert(etokens::SYNTAX_TYPE, Color::oklch(0.82, 0.10, 90.0));
        c.insert(etokens::SYNTAX_FUNCTION, Color::oklch(0.78, 0.11, 250.0));
        c.insert(etokens::SYNTAX_MACRO, Color::oklch(0.78, 0.12, 30.0));
        c.insert(etokens::SYNTAX_STRING, Color::oklch(0.80, 0.13, 145.0));
        c.insert(etokens::SYNTAX_NUMBER, Color::oklch(0.82, 0.10, 60.0));
        c.insert(etokens::SYNTAX_COMMENT, Color::oklch(0.62, 0.03, 150.0));
        c.insert(etokens::SYNTAX_PUNCT, Color::oklch(0.78, 0.02, 250.0));
        c.insert(etokens::SYNTAX_NORMAL, Color::oklch(0.90, 0.01, 250.0));
    }
}

// ---------------------------------------------------------------------------
// Reusable scene builders
// ---------------------------------------------------------------------------

/// A two-column inspector field row: a fixed-width caption column and the editor widget.
pub fn field_row(label: impl Into<String>, inner: impl Scene) -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(6),
            padding: UiRect::axes(px(6), px(2)),
            min_height: sizes::ROW_H,
        }
        Children [
            (Node { width: sizes::LABEL_COL, flex_shrink: 0.0 } Children [ label_small(label) ]),
            inner,
        ]
    }
}

/// A section header strip (component name, list/map name, etc.) with optional trailing
/// controls (remove buttons, counts) laid out at the right edge.
pub fn section_header(title: impl Into<String>, trailing: impl Scene) -> impl Scene {
    bsn! {
        Node {
            min_height: sizes::ROW_H,
            padding: UiRect::axes(px(6), px(3)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: px(6),
        }
        ThemeBackgroundColor(tokens::SUBPANE_HEADER_BG)
        Children [
            label_small(title),
            trailing,
        ]
    }
}

/// A short labeled icon+text segment for the status bar / HUD. The whole segment inherits
/// `text_token` so the icon tints to match the label.
pub fn status_segment(icon_path: &'static str, text: impl Into<String>) -> impl Scene {
    use bevy_feathers::{display::icon, theme::InheritableThemeTextColor};
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(4),
            padding: UiRect::horizontal(px(8)),
        }
        InheritableThemeTextColor(etokens::STATUS_BAR_TEXT)
        Children [
            (icon(icon_path) ThemedText),
            label_small(text),
        ]
    }
}

/// A modal dialog title bar: a header strip with the dialog title.
fn dialog_title_bar(title: impl Into<String>) -> impl Scene {
    bsn! {
        Node {
            min_height: px(30),
            padding: UiRect::horizontal(px(12)),
            align_items: AlignItems::Center,
            border: UiRect::bottom(px(1)),
        }
        ThemeBackgroundColor(tokens::DIALOG_HEADER_BG)
        ThemeBorderColor(etokens::PANEL_BORDER)
        Children [ label(title) ]
    }
}

/// A reusable modal dialog: a dimming full-screen scrim (click outside to dismiss) hosting a
/// centered, bordered, shadowed, rounded panel with a title bar and the given `body`. The
/// caller is responsible only for the body's widgets/markers; dismissal is wired to the
/// shared [`crate::ui::CloseOverlay`] / Escape path.
pub fn dialog_frame(title: impl Into<String>, width: Val, body: impl Scene) -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        EditorEntity
        EditorOverlay
        GlobalZIndex(z::DIALOG)
        ThemeBackgroundColor(etokens::SCRIM)
        on(|_: On<Pointer<Click>>, mut c: Commands| { c.trigger(CloseOverlay); })
        Children [
            (
                Node {
                    width: width,
                    max_height: Val::Percent(82.0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(8)),
                    overflow: Overflow::clip(),
                }
                EditorEntity
                GlobalZIndex(z::DIALOG)
                ThemeBackgroundColor(tokens::DIALOG_BG)
                ThemeBorderColor(tokens::DIALOG_BORDER)
                BoxShadow::new(Srgba::BLACK.with_alpha(0.5).into(), px(0), px(8), px(2), px(24))
                on(stop_click)
                Children [
                    dialog_title_bar(title),
                    (
                        Node {
                            display: Display::Flex,
                            flex_direction: FlexDirection::Column,
                            padding: px(14),
                            row_gap: px(10),
                            overflow: Overflow::scroll_y(),
                        }
                        Children [ body ]
                    ),
                ]
            ),
        ]
    }
}
