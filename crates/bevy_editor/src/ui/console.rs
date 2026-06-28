//! An in-editor console / log panel. A `tracing` layer (installed via the example's
//! `LogPlugin { custom_layer: editor_console_layer, .. }`) funnels every log record into a
//! shared buffer; a toggleable bottom panel (the `` ` `` key or View menu) renders the recent
//! lines with level coloring and a monospace font, so you rarely need the terminal.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use std::sync::Mutex;

use bevy_app::{App, Plugin, PropagateOver, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::constants::{fonts, size};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeTextColor, ThemeToken};
use bevy_feathers::tokens;
use bevy_log::tracing::field::{Field, Visit};
use bevy_log::tracing::{Event, Level, Subscriber};
use bevy_log::tracing_subscriber::layer::Context;
use bevy_log::tracing_subscriber::registry::LookupSpan;
use bevy_log::tracing_subscriber::Layer;
use bevy_log::BoxedLayer;
use bevy_picking::Pickable;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_text::{FontSourceTemplate, FontWeight, TextFont};
use bevy_ui::widget::Text;
use bevy_ui::{px, Display, FlexDirection, Node, Overflow, UiRect};
use bevy_ui_widgets::ScrollArea;

use crate::ui::bottom_dock::{BottomDock, BottomTab};
use crate::ui::style::etokens;

/// Severity of a captured log line.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ConsoleLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<Level> for ConsoleLevel {
    fn from(level: Level) -> Self {
        match level {
            Level::TRACE => ConsoleLevel::Trace,
            Level::DEBUG => ConsoleLevel::Debug,
            Level::INFO => ConsoleLevel::Info,
            Level::WARN => ConsoleLevel::Warn,
            Level::ERROR => ConsoleLevel::Error,
        }
    }
}

impl ConsoleLevel {
    fn token(self) -> ThemeToken {
        match self {
            ConsoleLevel::Error => etokens::ERROR,
            ConsoleLevel::Warn => etokens::WARNING,
            ConsoleLevel::Info => tokens::TEXT_MAIN,
            ConsoleLevel::Debug | ConsoleLevel::Trace => tokens::TEXT_DIM,
        }
    }
}

/// A captured log record.
#[derive(Clone)]
struct ConsoleLine {
    level: ConsoleLevel,
    target: String,
    message: String,
}

/// Shared sink the `tracing` layer writes to and the UI drains from.
#[derive(Resource, Clone)]
struct ConsoleBuffer(Arc<Mutex<VecDeque<ConsoleLine>>>);

/// Marks the scrollable container the log rows are spawned into.
#[derive(Component, Default, Clone, Copy)]
struct ConsoleContent;

/// Tracks the last-rendered line count so the console only rebuilds when new lines arrive.
#[derive(Resource, Default)]
struct ConsoleState {
    rendered: usize,
}

const MAX_LINES: usize = 1000;
const SHOWN_LINES: usize = 200;

// ---------------------------------------------------------------------------
// tracing layer
// ---------------------------------------------------------------------------

/// A `tracing` layer that appends every event to the shared [`ConsoleBuffer`].
struct ConsoleLayer {
    sink: Arc<Mutex<VecDeque<ConsoleLine>>>,
}

#[derive(Default)]
struct MessageVisitor(String);

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn core::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for ConsoleLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        if let Ok(mut buf) = self.sink.lock() {
            buf.push_back(ConsoleLine {
                level: (*meta.level()).into(),
                target: meta.target().to_string(),
                message: visitor.0,
            });
            while buf.len() > MAX_LINES {
                buf.pop_front();
            }
        }
    }
}

/// The `LogPlugin::custom_layer` hook. Installs the [`ConsoleBuffer`] resource and returns a
/// layer that captures all log records into it. Pass this to `LogPlugin` in your app:
/// `LogPlugin { custom_layer: editor_console_layer, ..default() }`.
pub fn editor_console_layer(app: &mut App) -> Option<BoxedLayer> {
    let sink = Arc::new(Mutex::new(VecDeque::new()));
    app.insert_resource(ConsoleBuffer(sink.clone()));
    Some(Box::new(ConsoleLayer { sink }))
}

// ---------------------------------------------------------------------------
// Panel
// ---------------------------------------------------------------------------

/// The console's scrollable body — the log rows container. Hosted by the bottom dock as its
/// Console tab (the dock owns visibility; this is just the content).
pub fn console_body() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(8), px(4)),
            row_gap: px(1),
            overflow: Overflow::scroll_y(),
        }
        ThemeBackgroundColor(tokens::PANE_BODY_BG)
        ScrollArea
        ConsoleContent
        Pickable::IGNORE
    }
}

fn console_line_row(line: &ConsoleLine) -> impl Scene {
    // Shorten noisy targets to their last path segment.
    let target = line.target.rsplit("::").next().unwrap_or(&line.target);
    let text = format!("{target}  {}", line.message);
    let token = line.level.token();
    bsn! {
        (
            Text(text)
            TextFont {
                font: FontSourceTemplate::Handle(fonts::MONO),
                font_size: size::SMALL_FONT,
                weight: FontWeight::NORMAL,
            }
            PropagateOver<TextFont>
            ThemeTextColor(token)
        )
    }
}

/// Rebuild the console rows when it's visible and new lines have arrived.
fn update_console(
    buffer: Option<Res<ConsoleBuffer>>,
    dock: Res<BottomDock>,
    mut state: ResMut<ConsoleState>,
    content: Query<Entity, With<ConsoleContent>>,
    mut commands: Commands,
) {
    let Some(buffer) = buffer else {
        return;
    };
    if !(dock.open && dock.active == BottomTab::Console) {
        return;
    }
    let Ok(container) = content.single() else {
        return;
    };
    let lines: Vec<ConsoleLine> = {
        let Ok(buf) = buffer.0.lock() else {
            return;
        };
        if buf.len() == state.rendered {
            return;
        }
        state.rendered = buf.len();
        buf.iter().rev().take(SHOWN_LINES).rev().cloned().collect()
    };
    let rows: Vec<Box<dyn SceneList>> = lines
        .iter()
        .map(|l| Box::new(EntityScene(console_line_row(l))) as Box<dyn SceneList>)
        .collect();
    commands.entity(container).despawn_children();
    commands
        .entity(container)
        .queue_spawn_related_scenes::<Children>(rows);
}

/// Installs the console panel and its log capture.
pub struct ConsolePlugin;

impl Plugin for ConsolePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConsoleState>()
            .add_systems(Update, update_console);
    }
}
