//! Out-of-process (remote) inspection over the Bevy Remote Protocol (BRP).
//!
//! This is an honest minimal scaffold: it connects to a running Bevy app that has
//! `RemotePlugin` + `RemoteHttpPlugin` enabled (default `127.0.0.1:15702`), issues a
//! `world.query` over raw HTTP on a worker thread, and reports how many entities the
//! remote world contains. It is **read-only** — remote component editing and a full remote
//! hierarchy/inspector are future work. The local editing path is unaffected.

use alloc::sync::Arc;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Mutex;
use std::time::Duration;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::controls::{
    ButtonVariant, FeathersButton, FeathersTextInput, FeathersTextInputContainer,
};
use bevy_feathers::display::label;
use bevy_feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy_feathers::tokens;
use bevy_input_focus::AutoFocus;
use bevy_log::{error, info};
use bevy_picking::events::{Click, Pointer};
use bevy_scene::prelude::*;
use bevy_text::EditableText;
use bevy_ui::widget::Text;
use bevy_ui::{
    percent, px, AlignItems, Display, FlexDirection, GlobalZIndex, JustifyContent, Node, Overflow,
    PositionType, UiRect,
};
use bevy_ui_widgets::{Activate, ScrollArea};

use crate::markers::EditorEntity;
use crate::ui::{stop_click, CloseOverlay, EditorOverlay, SeedText};

/// Default BRP endpoint (matches `bevy_remote`'s defaults).
const DEFAULT_REMOTE: &str = "127.0.0.1:15702";

/// Open the "Connect to Remote" dialog.
#[derive(Event, Clone, Copy)]
pub struct OpenConnectDialog;

/// Run a remote entity query against the configured URL.
#[derive(Event, Clone, Copy)]
struct RemoteQuery;

/// Tracks the remote connection and in-flight query.
#[derive(Resource, Default)]
struct RemoteState {
    url: Option<String>,
    result: Arc<Mutex<Option<Result<String, String>>>>,
    querying: bool,
}

/// The URL text input in the connect dialog.
#[derive(Component, Default, Clone, Copy)]
struct ConnectUrlInput;
/// The connect button in the connect dialog.
#[derive(Component, Default, Clone, Copy)]
struct ConnectConfirmButton;

/// Installs the remote-inspection scaffold.
pub struct RemotePlugin;

impl Plugin for RemotePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RemoteState>()
            .add_systems(Update, poll_remote)
            .add_observer(on_open_connect_dialog)
            .add_observer(on_connect_confirm)
            .add_observer(on_remote_query);
    }
}

fn on_open_connect_dialog(_: On<OpenConnectDialog>, mut commands: Commands) {
    commands.spawn_scene(connect_dialog());
}

fn on_connect_confirm(
    act: On<Activate>,
    buttons: Query<(), With<ConnectConfirmButton>>,
    inputs: Query<&EditableText, With<ConnectUrlInput>>,
    mut state: ResMut<RemoteState>,
    mut commands: Commands,
) {
    if !buttons.contains(act.entity) {
        return;
    }
    let Some(url) = inputs.iter().next().map(|e| e.value().to_string()) else {
        return;
    };
    let url = url.trim().to_string();
    if url.is_empty() {
        return;
    }
    state.url = Some(url);
    commands.trigger(RemoteQuery);
    commands.trigger(CloseOverlay);
}

fn on_remote_query(_: On<RemoteQuery>, mut state: ResMut<RemoteState>, mut commands: Commands) {
    if state.querying {
        return;
    }
    let Some(url) = state.url.clone() else {
        return;
    };
    state.querying = true;
    let slot = state.result.clone();
    std::thread::spawn(move || {
        let result = brp_query(&url);
        *slot.lock().unwrap() = Some(result);
    });
    commands.spawn_scene(message_overlay("Querying remote…"));
}

fn poll_remote(mut state: ResMut<RemoteState>, mut commands: Commands) {
    if !state.querying {
        return;
    }
    let taken = state.result.lock().unwrap().take();
    if let Some(result) = taken {
        state.querying = false;
        commands.trigger(CloseOverlay);
        let message = match result {
            Ok(summary) => {
                info!("Remote query ok: {summary}");
                summary
            }
            Err(err) => {
                error!("Remote query failed: {err}");
                format!("Remote query failed:\n{err}")
            }
        };
        commands.spawn_scene(message_overlay(&message));
    }
}

/// Issue a `world.query` over BRP and summarize the result. Blocking; runs on a worker
/// thread.
fn brp_query(url: &str) -> Result<String, String> {
    let host_port = url
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    let addr = if host_port.contains(':') {
        host_port.to_string()
    } else {
        format!("{host_port}:15702")
    };

    let body =
        r#"{"jsonrpc":"2.0","id":1,"method":"world.query","params":{"data":{},"filter":{}}}"#;
    let request = format!(
        "POST / HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );

    let mut stream = TcpStream::connect(&addr).map_err(|e| format!("connect failed: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| e.to_string())?;

    let status_ok = response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200");
    let resp_body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .unwrap_or("");
    if !status_ok || resp_body.contains("\"error\"") {
        let snippet: String = resp_body.chars().take(200).collect();
        return Err(format!("remote responded: {snippet}"));
    }
    let count = resp_body.matches("\"entity\"").count();
    Ok(format!("Connected to {addr}\n{count} entities (read-only)"))
}

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

fn connect_dialog() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        EditorEntity
        EditorOverlay
        GlobalZIndex(2000)
        on(|_: On<Pointer<Click>>, mut c: Commands| { c.trigger(CloseOverlay); })
        Children [
            (
                Node {
                    width: px(360),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: px(10),
                    row_gap: px(8),
                }
                EditorEntity
                ThemeBackgroundColor(tokens::PANE_HEADER_BG)
                GlobalZIndex(2001)
                on(stop_click)
                Children [
                    (Node { padding: UiRect::axes(px(2), px(2)) } Children [ label("Connect to Remote (BRP host:port)") ]),
                    (@FeathersTextInputContainer Children [
                        (@FeathersTextInput SeedText(String::from(DEFAULT_REMOTE)) ConnectUrlInput AutoFocus)
                    ]),
                    (
                        Node { display: Display::Flex, flex_direction: FlexDirection::Row, column_gap: px(8) }
                        Children [
                            (@FeathersButton { @variant: ButtonVariant::Primary, @caption: bsn! { Text("Connect") ThemedText } }
                                ConnectConfirmButton),
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Cancel") ThemedText } }
                                on(|_: On<Activate>, mut c: Commands| { c.trigger(CloseOverlay); })),
                        ]
                    ),
                ]
            ),
        ]
    }
}

/// A centered modal showing a message with a Close button.
fn message_overlay(message: &str) -> impl Scene {
    let message = message.to_string();
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        EditorEntity
        EditorOverlay
        GlobalZIndex(2000)
        on(|_: On<Pointer<Click>>, mut c: Commands| { c.trigger(CloseOverlay); })
        Children [
            (
                Node {
                    width: px(360),
                    max_height: percent(60),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: px(12),
                    row_gap: px(10),
                    overflow: Overflow::scroll_y(),
                }
                EditorEntity
                ScrollArea
                ThemeBackgroundColor(tokens::PANE_HEADER_BG)
                GlobalZIndex(2001)
                on(stop_click)
                Children [
                    (Node { padding: UiRect::axes(px(2), px(2)) } Children [ label(message) ]),
                    (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Close") ThemedText } }
                        on(|_: On<Activate>, mut c: Commands| { c.trigger(CloseOverlay); })),
                ]
            ),
        ]
    }
}
