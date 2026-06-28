//! A tabbed dock along the bottom of the editor — the home for the Console, build Output, and
//! (in later phases) the animation timeline and debugger. It replaces the old single-purpose
//! console strip: feature plugins fill a [`BottomTabContent`] node, and the dock shows exactly
//! the active tab. The open/active state is a serializable "workspace" persisted to the
//! editor-global data dir so the layout survives restarts.

use bevy_app::{App, Plugin, Startup, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton};
use bevy_feathers::display::icon;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemedText};
use bevy_feathers::tokens;
use bevy_picking::Pickable;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_ui::widget::Text;
use bevy_ui::{px, AlignItems, Display, FlexDirection, Node, UiRect};
use bevy_ui_widgets::Activate;
use serde::{Deserialize, Serialize};

use crate::markers::EditorEntity;
use crate::project::editor_data_dir;
use crate::ui::style::{etokens, sizes, space};
use crate::ui::{icons, ToggleConsole};

/// A tab in the bottom dock. Add a variant + a matching content node (tagged
/// [`BottomTabContent`]) to add a new bottom panel.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum BottomTab {
    /// The log console.
    #[default]
    Console,
    /// Build / cargo output and diagnostics.
    Output,
    /// Performance + debugger stats.
    Stats,
    /// Animation timeline for the selection.
    Animation,
    /// Material editor for the selection.
    Material,
    /// Audio master-volume mixer.
    Audio,
    /// Game-UI theme/design-token editor.
    Theme,
    /// Localization string-table editor.
    Localization,
    /// Tilemap palette + grid editor.
    Tilemap,
    /// UI / canvas layout editor.
    Ui,
}

impl BottomTab {
    /// All tabs, in display order.
    pub const ALL: [BottomTab; 10] = [
        BottomTab::Console,
        BottomTab::Output,
        BottomTab::Stats,
        BottomTab::Animation,
        BottomTab::Material,
        BottomTab::Audio,
        BottomTab::Theme,
        BottomTab::Localization,
        BottomTab::Tilemap,
        BottomTab::Ui,
    ];

    /// Human-readable tab title.
    pub fn title(self) -> &'static str {
        match self {
            BottomTab::Console => "Console",
            BottomTab::Output => "Output",
            BottomTab::Stats => "Stats",
            BottomTab::Animation => "Animation",
            BottomTab::Material => "Material",
            BottomTab::Audio => "Audio",
            BottomTab::Theme => "Theme",
            BottomTab::Localization => "Localization",
            BottomTab::Tilemap => "Tilemap",
            BottomTab::Ui => "UI",
        }
    }

    /// Leading tab icon.
    pub fn icon(self) -> &'static str {
        match self {
            BottomTab::Console => icons::TERMINAL,
            BottomTab::Output => icons::BUILD,
            BottomTab::Stats => icons::SLIDERS,
            BottomTab::Animation => icons::PLAY_MODE,
            BottomTab::Material => icons::SPHERE,
            BottomTab::Audio => icons::PLAY,
            BottomTab::Theme => icons::SUN,
            BottomTab::Localization => icons::LIST,
            BottomTab::Tilemap => icons::GRID,
            BottomTab::Ui => icons::SQUARE,
        }
    }

    /// Stable key used when persisting the layout.
    fn key(self) -> &'static str {
        match self {
            BottomTab::Console => "console",
            BottomTab::Output => "output",
            BottomTab::Stats => "stats",
            BottomTab::Animation => "animation",
            BottomTab::Material => "material",
            BottomTab::Audio => "audio",
            BottomTab::Theme => "theme",
            BottomTab::Localization => "localization",
            BottomTab::Tilemap => "tilemap",
            BottomTab::Ui => "ui",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        BottomTab::ALL.into_iter().find(|t| t.key() == key)
    }
}

/// Open/active state of the bottom dock (a minimal serializable workspace).
#[derive(Resource)]
pub struct BottomDock {
    /// Whether the dock is visible.
    pub open: bool,
    /// Which tab's content is shown.
    pub active: BottomTab,
}

impl Default for BottomDock {
    fn default() -> Self {
        Self {
            open: false,
            active: BottomTab::Console,
        }
    }
}

/// Marks the dock's root node (shown/hidden as the dock opens/closes).
#[derive(Component, Default, Clone, Copy)]
struct BottomDockRoot;
/// Marks the tab-strip container (rebuilt when the active tab changes).
#[derive(Component, Default, Clone, Copy)]
struct BottomTabStrip;
/// Marks a tab's content node; the dock shows only the active tab's content.
#[derive(Component, Clone, Copy)]
pub struct BottomTabContent(pub BottomTab);
impl Default for BottomTabContent {
    fn default() -> Self {
        Self(BottomTab::Console)
    }
}
/// A tab button in the strip; clicking activates that tab.
#[derive(Component, Clone, Copy)]
struct BottomTabButton(BottomTab);
impl Default for BottomTabButton {
    fn default() -> Self {
        Self(BottomTab::Console)
    }
}
/// The dock's close (×) button.
#[derive(Component, Default, Clone, Copy)]
struct BottomDockClose;

/// Switch the bottom dock to `tab` (opening it if needed).
#[derive(Event, Clone, Copy)]
pub struct ShowBottomTab(pub BottomTab);

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default)]
struct SavedLayout {
    bottom_open: bool,
    bottom_active: String,
}

fn layout_path() -> Option<std::path::PathBuf> {
    editor_data_dir().map(|d| d.join("layout.ron"))
}

fn save_layout(dock: &BottomDock) {
    let Some(path) = layout_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let saved = SavedLayout {
        bottom_open: dock.open,
        bottom_active: dock.active.key().to_string(),
    };
    if let Ok(text) = ron::ser::to_string_pretty(&saved, ron::ser::PrettyConfig::default()) {
        let _ = std::fs::write(&path, text);
    }
}

fn load_layout(mut dock: ResMut<BottomDock>) {
    let Some(path) = layout_path() else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    if let Ok(saved) = ron::from_str::<SavedLayout>(&text) {
        dock.open = saved.bottom_open;
        if let Some(tab) = BottomTab::from_key(&saved.bottom_active) {
            dock.active = tab;
        }
    }
}

// ---------------------------------------------------------------------------
// Scene
// ---------------------------------------------------------------------------

/// The bottom dock shell: a tab strip header + a body that hosts each tab's content node.
/// Placed in the editor shell above the status bar. Feature plugins fill the content nodes
/// (the console plugin fills the Console body; build/export fills the Output body).
pub fn bottom_dock_panel() -> impl Scene {
    bsn! {
        Node {
            display: Display::None,
            flex_direction: FlexDirection::Column,
            height: px(200),
            border: UiRect::top(px(1)),
        }
        EditorEntity
        BottomDockRoot
        ThemeBorderColor(etokens::PANEL_BORDER)
        Children [
            (
                Node {
                    min_height: sizes::PANEL_HEADER_H,
                    padding: UiRect::horizontal(px(6)),
                    align_items: AlignItems::Center,
                    column_gap: space::XS,
                    border: UiRect::bottom(px(1)),
                }
                ThemeBackgroundColor(tokens::PANE_HEADER_BG)
                ThemeBorderColor(etokens::PANEL_BORDER)
                Children [
                    (Node { flex_grow: 1.0, flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: space::XS } BottomTabStrip),
                    (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { (icon(icons::CLOSE) ThemedText) } }
                        BottomDockClose),
                ]
            ),
            (
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                }
                Children [
                    (crate::ui::console::console_body() BottomTabContent(BottomTab::Console)),
                    (output_body() BottomTabContent(BottomTab::Output)),
                    (crate::diagnostics::stats_body() BottomTabContent(BottomTab::Stats)),
                    (crate::animation::animation_body() BottomTabContent(BottomTab::Animation)),
                    (crate::material::material_body() BottomTabContent(BottomTab::Material)),
                    (crate::audio::audio_body() BottomTabContent(BottomTab::Audio)),
                    (crate::theme_editor::theme_editor_body() BottomTabContent(BottomTab::Theme)),
                    (crate::localization::localization_body() BottomTabContent(BottomTab::Localization)),
                    (crate::tilemap::tilemap_body() BottomTabContent(BottomTab::Tilemap)),
                    (crate::ui_edit::ui_body() BottomTabContent(BottomTab::Ui)),
                ]
            ),
        ]
    }
}

/// The Output tab body: a placeholder until the build/cargo integration fills it.
fn output_body() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(8), px(4)),
            row_gap: px(1),
            overflow: bevy_ui::Overflow::scroll_y(),
        }
        ThemeBackgroundColor(tokens::PANE_BODY_BG)
        bevy_ui_widgets::ScrollArea
        OutputContent
        Pickable::IGNORE
    }
}

/// Marks the scrollable container the build/cargo output rows are spawned into.
#[derive(Component, Default, Clone, Copy)]
pub struct OutputContent;

fn tab_button(tab: BottomTab, active: bool) -> impl Scene {
    let variant = if active {
        ButtonVariant::Primary
    } else {
        ButtonVariant::Plain
    };
    let title = tab.title().to_string();
    let icon_path = tab.icon();
    bsn! {
        (@FeathersButton { @variant: variant, @caption: bsn! {
            (Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(6), padding: UiRect::axes(px(4), px(1)) }
                Children [ (icon(icon_path) ThemedText), (Text(title) ThemedText) ])
        } }
            BottomTabButton(tab))
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Reflect [`BottomDock`] onto the dock root + tab content nodes whenever it changes, and
/// persist the new layout.
fn apply_bottom_dock(
    dock: Res<BottomDock>,
    mut roots: Query<&mut Node, (With<BottomDockRoot>, Without<BottomTabContent>)>,
    mut contents: Query<(&BottomTabContent, &mut Node), Without<BottomDockRoot>>,
) {
    if !dock.is_changed() {
        return;
    }
    for mut node in roots.iter_mut() {
        node.display = if dock.open {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (content, mut node) in contents.iter_mut() {
        node.display = if dock.open && content.0 == dock.active {
            Display::Flex
        } else {
            Display::None
        };
    }
    save_layout(&dock);
}

/// Rebuild the tab strip (highlighting the active tab) when the dock changes.
fn rebuild_tab_strip(
    dock: Res<BottomDock>,
    strip: Query<Entity, With<BottomTabStrip>>,
    mut commands: Commands,
) {
    if !dock.is_changed() {
        return;
    }
    let Ok(strip) = strip.single() else {
        return;
    };
    let rows: Vec<Box<dyn SceneList>> = BottomTab::ALL
        .into_iter()
        .map(|t| Box::new(EntityScene(tab_button(t, t == dock.active))) as Box<dyn SceneList>)
        .collect();
    commands.entity(strip).despawn_children();
    commands
        .entity(strip)
        .queue_spawn_related_scenes::<Children>(rows);
}

fn on_show_bottom_tab(show: On<ShowBottomTab>, mut dock: ResMut<BottomDock>) {
    dock.open = true;
    dock.active = show.0;
}

fn on_tab_button(
    act: On<Activate>,
    buttons: Query<&BottomTabButton>,
    mut dock: ResMut<BottomDock>,
) {
    if let Ok(button) = buttons.get(act.entity) {
        dock.active = button.0;
        dock.open = true;
    }
}

fn on_toggle_console(_: On<ToggleConsole>, mut dock: ResMut<BottomDock>) {
    // Backtick / View menu: toggle the dock, focused on the Console tab.
    if dock.open && dock.active == BottomTab::Console {
        dock.open = false;
    } else {
        dock.open = true;
        dock.active = BottomTab::Console;
    }
}

fn on_close_button(
    act: On<Activate>,
    buttons: Query<(), With<BottomDockClose>>,
    mut dock: ResMut<BottomDock>,
) {
    if buttons.contains(act.entity) {
        dock.open = false;
    }
}

/// Installs the bottom dock.
pub struct BottomDockPlugin;

impl Plugin for BottomDockPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BottomDock>()
            .add_systems(Startup, load_layout)
            .add_systems(Update, (apply_bottom_dock, rebuild_tab_strip))
            .add_observer(on_show_bottom_tab)
            .add_observer(on_tab_button)
            .add_observer(on_toggle_console)
            .add_observer(on_close_button);
    }
}
