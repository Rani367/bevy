//! The **Animation** bottom-dock tab: inspect and control the selected entity's
//! [`AnimationPlayer`]. It reports the active-animation count and paused state and offers
//! Pause-All / Resume-All / Restart controls (scrubbing all active clips). A full keyframe
//! timeline is future work; this gives playback control over `bevy_animation` clips from the
//! editor.

use bevy_animation::AnimationPlayer;
use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton};
use bevy_feathers::display::label_dim;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy_feathers::tokens;
use bevy_scene::prelude::*;
use bevy_ui::widget::Text;
use bevy_ui::{px, AlignItems, Display, FlexDirection, Node, Overflow, UiRect};
use bevy_ui_widgets::Activate;

use crate::state::EditorSelection;
use crate::ui::{BottomDock, BottomTab};

/// Pause every active animation on the selected entity.
#[derive(Event, Clone, Copy)]
pub struct AnimPauseAll;
/// Resume every active animation on the selected entity.
#[derive(Event, Clone, Copy)]
pub struct AnimResumeAll;
/// Seek every active animation on the selected entity back to the start.
#[derive(Event, Clone, Copy)]
pub struct AnimRestart;

/// Marks the status label that reports the selection's animation state.
#[derive(Component, Default, Clone, Copy)]
struct AnimStatusLabel;

/// Installs the animation panel.
pub struct AnimationEditorPlugin;

impl Plugin for AnimationEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_animation_status)
            .add_observer(on_pause_all)
            .add_observer(on_resume_all)
            .add_observer(on_restart);
    }
}

/// The Animation tab body: playback controls + a status line.
pub fn animation_body() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(8), px(6)),
            row_gap: px(6),
            overflow: Overflow::scroll_y(),
        }
        ThemeBackgroundColor(tokens::PANE_BODY_BG)
        Children [
            (
                Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(8) }
                Children [
                    (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Pause") ThemedText } }
                        on(|_: On<Activate>, mut c: Commands| { c.trigger(AnimPauseAll); })),
                    (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Resume") ThemedText } }
                        on(|_: On<Activate>, mut c: Commands| { c.trigger(AnimResumeAll); })),
                    (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Restart") ThemedText } }
                        on(|_: On<Activate>, mut c: Commands| { c.trigger(AnimRestart); })),
                ]
            ),
            (label_dim("Select an entity with an AnimationPlayer") AnimStatusLabel),
        ]
    }
}

fn selected_player<'a>(
    selection: &EditorSelection,
    players: &'a mut Query<&mut AnimationPlayer>,
) -> Option<Mut<'a, AnimationPlayer>> {
    let entity = selection.primary?;
    players.get_mut(entity).ok()
}

fn update_animation_status(
    dock: Res<BottomDock>,
    selection: Res<EditorSelection>,
    players: Query<&AnimationPlayer>,
    mut labels: Query<&mut Text, With<AnimStatusLabel>>,
) {
    if !(dock.open && dock.active == BottomTab::Animation) {
        return;
    }
    let status = match selection.primary.and_then(|e| players.get(e).ok()) {
        Some(player) => {
            let count = player.playing_animations().count();
            if count == 0 {
                "AnimationPlayer present — no active clips".to_string()
            } else {
                let paused = if player.all_paused() { " (paused)" } else { "" };
                format!("{count} active clip(s){paused}")
            }
        }
        None => "No AnimationPlayer on the selection".to_string(),
    };
    for mut text in labels.iter_mut() {
        if text.0 != status {
            text.0 = status.clone();
        }
    }
}

fn on_pause_all(
    _: On<AnimPauseAll>,
    selection: Res<EditorSelection>,
    mut players: Query<&mut AnimationPlayer>,
) {
    if let Some(mut player) = selected_player(&selection, &mut players) {
        player.pause_all();
    }
}

fn on_resume_all(
    _: On<AnimResumeAll>,
    selection: Res<EditorSelection>,
    mut players: Query<&mut AnimationPlayer>,
) {
    if let Some(mut player) = selected_player(&selection, &mut players) {
        player.resume_all();
    }
}

fn on_restart(
    _: On<AnimRestart>,
    selection: Res<EditorSelection>,
    mut players: Query<&mut AnimationPlayer>,
) {
    if let Some(mut player) = selected_player(&selection, &mut players) {
        for (_, active) in player.playing_animations_mut() {
            active.seek_to(0.0);
        }
    }
}
