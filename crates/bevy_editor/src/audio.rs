//! The **Audio** bottom-dock tab: a master-volume mixer bound to Bevy's [`GlobalVolume`]. A
//! single slider scales all game audio; the value re-syncs from `GlobalVolume` when the tab is
//! shown. (A full per-bus mixer is future work — Bevy ships only a global volume + per-sink
//! volume today.)

use bevy_app::{App, Plugin, Update};
use bevy_audio::{GlobalVolume, Volume};
use bevy_ecs::prelude::*;
use bevy_feathers::controls::FeathersSlider;
use bevy_feathers::display::{label, label_dim};
use bevy_scene::prelude::*;
use bevy_ui::{px, AlignItems, Display, FlexDirection, Node, Overflow, UiRect};
use bevy_ui_widgets::{SliderValue, ValueChange};

use crate::ui::{BottomDock, BottomTab};

/// Marks the master-volume slider.
#[derive(Component, Default, Clone, Copy)]
struct MasterVolumeSlider;

/// Installs the audio mixer panel.
pub struct AudioEditorPlugin;

impl Plugin for AudioEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_master_volume)
            .add_observer(on_volume_change);
    }
}

/// The Audio tab body: a labeled master-volume slider.
pub fn audio_body() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(8), px(6)),
            row_gap: px(4),
            overflow: Overflow::scroll_y(),
        }
        bevy_feathers::theme::ThemeBackgroundColor(bevy_feathers::tokens::PANE_BODY_BG)
        bevy_ui_widgets::ScrollArea
        Children [
            (label_dim("Master volume")),
            (
                Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(8), min_height: px(26) }
                Children [
                    (Node { width: px(80) } Children [ (label("Volume")) ]),
                    (@FeathersSlider { @value: 1.0, @min: 0.0, @max: 1.0 }
                        MasterVolumeSlider
                        Node { flex_grow: 1.0 }),
                ]
            ),
        ]
    }
}

fn on_volume_change(
    change: On<ValueChange<f32>>,
    sliders: Query<(), With<MasterVolumeSlider>>,
    volume: Option<ResMut<GlobalVolume>>,
) {
    if !sliders.contains(change.source) {
        return;
    }
    if let Some(mut volume) = volume {
        volume.volume = Volume::Linear(change.value.clamp(0.0, 1.0));
    }
}

/// Re-sync the slider from `GlobalVolume` when the Audio tab is shown.
fn sync_master_volume(
    dock: Res<BottomDock>,
    volume: Option<Res<GlobalVolume>>,
    sliders: Query<(Entity, &SliderValue), With<MasterVolumeSlider>>,
    mut commands: Commands,
) {
    if !(dock.open && dock.active == BottomTab::Audio && dock.is_changed()) {
        return;
    }
    let Some(volume) = volume else {
        return;
    };
    let want = volume.volume.to_linear();
    for (entity, value) in sliders.iter() {
        if (value.0 - want).abs() > 1e-4 {
            commands.entity(entity).insert(SliderValue(want));
        }
    }
}
