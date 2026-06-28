//! The **Theme** bottom-dock tab: a small editor for the *game's* UI design tokens — a list of
//! named colors (background, text, accent, …) the game being built can load from
//! `assets/theme.ron`. Each row has a live swatch, an editable name, and a hex color field;
//! Add/Save manage the list and persist it as RON. (This authors the game's palette; it is
//! independent of the editor's own chrome theme.)

use bevy_app::{App, Plugin, Update};
use bevy_color::Color;
use bevy_ecs::prelude::*;
use bevy_feathers::controls::{
    ButtonVariant, ColorSwatchValue, FeathersButton, FeathersColorSwatch, FeathersTextInput,
    FeathersTextInputContainer,
};
use bevy_feathers::display::{icon, label_dim};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy_feathers::tokens;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_text::EditableText;
use bevy_ui::widget::Text;
use bevy_ui::{px, AlignItems, Display, FlexDirection, Node, Overflow, UiRect};
use bevy_ui_widgets::{Activate, ScrollArea};
use serde::{Deserialize, Serialize};

use crate::project::ActiveProject;
use crate::ui::{icons, SeedText, ShowToast};

/// One named color token in the game theme. `color` is sRGBA in 0..=1.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GameThemeEntry {
    /// Token name (e.g. `primary`, `background`).
    pub name: String,
    /// sRGBA channels, 0..=1.
    pub color: [f32; 4],
}

/// The editable game-UI theme palette, persisted to `<project>/assets/theme.ron`.
#[derive(Resource, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GameTheme {
    /// The named color tokens, in display order.
    pub entries: Vec<GameThemeEntry>,
}

impl Default for GameTheme {
    fn default() -> Self {
        let e = |name: &str, color: [f32; 4]| GameThemeEntry {
            name: name.to_string(),
            color,
        };
        Self {
            entries: vec![
                e("background", [0.10, 0.11, 0.13, 1.0]),
                e("surface", [0.16, 0.17, 0.20, 1.0]),
                e("text", [0.90, 0.92, 0.95, 1.0]),
                e("primary", [0.30, 0.55, 0.95, 1.0]),
                e("accent", [0.95, 0.55, 0.25, 1.0]),
            ],
        }
    }
}

/// Set when the token rows should be rebuilt (after add/remove/load).
#[derive(Resource)]
struct GameThemeDirty(bool);

/// The scrollable container the token rows are spawned into.
#[derive(Component, Default, Clone, Copy)]
struct ThemeTokenList;
/// The live preview swatch for row `0`-based index.
#[derive(Component, Default, Clone, Copy)]
struct ThemeRowSwatch(usize);
/// The name text input for a row.
#[derive(Component, Default, Clone, Copy)]
struct ThemeNameInput(usize);
/// The hex color text input for a row.
#[derive(Component, Default, Clone, Copy)]
struct ThemeHexInput(usize);
/// The remove (×) button for a row.
#[derive(Component, Default, Clone, Copy)]
struct ThemeRemoveButton(usize);
/// The "Add Token" button.
#[derive(Component, Default, Clone, Copy)]
struct ThemeAddButton;
/// The "Save" button.
#[derive(Component, Default, Clone, Copy)]
struct ThemeSaveButton;

/// Installs the game-theme editor tab.
pub struct ThemeEditorPlugin;

impl Plugin for ThemeEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameTheme>()
            .insert_resource(GameThemeDirty(true))
            .add_systems(
                Update,
                (
                    load_game_theme_on_project_change,
                    rebuild_theme_list,
                    commit_theme_inputs,
                ),
            )
            .add_observer(on_theme_add)
            .add_observer(on_theme_remove)
            .add_observer(on_theme_save);
    }
}

/// Format sRGBA channels as `#rrggbbaa`.
fn color_to_hex(c: [f32; 4]) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        b(c[0]),
        b(c[1]),
        b(c[2]),
        b(c[3])
    )
}

/// Parse `#rgb`-style hex (`#rrggbb` or `#rrggbbaa`, leading `#` optional) into sRGBA.
fn parse_hex(s: &str) -> Option<[f32; 4]> {
    let s = s.trim().trim_start_matches('#');
    let ch = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    let (r, g, b, a) = match s.len() {
        6 => (ch(0)?, ch(2)?, ch(4)?, 255),
        8 => (ch(0)?, ch(2)?, ch(4)?, ch(6)?),
        _ => return None,
    };
    Some([
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ])
}

/// The Theme tab body: a scrollable token list plus Add / Save controls.
pub fn theme_editor_body() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(8), px(6)),
            row_gap: px(2),
            overflow: Overflow::scroll_y(),
        }
        ThemeBackgroundColor(tokens::PANE_BODY_BG)
        ScrollArea
        Children [
            (label_dim("Game UI theme tokens — saved to assets/theme.ron")),
            (Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(1) } ThemeTokenList),
            (
                Node { flex_direction: FlexDirection::Row, column_gap: px(6), padding: UiRect::axes(px(2), px(6)) }
                Children [
                    (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("+ Add Token") ThemedText } }
                        ThemeAddButton),
                    (@FeathersButton { @variant: ButtonVariant::Primary, @caption: bsn! { Text("Save") ThemedText } }
                        ThemeSaveButton),
                ]
            ),
        ]
    }
}

fn theme_row(idx: usize, entry: &GameThemeEntry) -> impl Scene {
    let color = Color::srgba(
        entry.color[0],
        entry.color[1],
        entry.color[2],
        entry.color[3],
    );
    let swatch = ColorSwatchValue(color);
    let name = entry.name.clone();
    let hex = color_to_hex(entry.color);
    bsn! {
        (
            Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(6), padding: UiRect::axes(px(2), px(2)) }
            Children [
                (@FeathersColorSwatch template_value(swatch) ThemeRowSwatch(idx) Node { width: px(28), height: px(18) }),
                (@FeathersTextInputContainer Node { width: px(120) } Children [
                    (@FeathersTextInput SeedText(name) ThemeNameInput(idx))
                ]),
                (@FeathersTextInputContainer Node { width: px(100) } Children [
                    (@FeathersTextInput SeedText(hex) ThemeHexInput(idx))
                ]),
                (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { (icon(icons::X)) } }
                    ThemeRemoveButton(idx)),
            ]
        )
    }
}

fn rebuild_theme_list(
    mut dirty: ResMut<GameThemeDirty>,
    theme: Res<GameTheme>,
    list_q: Query<Entity, With<ThemeTokenList>>,
    mut commands: Commands,
) {
    if !dirty.0 {
        return;
    }
    let Ok(list) = list_q.single() else {
        return; // tab not spawned yet
    };
    dirty.0 = false;
    let rows: Vec<Box<dyn SceneList>> = if theme.entries.is_empty() {
        vec![Box::new(EntityScene(label_dim(
            "No tokens — click Add Token",
        )))]
    } else {
        theme
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| Box::new(EntityScene(theme_row(i, e))) as Box<dyn SceneList>)
            .collect()
    };
    commands.entity(list).despawn_children();
    commands
        .entity(list)
        .queue_spawn_related_scenes::<Children>(rows);
}

/// Write name/hex edits back into [`GameTheme`] without rebuilding (preserving focus); a valid
/// hex also updates that row's live swatch in place.
fn commit_theme_inputs(
    names: Query<(&ThemeNameInput, &EditableText), Changed<EditableText>>,
    hexes: Query<(&ThemeHexInput, &EditableText), Changed<EditableText>>,
    mut theme: ResMut<GameTheme>,
    mut swatches: Query<(&ThemeRowSwatch, &mut ColorSwatchValue)>,
) {
    for (input, text) in names.iter() {
        if let Some(entry) = theme.entries.get_mut(input.0) {
            entry.name = text.value().to_string();
        }
    }
    for (input, text) in hexes.iter() {
        if let Some(color) = parse_hex(&text.value().to_string()) {
            if let Some(entry) = theme.entries.get_mut(input.0) {
                entry.color = color;
            }
            let c = Color::srgba(color[0], color[1], color[2], color[3]);
            for (row, mut value) in swatches.iter_mut() {
                if row.0 == input.0 {
                    value.0 = c;
                }
            }
        }
    }
}

fn on_theme_add(
    act: On<Activate>,
    buttons: Query<(), With<ThemeAddButton>>,
    mut theme: ResMut<GameTheme>,
    mut dirty: ResMut<GameThemeDirty>,
) {
    if !buttons.contains(act.entity) {
        return;
    }
    let n = theme.entries.len();
    theme.entries.push(GameThemeEntry {
        name: format!("token_{n}"),
        color: [0.5, 0.5, 0.5, 1.0],
    });
    dirty.0 = true;
}

fn on_theme_remove(
    act: On<Activate>,
    buttons: Query<&ThemeRemoveButton>,
    mut theme: ResMut<GameTheme>,
    mut dirty: ResMut<GameThemeDirty>,
) {
    if let Ok(button) = buttons.get(act.entity)
        && button.0 < theme.entries.len()
    {
        theme.entries.remove(button.0);
        dirty.0 = true;
    }
}

fn on_theme_save(
    act: On<Activate>,
    buttons: Query<(), With<ThemeSaveButton>>,
    theme: Res<GameTheme>,
    project: Res<ActiveProject>,
    mut commands: Commands,
) {
    if !buttons.contains(act.entity) {
        return;
    }
    match save_theme(&theme, &project) {
        Ok(path) => commands.trigger(ShowToast::success(format!("Saved {path}"))),
        Err(err) => commands.trigger(ShowToast::error(format!("Theme save failed: {err}"))),
    }
}

fn save_theme(theme: &GameTheme, project: &ActiveProject) -> Result<String, String> {
    let dir = project.assets_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("theme.ron");
    let text = ron::ser::to_string_pretty(theme, ron::ser::PrettyConfig::default())
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok("assets/theme.ron".to_string())
}

fn load_game_theme_on_project_change(
    project: Res<ActiveProject>,
    mut theme: ResMut<GameTheme>,
    mut dirty: ResMut<GameThemeDirty>,
) {
    if !project.is_changed() {
        return;
    }
    let path = project.assets_dir().join("theme.ron");
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(loaded) = ron::from_str::<GameTheme>(&text)
    {
        *theme = loaded;
    }
    dirty.0 = true;
}

#[cfg(test)]
mod tests {
    use super::{color_to_hex, parse_hex, GameTheme};

    #[test]
    fn hex_round_trips() {
        let c = [0.0, 0.533_333_3, 1.0, 1.0];
        let hex = color_to_hex(c);
        assert_eq!(hex, "#0088ffff");
        let back = parse_hex(&hex).unwrap();
        for (a, b) in c.iter().zip(back.iter()) {
            assert!((a - b).abs() < 0.01, "{a} vs {b}");
        }
    }

    #[test]
    fn parse_hex_accepts_6_and_8_and_rejects_garbage() {
        assert_eq!(parse_hex("#ffffff"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(parse_hex("000000ff"), Some([0.0, 0.0, 0.0, 1.0]));
        assert!(parse_hex("#fff").is_none());
        assert!(parse_hex("nothex!").is_none());
        assert!(parse_hex("").is_none());
    }

    #[test]
    fn game_theme_ron_round_trips() {
        let theme = GameTheme::default();
        let text = ron::ser::to_string(&theme).unwrap();
        let back: GameTheme = ron::from_str(&text).unwrap();
        assert_eq!(theme, back);
    }
}
