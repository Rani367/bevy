//! A lightweight debugger / profiler: the **Stats** bottom-dock tab showing live performance and
//! world metrics (FPS, frame time, entity counts, selection), plus single-frame **stepping** of
//! game logic while paused. Metrics are computed directly from `Time` + ECS queries, so no extra
//! diagnostics plugins are required.

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::constants::{fonts, size};
use bevy_feathers::controls::{ButtonVariant, FeathersButton};
use bevy_feathers::theme::{ThemeTextColor, ThemedText};
use bevy_feathers::tokens;
use bevy_scene::prelude::*;
use bevy_state::state::{NextState, State};
use bevy_text::{FontSourceTemplate, FontWeight, TextFont};
use bevy_time::Time;
use bevy_ui::widget::Text;
use bevy_ui::{px, AlignItems, Display, FlexDirection, Node, Overflow, UiRect};
use bevy_ui_widgets::{Activate, ScrollArea};

use crate::markers::SceneEntity;
use crate::state::{EditorSelection, EditorState};

/// Which live stat a row displays.
#[derive(Component, Clone, Copy, PartialEq, Eq, Default)]
enum StatField {
    #[default]
    Fps,
    FrameMs,
    Entities,
    SceneEntities,
    Selection,
    RunState,
}

/// Smoothed FPS estimate for the stats panel.
#[derive(Resource, Default)]
struct StatsFps(f32);

/// Present (as a resource) while a single-frame step is in flight.
#[derive(Resource)]
struct StepPending;

/// Step game logic forward one frame while paused.
#[derive(Event, Clone, Copy)]
pub struct StepFrameRequest;

/// Installs the stats/debugger panel.
pub struct DiagnosticsPlugin;

impl Plugin for DiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StatsFps>()
            .add_systems(Update, (update_stats, finish_step))
            .add_observer(on_step);
    }
}

/// The Stats tab body: a header with a Step button, then a column of live metric rows.
pub fn stats_body() -> impl Scene {
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
        bevy_feathers::theme::ThemeBackgroundColor(tokens::PANE_BODY_BG)
        ScrollArea
        Children [
            (
                Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(8), padding: UiRect::bottom(px(4)) }
                Children [
                    (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Step Frame") ThemedText } }
                        on(|_: On<Activate>, mut c: Commands| { c.trigger(StepFrameRequest); })),
                    (stat_text("", StatField::RunState)),
                ]
            ),
            (stat_text("FPS: --", StatField::Fps)),
            (stat_text("Frame: -- ms", StatField::FrameMs)),
            (stat_text("Entities: --", StatField::Entities)),
            (stat_text("Scene entities: --", StatField::SceneEntities)),
            (stat_text("Selected: --", StatField::Selection)),
        ]
    }
}

fn stat_text(initial: &str, field: StatField) -> impl Scene {
    let text = initial.to_string();
    bsn! {
        (
            Text(text)
            TextFont {
                font: FontSourceTemplate::Handle(fonts::MONO),
                font_size: size::SMALL_FONT,
                weight: FontWeight::NORMAL,
            }
            bevy_app::PropagateOver<TextFont>
            ThemeTextColor(tokens::TEXT_MAIN)
            template_value(field)
        )
    }
}

fn update_stats(
    time: Res<Time>,
    mut fps: ResMut<StatsFps>,
    selection: Res<EditorSelection>,
    state: Res<State<EditorState>>,
    all: Query<Entity>,
    scene_entities: Query<(), With<SceneEntity>>,
    mut rows: Query<(&mut Text, &StatField)>,
) {
    let dt = time.delta_secs();
    if dt > 0.0 {
        let inst = 1.0 / dt;
        fps.0 = if fps.0 <= 0.0 {
            inst
        } else {
            fps.0 * 0.9 + inst * 0.1
        };
    }
    let entity_count = all.iter().count();
    let scene_count = scene_entities.iter().count();
    for (mut text, field) in rows.iter_mut() {
        let value = match field {
            StatField::Fps => format!("FPS: {:.0}", fps.0),
            StatField::FrameMs => format!("Frame: {:.2} ms", dt * 1000.0),
            StatField::Entities => format!("Entities: {entity_count}"),
            StatField::SceneEntities => format!("Scene entities: {scene_count}"),
            StatField::Selection => format!("Selected: {}", selection.all.len()),
            StatField::RunState => format!("{:?}", state.get()),
        };
        if text.0 != value {
            text.0 = value;
        }
    }
}

fn on_step(
    _: On<StepFrameRequest>,
    state: Res<State<EditorState>>,
    mut next: ResMut<NextState<EditorState>>,
    mut commands: Commands,
) {
    if *state.get() == EditorState::Paused {
        next.set(EditorState::Playing);
        commands.insert_resource(StepPending);
    }
}

/// One frame after a step entered `Playing`, return to `Paused`.
fn finish_step(
    pending: Option<Res<StepPending>>,
    state: Res<State<EditorState>>,
    mut next: ResMut<NextState<EditorState>>,
    mut commands: Commands,
) {
    if pending.is_some() && *state.get() == EditorState::Playing {
        next.set(EditorState::Paused);
        commands.remove_resource::<StepPending>();
    }
}
