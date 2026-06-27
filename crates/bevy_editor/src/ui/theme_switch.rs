//! Light/dark theme switching. The View menu (and the command palette) trigger
//! [`crate::ui::ToggleTheme`]; this swaps the whole [`UiTheme`] `ThemeProps`, which the
//! Feathers `update_theme` system then re-resolves across every themed entity.

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
use bevy_feathers::theme::UiTheme;

use crate::ui::style::{create_editor_theme, create_light_theme};
use crate::ui::ToggleTheme;

/// Which editor theme is active.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum EditorThemeMode {
    /// The default dark theme.
    #[default]
    Dark,
    /// The light theme.
    Light,
}

fn on_toggle_theme(
    _: On<ToggleTheme>,
    mut mode: ResMut<EditorThemeMode>,
    mut theme: ResMut<UiTheme>,
) {
    *mode = match *mode {
        EditorThemeMode::Dark => EditorThemeMode::Light,
        EditorThemeMode::Light => EditorThemeMode::Dark,
    };
    theme.0 = match *mode {
        EditorThemeMode::Dark => create_editor_theme(),
        EditorThemeMode::Light => create_light_theme(),
    };
}

/// Installs the theme toggle.
pub struct ThemeSwitchPlugin;

impl Plugin for ThemeSwitchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorThemeMode>()
            .add_observer(on_toggle_theme);
    }
}
