//! Transient toast notifications. Feature code triggers [`ShowToast`] (e.g. on save, build,
//! or a failed edit) and a small colored card slides into the bottom-right corner, then
//! auto-dismisses. This is the editor's primary success/failure feedback channel.

use bevy_app::{App, Plugin, Startup, Update};
use bevy_color::{Alpha, Srgba};
use bevy_ecs::prelude::*;
use bevy_feathers::display::{icon, label};
use bevy_feathers::theme::{
    ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemeToken, ThemedText,
};
use bevy_picking::Pickable;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_time::{Time, Timer, TimerMode};
use bevy_ui::{
    px, AlignItems, BorderRadius, BoxShadow, Display, FlexDirection, GlobalZIndex, Node,
    PositionType, UiRect,
};

use crate::markers::EditorEntity;
use crate::ui::icons;
use crate::ui::style::{etokens, z};

/// Severity of a toast, which selects its icon and accent color.
#[derive(Clone, Copy)]
pub enum ToastLevel {
    /// Neutral information.
    Info,
    /// A successful action (save, build).
    Success,
    /// A recoverable problem.
    Warning,
    /// A failure.
    Error,
}

impl ToastLevel {
    fn icon(self) -> &'static str {
        match self {
            ToastLevel::Info => icons::INFO,
            ToastLevel::Success => icons::SUCCESS,
            ToastLevel::Warning => icons::WARNING,
            ToastLevel::Error => icons::ERROR,
        }
    }
    fn token(self) -> ThemeToken {
        match self {
            ToastLevel::Info => etokens::INFO,
            ToastLevel::Success => etokens::SUCCESS,
            ToastLevel::Warning => etokens::WARNING,
            ToastLevel::Error => etokens::ERROR,
        }
    }
}

/// Show a transient toast notification.
#[derive(Event, Clone)]
pub struct ShowToast {
    /// The message text.
    pub text: String,
    /// Severity (icon + accent color).
    pub level: ToastLevel,
}

impl ShowToast {
    /// An informational toast.
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            level: ToastLevel::Info,
        }
    }
    /// A success toast.
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            level: ToastLevel::Success,
        }
    }
    /// A warning toast.
    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            level: ToastLevel::Warning,
        }
    }
    /// An error toast.
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            level: ToastLevel::Error,
        }
    }
}

/// The bottom-right container that stacks toast cards.
#[derive(Component)]
struct ToastRoot;

/// A live toast card; despawns when its timer elapses.
#[derive(Component, Clone)]
struct Toast {
    timer: Timer,
}

impl Default for Toast {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(4.0, TimerMode::Once),
        }
    }
}

fn spawn_toast_root(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: px(16),
            bottom: px(36),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::End,
            row_gap: px(8),
            max_width: px(380),
            ..Default::default()
        },
        EditorEntity,
        ToastRoot,
        GlobalZIndex(z::TOAST),
        Pickable::IGNORE,
    ));
}

fn toast_card(text: String, level: ToastLevel) -> impl Scene {
    let icon_path = level.icon();
    let border_token = level.token();
    let text_token = level.token();
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
            padding: UiRect::axes(px(12), px(8)),
            border: UiRect::left(px(3)),
            border_radius: {BorderRadius::all(px(6))},
            max_width: px(360),
        }
        EditorEntity
        Toast
        Pickable::IGNORE
        ThemeBackgroundColor(etokens::TOAST_BG)
        ThemeBorderColor(border_token)
        BoxShadow::new(Srgba::BLACK.with_alpha(0.4).into(), px(0), px(3), px(1), px(12))
        Children [
            // The level token resolves to the icon's text color via the theme system, and
            // `ThemedIcon` tints the image to it.
            (icon(icon_path) ThemedText ThemeTextColor(text_token)),
            label(text),
        ]
    }
}

fn on_show_toast(
    show: On<ShowToast>,
    roots: Query<Entity, With<ToastRoot>>,
    mut commands: Commands,
) {
    for root in roots.iter() {
        let card = toast_card(show.text.clone(), show.level);
        commands
            .entity(root)
            .queue_spawn_related_scenes::<Children>(vec![
                Box::new(EntityScene(card)) as Box<dyn SceneList>
            ]);
    }
}

fn expire_toasts(time: Res<Time>, mut toasts: Query<(Entity, &mut Toast)>, mut commands: Commands) {
    for (entity, mut toast) in toasts.iter_mut() {
        if toast.timer.tick(time.delta()).just_finished() {
            commands.entity(entity).despawn();
        }
    }
}

/// Installs the toast notification system.
pub struct ToastPlugin;

impl Plugin for ToastPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_toast_root)
            .add_systems(Update, expire_toasts)
            .add_observer(on_show_toast);
    }
}
