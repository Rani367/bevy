//! The **Localization** bottom-dock tab: a string-table editor. Each row is a translation key
//! with one value per locale (locales are columns); the table is saved to
//! `assets/localization.ron` for the game to load. Add Row / Add Locale grow the table, and
//! every cell is an editable text field.

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::controls::{
    ButtonVariant, FeathersButton, FeathersTextInput, FeathersTextInputContainer,
};
use bevy_feathers::display::{icon, label_dim, label_small};
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

/// A translation key and its per-locale values (aligned to [`LocalizationTable::locales`]).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LocEntry {
    /// The lookup key (e.g. `menu.play`).
    pub key: String,
    /// One translated string per locale, in the same order as `locales`.
    pub values: Vec<String>,
}

/// The editable localization string table, persisted to `<project>/assets/localization.ron`.
#[derive(Resource, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LocalizationTable {
    /// Locale codes (column headers), e.g. `["en", "fr"]`.
    pub locales: Vec<String>,
    /// The translation rows.
    pub entries: Vec<LocEntry>,
}

impl Default for LocalizationTable {
    fn default() -> Self {
        Self {
            locales: vec!["en".to_string()],
            entries: vec![
                LocEntry {
                    key: "menu.play".to_string(),
                    values: vec!["Play".to_string()],
                },
                LocEntry {
                    key: "menu.quit".to_string(),
                    values: vec!["Quit".to_string()],
                },
            ],
        }
    }
}

impl LocalizationTable {
    /// Ensure every row has exactly one value per locale (pad/truncate as needed).
    fn normalize(&mut self) {
        let n = self.locales.len();
        for entry in &mut self.entries {
            entry.values.resize(n, String::new());
        }
    }
}

/// Set when the rows should be rebuilt (after add/remove/load).
#[derive(Resource)]
struct LocDirty(bool);

/// The scrollable container the rows are spawned into.
#[derive(Component, Default, Clone, Copy)]
struct LocList;
/// A locale-code header input for column `0`-based index.
#[derive(Component, Default, Clone, Copy)]
struct LocLocaleInput(usize);
/// A row's key input.
#[derive(Component, Default, Clone, Copy)]
struct LocKeyInput(usize);
/// A row's value input for a given locale column.
#[derive(Component, Default, Clone, Copy)]
struct LocValueInput {
    row: usize,
    col: usize,
}
/// A row's remove (×) button.
#[derive(Component, Default, Clone, Copy)]
struct LocRemoveButton(usize);
/// The "Add Row" button.
#[derive(Component, Default, Clone, Copy)]
struct LocAddRow;
/// The "Add Locale" button.
#[derive(Component, Default, Clone, Copy)]
struct LocAddLocale;
/// The "Save" button.
#[derive(Component, Default, Clone, Copy)]
struct LocSave;

/// Installs the localization editor tab.
pub struct LocalizationEditorPlugin;

impl Plugin for LocalizationEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalizationTable>()
            .insert_resource(LocDirty(true))
            .add_systems(
                Update,
                (
                    load_localization_on_project_change,
                    rebuild_loc_list,
                    commit_loc_inputs,
                ),
            )
            .add_observer(on_loc_add_row)
            .add_observer(on_loc_add_locale)
            .add_observer(on_loc_remove)
            .add_observer(on_loc_save);
    }
}

/// The Localization tab body: a locale header, a scrollable row list, and Add/Save controls.
pub fn localization_body() -> impl Scene {
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
            (label_dim("Localization string table — saved to assets/localization.ron")),
            (Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(1) } LocList),
            (
                Node { flex_direction: FlexDirection::Row, column_gap: px(6), padding: UiRect::axes(px(2), px(6)) }
                Children [
                    (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("+ Add Row") ThemedText } }
                        LocAddRow),
                    (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("+ Add Locale") ThemedText } }
                        LocAddLocale),
                    (@FeathersButton { @variant: ButtonVariant::Primary, @caption: bsn! { Text("Save") ThemedText } }
                        LocSave),
                ]
            ),
        ]
    }
}

fn locale_header(locales: &[String]) -> impl Scene {
    let cols: Vec<Box<dyn SceneList>> = locales
        .iter()
        .enumerate()
        .map(|(i, code)| {
            let code = code.clone();
            Box::new(EntityScene(bsn! {
                (@FeathersTextInputContainer Node { width: px(120) } Children [
                    (@FeathersTextInput SeedText(code) LocLocaleInput(i))
                ])
            })) as Box<dyn SceneList>
        })
        .collect();
    bsn! {
        (
            Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(6), padding: UiRect::axes(px(2), px(2)) }
            Children [
                (Node { width: px(140) } Children [ (label_small("key")) ]),
                (Node { flex_direction: FlexDirection::Row, column_gap: px(6) } Children [ {cols} ]),
            ]
        )
    }
}

fn loc_row(idx: usize, entry: &LocEntry) -> impl Scene {
    let key = entry.key.clone();
    let values: Vec<Box<dyn SceneList>> = entry
        .values
        .iter()
        .enumerate()
        .map(|(col, v)| {
            let v = v.clone();
            Box::new(EntityScene(bsn! {
                (@FeathersTextInputContainer Node { width: px(120) } Children [
                    (@FeathersTextInput SeedText(v) LocValueInput { row: idx, col: col })
                ])
            })) as Box<dyn SceneList>
        })
        .collect();
    bsn! {
        (
            Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(6), padding: UiRect::axes(px(2), px(2)) }
            Children [
                (@FeathersTextInputContainer Node { width: px(140) } Children [
                    (@FeathersTextInput SeedText(key) LocKeyInput(idx))
                ]),
                (Node { flex_direction: FlexDirection::Row, column_gap: px(6) } Children [ {values} ]),
                (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { (icon(icons::X)) } }
                    LocRemoveButton(idx)),
            ]
        )
    }
}

fn rebuild_loc_list(
    mut dirty: ResMut<LocDirty>,
    mut table: ResMut<LocalizationTable>,
    list_q: Query<Entity, With<LocList>>,
    mut commands: Commands,
) {
    if !dirty.0 {
        return;
    }
    let Ok(list) = list_q.single() else {
        return;
    };
    dirty.0 = false;
    table.normalize();
    let mut rows: Vec<Box<dyn SceneList>> =
        vec![Box::new(EntityScene(locale_header(&table.locales)))];
    if table.entries.is_empty() {
        rows.push(Box::new(EntityScene(label_dim(
            "No strings — click Add Row",
        ))));
    } else {
        for (i, entry) in table.entries.iter().enumerate() {
            rows.push(Box::new(EntityScene(loc_row(i, entry))));
        }
    }
    commands.entity(list).despawn_children();
    commands
        .entity(list)
        .queue_spawn_related_scenes::<Children>(rows);
}

/// Write key/value/locale edits back into the table without rebuilding (preserving focus).
fn commit_loc_inputs(
    keys: Query<(&LocKeyInput, &EditableText), Changed<EditableText>>,
    values: Query<(&LocValueInput, &EditableText), Changed<EditableText>>,
    locales: Query<(&LocLocaleInput, &EditableText), Changed<EditableText>>,
    mut table: ResMut<LocalizationTable>,
) {
    for (input, text) in keys.iter() {
        if let Some(entry) = table.entries.get_mut(input.0) {
            entry.key = text.value().to_string();
        }
    }
    for (input, text) in values.iter() {
        let value = text.value().to_string();
        if let Some(entry) = table.entries.get_mut(input.row)
            && let Some(slot) = entry.values.get_mut(input.col)
        {
            *slot = value;
        }
    }
    for (input, text) in locales.iter() {
        if let Some(code) = table.locales.get_mut(input.0) {
            *code = text.value().to_string();
        }
    }
}

fn on_loc_add_row(
    act: On<Activate>,
    buttons: Query<(), With<LocAddRow>>,
    mut table: ResMut<LocalizationTable>,
    mut dirty: ResMut<LocDirty>,
) {
    if !buttons.contains(act.entity) {
        return;
    }
    let n = table.locales.len();
    table.entries.push(LocEntry {
        key: String::new(),
        values: vec![String::new(); n],
    });
    dirty.0 = true;
}

fn on_loc_add_locale(
    act: On<Activate>,
    buttons: Query<(), With<LocAddLocale>>,
    mut table: ResMut<LocalizationTable>,
    mut dirty: ResMut<LocDirty>,
) {
    if !buttons.contains(act.entity) {
        return;
    }
    let n = table.locales.len();
    table.locales.push(format!("locale_{n}"));
    table.normalize();
    dirty.0 = true;
}

fn on_loc_remove(
    act: On<Activate>,
    buttons: Query<&LocRemoveButton>,
    mut table: ResMut<LocalizationTable>,
    mut dirty: ResMut<LocDirty>,
) {
    if let Ok(button) = buttons.get(act.entity)
        && button.0 < table.entries.len()
    {
        table.entries.remove(button.0);
        dirty.0 = true;
    }
}

fn on_loc_save(
    act: On<Activate>,
    buttons: Query<(), With<LocSave>>,
    table: Res<LocalizationTable>,
    project: Res<ActiveProject>,
    mut commands: Commands,
) {
    if !buttons.contains(act.entity) {
        return;
    }
    match save_table(&table, &project) {
        Ok(path) => commands.trigger(ShowToast::success(format!("Saved {path}"))),
        Err(err) => commands.trigger(ShowToast::error(format!("Localization save failed: {err}"))),
    }
}

fn save_table(table: &LocalizationTable, project: &ActiveProject) -> Result<String, String> {
    let dir = project.assets_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("localization.ron");
    let text = ron::ser::to_string_pretty(table, ron::ser::PrettyConfig::default())
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok("assets/localization.ron".to_string())
}

fn load_localization_on_project_change(
    project: Res<ActiveProject>,
    mut table: ResMut<LocalizationTable>,
    mut dirty: ResMut<LocDirty>,
) {
    if !project.is_changed() {
        return;
    }
    let path = project.assets_dir().join("localization.ron");
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(loaded) = ron::from_str::<LocalizationTable>(&text)
    {
        *table = loaded;
    }
    dirty.0 = true;
}

#[cfg(test)]
mod tests {
    use super::{LocEntry, LocalizationTable};

    #[test]
    fn normalize_pads_and_truncates_values() {
        let mut table = LocalizationTable {
            locales: vec!["en".into(), "fr".into()],
            entries: vec![
                LocEntry {
                    key: "a".into(),
                    values: vec!["A".into()],
                },
                LocEntry {
                    key: "b".into(),
                    values: vec!["B".into(), "Bf".into(), "extra".into()],
                },
            ],
        };
        table.normalize();
        assert_eq!(
            table.entries[0].values,
            vec!["A".to_string(), String::new()]
        );
        assert_eq!(
            table.entries[1].values,
            vec!["B".to_string(), "Bf".to_string()]
        );
    }

    #[test]
    fn localization_ron_round_trips() {
        let table = LocalizationTable::default();
        let text = ron::ser::to_string(&table).unwrap();
        let back: LocalizationTable = ron::from_str(&text).unwrap();
        assert_eq!(table, back);
    }
}
