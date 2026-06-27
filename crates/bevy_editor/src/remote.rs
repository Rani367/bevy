//! Out-of-process (remote) editing over the Bevy Remote Protocol (BRP).
//!
//! Connects to a running Bevy app that has `RemotePlugin` + `RemoteHttpPlugin` enabled
//! (default `127.0.0.1:15702`) and drives it over raw HTTP JSON-RPC on worker threads: it
//! queries the remote world (reporting its entities) and can **edit** it — spawn an entity,
//! despawn one, and mutate a component field (`world.spawn_entity` / `world.despawn_entity` /
//! `world.mutate_components`). The low-level [`brp_request`] helper is public so external
//! tooling can issue any BRP method. The local editing path is unaffected.

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
use bevy_feathers::display::{label, label_dim};
use bevy_feathers::theme::ThemedText;
use bevy_input_focus::AutoFocus;
use bevy_log::{error, info};
use bevy_scene::prelude::*;
use bevy_text::EditableText;
use bevy_ui::widget::Text;
use bevy_ui::{px, Display, FlexDirection, JustifyContent, Node};
use bevy_ui_widgets::Activate;

use crate::ui::style::dialog_frame;
use crate::ui::{CloseOverlay, SeedText};

/// Default BRP endpoint (matches `bevy_remote`'s defaults).
const DEFAULT_REMOTE: &str = "127.0.0.1:15702";

/// Open the "Connect to Remote" dialog.
#[derive(Event, Clone, Copy)]
pub struct OpenConnectDialog;

/// Run a remote entity query against the configured URL.
#[derive(Event, Clone, Copy)]
struct RemoteQuery;

/// Remote edit actions issued from the remote-actions overlay.
#[derive(Event, Clone, Copy)]
enum RemoteEdit {
    /// Spawn a new remote entity.
    Spawn,
    /// Despawn the first queried remote entity.
    DespawnOne,
}

/// A successful query: a human summary plus the queried entity ids.
struct QueryOk {
    summary: String,
    entities: Vec<u64>,
}

/// Tracks the remote connection and in-flight query.
#[derive(Resource, Default)]
struct RemoteState {
    /// The normalized `host:port` of the connected remote.
    addr: Option<String>,
    /// The most recent set of queried remote entity ids.
    entities: Vec<u64>,
    result: Arc<Mutex<Option<Result<QueryOk, String>>>>,
    querying: bool,
}

/// The URL text input in the connect dialog.
#[derive(Component, Default, Clone, Copy)]
struct ConnectUrlInput;
/// The connect button in the connect dialog.
#[derive(Component, Default, Clone, Copy)]
struct ConnectConfirmButton;

/// Installs the remote-editing subsystem.
pub struct RemotePlugin;

impl Plugin for RemotePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RemoteState>()
            .add_systems(Update, poll_remote)
            .add_observer(on_open_connect_dialog)
            .add_observer(on_connect_confirm)
            .add_observer(on_remote_query)
            .add_observer(on_remote_edit);
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
    state.addr = Some(normalize_addr(&url));
    commands.trigger(RemoteQuery);
    commands.trigger(CloseOverlay);
}

fn on_remote_query(_: On<RemoteQuery>, mut state: ResMut<RemoteState>, mut commands: Commands) {
    if state.querying {
        return;
    }
    let Some(addr) = state.addr.clone() else {
        return;
    };
    state.querying = true;
    let slot = state.result.clone();
    std::thread::spawn(move || {
        let result = brp_query_entities(&addr).map(|entities| QueryOk {
            summary: format!("Connected to {addr}\n{} entities", entities.len()),
            entities,
        });
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
        match result {
            Ok(ok) => {
                info!("Remote query ok: {}", ok.summary);
                state.entities = ok.entities;
                commands.spawn_scene(remote_overlay(ok.summary));
            }
            Err(err) => {
                error!("Remote query failed: {err}");
                commands.spawn_scene(message_overlay(&format!("Remote query failed:\n{err}")));
            }
        }
    }
}

/// Apply a remote edit (spawn / despawn), then re-query to refresh the actions overlay.
/// Edits are quick localhost requests, so they run synchronously.
fn on_remote_edit(edit: On<RemoteEdit>, mut state: ResMut<RemoteState>, mut commands: Commands) {
    let Some(addr) = state.addr.clone() else {
        return;
    };
    let applied = match *edit {
        RemoteEdit::Spawn => brp_spawn(&addr).map(|_| ()),
        RemoteEdit::DespawnOne => match state.entities.first().copied() {
            Some(id) => brp_despawn(&addr, id).map(|_| ()),
            None => Err("no remote entities to despawn".to_string()),
        },
    };
    commands.trigger(CloseOverlay);
    match applied.and_then(|()| brp_query_entities(&addr)) {
        Ok(entities) => {
            let summary = format!("Connected to {addr}\n{} entities", entities.len());
            state.entities = entities;
            commands.spawn_scene(remote_overlay(summary));
        }
        Err(err) => {
            error!("Remote edit failed: {err}");
            commands.spawn_scene(message_overlay(&format!("Remote edit failed:\n{err}")));
        }
    }
}

/// Normalize a user-entered `host[:port]` / URL into a `host:port` address.
pub fn normalize_addr(url: &str) -> String {
    let host_port = url
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    if host_port.contains(':') {
        host_port.to_string()
    } else {
        format!(
            "{host_port}:{}",
            DEFAULT_REMOTE.rsplit(':').next().unwrap_or("15702")
        )
    }
}

/// Issue a single BRP JSON-RPC request (`method` with `params` JSON) over raw HTTP and return
/// the JSON-RPC response body. Blocking; intended for a worker thread. Errors on transport
/// failure, a non-200 status, or a JSON-RPC `error` member.
pub fn brp_request(addr: &str, method: &str, params: &str) -> Result<String, String> {
    let body = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{params}}}"#);
    let request = format!(
        "POST / HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );

    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect failed: {e}"))?;
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
        .unwrap_or("")
        .to_string();
    if !status_ok || resp_body.contains("\"error\"") {
        let snippet: String = resp_body.chars().take(300).collect();
        return Err(format!("remote responded: {snippet}"));
    }
    Ok(resp_body)
}

/// Query all entities (`world.query`) and return their numeric ids.
pub fn brp_query_entities(addr: &str) -> Result<Vec<u64>, String> {
    let body = brp_request(addr, "world.query", r#"{"data":{},"filter":{}}"#)?;
    Ok(parse_entity_ids(&body))
}

/// Spawn a remote entity carrying a `Transform` and a `Name` (`world.spawn_entity`).
pub fn brp_spawn(addr: &str) -> Result<String, String> {
    let params = r#"{"components":{
        "bevy_transform::components::transform::Transform":{"translation":[0.0,0.0,0.0],"rotation":[0.0,0.0,0.0,1.0],"scale":[1.0,1.0,1.0]},
        "bevy_ecs::name::Name":"Remote Entity"
    }}"#;
    brp_request(addr, "world.spawn_entity", params)
}

/// Despawn a remote entity (`world.despawn_entity`).
pub fn brp_despawn(addr: &str, entity: u64) -> Result<String, String> {
    brp_request(
        addr,
        "world.despawn_entity",
        &format!(r#"{{"entity":{entity}}}"#),
    )
}

/// Mutate a field of a remote entity's component (`world.mutate_components`).
pub fn brp_mutate(
    addr: &str,
    entity: u64,
    component: &str,
    path: &str,
    value_json: &str,
) -> Result<String, String> {
    let params = format!(
        r#"{{"entity":{entity},"component":"{component}","path":"{path}","value":{value_json}}}"#
    );
    brp_request(addr, "world.mutate_components", &params)
}

/// Crude extraction of numeric `"entity": <id>` values from a BRP response body.
pub fn parse_entity_ids(body: &str) -> Vec<u64> {
    const KEY: &str = "\"entity\":";
    let mut ids = Vec::new();
    let mut rest = body;
    while let Some(idx) = rest.find(KEY) {
        rest = &rest[idx + KEY.len()..];
        let digits: String = rest
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(id) = digits.parse::<u64>() {
            ids.push(id);
        }
    }
    ids
}

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

fn connect_dialog() -> impl Scene {
    dialog_frame(
        "Connect to Remote",
        px(420),
        bsn! {
            (
                Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(8) }
                Children [
                    (label_dim("Bevy Remote Protocol host:port")),
                    (@FeathersTextInputContainer Children [
                        (@FeathersTextInput SeedText(String::from(DEFAULT_REMOTE)) ConnectUrlInput AutoFocus)
                    ]),
                    (
                        Node { display: Display::Flex, flex_direction: FlexDirection::Row, justify_content: JustifyContent::End, column_gap: px(8) }
                        Children [
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Cancel") ThemedText } }
                                on(|_: On<Activate>, mut c: Commands| { c.trigger(CloseOverlay); })),
                            (@FeathersButton { @variant: ButtonVariant::Primary, @caption: bsn! { Text("Connect") ThemedText } }
                                ConnectConfirmButton),
                        ]
                    ),
                ]
            )
        },
    )
}

/// The remote-actions overlay: the connection summary plus spawn / despawn / refresh edits.
fn remote_overlay(message: String) -> impl Scene {
    dialog_frame(
        "Remote Editing",
        px(420),
        bsn! {
            (
                Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(10) }
                Children [
                    (label(message)),
                    (
                        Node { display: Display::Flex, flex_direction: FlexDirection::Row, column_gap: px(6), flex_wrap: bevy_ui::FlexWrap::Wrap, row_gap: px(6) }
                        Children [
                            (@FeathersButton { @variant: ButtonVariant::Primary, @caption: bsn! { Text("Spawn") ThemedText } }
                                on(|_: On<Activate>, mut c: Commands| { c.trigger(RemoteEdit::Spawn); })),
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Despawn one") ThemedText } }
                                on(|_: On<Activate>, mut c: Commands| { c.trigger(RemoteEdit::DespawnOne); })),
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Refresh") ThemedText } }
                                on(|_: On<Activate>, mut c: Commands| { c.trigger(RemoteQuery); })),
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Close") ThemedText } }
                                on(|_: On<Activate>, mut c: Commands| { c.trigger(CloseOverlay); })),
                        ]
                    ),
                ]
            )
        },
    )
}

/// A centered modal showing a message with a Close button.
fn message_overlay(message: &str) -> impl Scene {
    let message = message.to_string();
    dialog_frame(
        "Remote",
        px(380),
        bsn! {
            (
                Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(10) }
                Children [
                    (label(message)),
                    (
                        Node { display: Display::Flex, flex_direction: FlexDirection::Row, justify_content: JustifyContent::End }
                        Children [
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Close") ThemedText } }
                                on(|_: On<Activate>, mut c: Commands| { c.trigger(CloseOverlay); })),
                        ]
                    ),
                ]
            )
        },
    )
}
