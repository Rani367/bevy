//! The **Animation** bottom-dock tab: a small but real keyframe **timeline editor**. Rather than
//! depending on `bevy_animation`'s clip machinery (which targets released Bevy), this is a
//! dependency-free, in-tree animation system — an [`EditedAnimation`] component holding per-channel
//! keyframe tracks over the entity's `Transform`. It serializes with the scene like the other
//! first-party gameplay components.
//!
//! Workflow: select an entity → **Add Animation** → scrub the playhead, pose the object (gizmo /
//! inspector), **Add Key** to capture the pose at that time → **Play** to preview. Channels are the
//! nine Transform components (position XYZ, euler rotation XYZ in degrees, scale XYZ); a channel is
//! only driven if it has keys.

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton, FeathersSlider};
use bevy_feathers::display::{label_dim, label_small};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy_feathers::tokens;
use bevy_math::{EulerRot, Quat};
use bevy_picking::Pickable;
use bevy_reflect::std_traits::ReflectDefault;
use bevy_reflect::Reflect;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_time::Time;
use bevy_transform::components::Transform;
use bevy_ui::widget::Text;
use bevy_ui::{
    percent, px, AlignItems, BorderRadius, Display, FlexDirection, FlexWrap, Node, Overflow,
    PositionType, UiRect,
};
use bevy_ui_widgets::{Activate, ScrollArea, SliderValue, ValueChange};

use crate::state::EditorSelection;
use crate::ui::style::etokens;
use crate::ui::{BottomDock, BottomTab};

// ---------------------------------------------------------------------------
// Data model (reflected → serializes with the scene)
// ---------------------------------------------------------------------------

/// One animatable channel = one `Transform` component value.
#[derive(Reflect, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AnimChannel {
    /// Translation X.
    #[default]
    PosX,
    /// Translation Y.
    PosY,
    /// Translation Z.
    PosZ,
    /// Euler rotation about X (degrees).
    RotX,
    /// Euler rotation about Y (degrees).
    RotY,
    /// Euler rotation about Z (degrees).
    RotZ,
    /// Scale X.
    ScaleX,
    /// Scale Y.
    ScaleY,
    /// Scale Z.
    ScaleZ,
}

impl AnimChannel {
    /// Every channel, in display order.
    pub const ALL: [AnimChannel; 9] = [
        AnimChannel::PosX,
        AnimChannel::PosY,
        AnimChannel::PosZ,
        AnimChannel::RotX,
        AnimChannel::RotY,
        AnimChannel::RotZ,
        AnimChannel::ScaleX,
        AnimChannel::ScaleY,
        AnimChannel::ScaleZ,
    ];

    /// Short lane label.
    pub fn label(self) -> &'static str {
        match self {
            AnimChannel::PosX => "Pos X",
            AnimChannel::PosY => "Pos Y",
            AnimChannel::PosZ => "Pos Z",
            AnimChannel::RotX => "Rot X",
            AnimChannel::RotY => "Rot Y",
            AnimChannel::RotZ => "Rot Z",
            AnimChannel::ScaleX => "Scl X",
            AnimChannel::ScaleY => "Scl Y",
            AnimChannel::ScaleZ => "Scl Z",
        }
    }
}

/// A single keyframe: a value at a time (seconds).
#[derive(Reflect, Clone, Copy, Debug, Default)]
pub struct Keyframe {
    /// Time in seconds.
    pub time: f32,
    /// Channel value at `time`.
    pub value: f32,
}

/// One channel's keyframes, kept sorted by time.
#[derive(Reflect, Clone, Debug, Default)]
pub struct AnimTrack {
    /// The channel this track drives.
    pub channel: AnimChannel,
    /// Keyframes, sorted ascending by time.
    pub keys: Vec<Keyframe>,
}

/// An editor-authored keyframe animation over the entity's `Transform`.
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component, Default)]
pub struct EditedAnimation {
    /// Loop length in seconds.
    pub duration: f32,
    /// Current playhead time.
    pub time: f32,
    /// Whether the preview is advancing.
    pub playing: bool,
    /// Loop at the end vs. stop.
    pub looping: bool,
    /// Rest pose: channels without keys fall back to this.
    pub base: Transform,
    /// Per-channel keyframe tracks.
    pub tracks: Vec<AnimTrack>,
}

impl Default for EditedAnimation {
    fn default() -> Self {
        Self {
            duration: 2.0,
            time: 0.0,
            playing: false,
            looping: true,
            base: Transform::IDENTITY,
            tracks: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Sampling / pure helpers (unit-tested)
// ---------------------------------------------------------------------------

/// Linearly sample a sorted keyframe list at `t` (clamped to the end values).
fn sample_track(keys: &[Keyframe], t: f32) -> Option<f32> {
    match keys.first() {
        None => None,
        Some(first) if t <= first.time => Some(first.value),
        Some(_) => {
            let last = keys.last().unwrap();
            if t >= last.time {
                return Some(last.value);
            }
            for w in keys.windows(2) {
                let (a, b) = (w[0], w[1]);
                if t >= a.time && t <= b.time {
                    let span = (b.time - a.time).max(1e-6);
                    let f = (t - a.time) / span;
                    return Some(a.value + (b.value - a.value) * f);
                }
            }
            Some(last.value)
        }
    }
}

/// Read a channel's value from a transform (rotation channels in degrees).
fn channel_value(t: &Transform, ch: AnimChannel) -> f32 {
    let (ex, ey, ez) = t.rotation.to_euler(EulerRot::XYZ);
    match ch {
        AnimChannel::PosX => t.translation.x,
        AnimChannel::PosY => t.translation.y,
        AnimChannel::PosZ => t.translation.z,
        AnimChannel::RotX => ex.to_degrees(),
        AnimChannel::RotY => ey.to_degrees(),
        AnimChannel::RotZ => ez.to_degrees(),
        AnimChannel::ScaleX => t.scale.x,
        AnimChannel::ScaleY => t.scale.y,
        AnimChannel::ScaleZ => t.scale.z,
    }
}

/// Compose the transform at the animation's current time from its base pose + keyed channels.
fn apply_animation(anim: &EditedAnimation) -> Transform {
    let mut t = anim.base;
    let (mut ex, mut ey, mut ez) = anim.base.rotation.to_euler(EulerRot::XYZ);
    let mut rot_touched = false;
    for track in &anim.tracks {
        let Some(v) = sample_track(&track.keys, anim.time) else {
            continue;
        };
        match track.channel {
            AnimChannel::PosX => t.translation.x = v,
            AnimChannel::PosY => t.translation.y = v,
            AnimChannel::PosZ => t.translation.z = v,
            AnimChannel::RotX => {
                ex = v.to_radians();
                rot_touched = true;
            }
            AnimChannel::RotY => {
                ey = v.to_radians();
                rot_touched = true;
            }
            AnimChannel::RotZ => {
                ez = v.to_radians();
                rot_touched = true;
            }
            AnimChannel::ScaleX => t.scale.x = v,
            AnimChannel::ScaleY => t.scale.y = v,
            AnimChannel::ScaleZ => t.scale.z = v,
        }
    }
    if rot_touched {
        t.rotation = Quat::from_euler(EulerRot::XYZ, ex, ey, ez);
    }
    t
}

/// Insert (or replace, within an epsilon) a keyframe for `channel` at `time`, keeping the track
/// sorted.
fn insert_key(tracks: &mut Vec<AnimTrack>, channel: AnimChannel, time: f32, value: f32) {
    const EPS: f32 = 1e-3;
    let track = match tracks.iter_mut().position(|t| t.channel == channel) {
        Some(i) => &mut tracks[i],
        None => {
            tracks.push(AnimTrack {
                channel,
                keys: Vec::new(),
            });
            tracks.last_mut().unwrap()
        }
    };
    if let Some(k) = track.keys.iter_mut().find(|k| (k.time - time).abs() < EPS) {
        k.value = value;
    } else {
        track.keys.push(Keyframe { time, value });
        track.keys.sort_by(|a, b| {
            a.time
                .partial_cmp(&b.time)
                .unwrap_or(core::cmp::Ordering::Equal)
        });
    }
}

/// Remove every keyframe near `time` (and any track left empty).
fn remove_keys_at(tracks: &mut Vec<AnimTrack>, time: f32) {
    const EPS: f32 = 2e-2;
    for track in tracks.iter_mut() {
        track.keys.retain(|k| (k.time - time).abs() >= EPS);
    }
    tracks.retain(|t| !t.keys.is_empty());
}

// ---------------------------------------------------------------------------
// UI markers
// ---------------------------------------------------------------------------

/// The rebuildable panel container.
#[derive(Component, Default, Clone, Copy)]
struct AnimPanelContent;
/// The scrub slider.
#[derive(Component, Default, Clone, Copy)]
struct AnimScrub;
/// The live time readout label.
#[derive(Component, Default, Clone, Copy)]
struct AnimTimeLabel;
/// A control button carrying its action.
#[derive(Component, Default, Clone, Copy)]
struct AnimCtl(AnimAction);
/// A timeline lane's track strip; populated with keyframe diamonds.
#[derive(Component, Default, Clone)]
struct AnimLane {
    times: Vec<f32>,
    duration: f32,
}

/// What an animation control button does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum AnimAction {
    #[default]
    AddAnimation,
    PlayPause,
    Stop,
    AddKey,
    RemoveKey,
    DurInc,
    DurDec,
}

/// Caches the last-rendered structural signature so the panel only rebuilds on structural changes
/// (not every frame as the playhead advances).
#[derive(Resource, Default)]
struct AnimPanelSig(u64);

// ---------------------------------------------------------------------------
// Scene
// ---------------------------------------------------------------------------

/// The Animation tab body: a scroll container the timeline is rebuilt into.
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
        ScrollArea
        AnimPanelContent
    }
}

fn ctl_btn(caption: &'static str, action: AnimAction, variant: ButtonVariant) -> impl Scene {
    let cap = caption.to_string();
    bsn! {
        (@FeathersButton { @variant: variant, @caption: bsn! { (Text(cap) ThemedText) } }
            AnimCtl(action))
    }
}

fn controls_row(playing: bool) -> impl Scene {
    let play = if playing { "Pause" } else { "Play" };
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(6),
            row_gap: px(4),
            flex_wrap: FlexWrap::Wrap,
        }
        Children [
            ctl_btn(play, AnimAction::PlayPause, ButtonVariant::Primary),
            ctl_btn("Stop", AnimAction::Stop, ButtonVariant::Normal),
            ctl_btn("Add Key", AnimAction::AddKey, ButtonVariant::Normal),
            ctl_btn("Remove Key", AnimAction::RemoveKey, ButtonVariant::Normal),
            ctl_btn("Dur −", AnimAction::DurDec, ButtonVariant::Normal),
            ctl_btn("Dur +", AnimAction::DurInc, ButtonVariant::Normal),
        ]
    }
}

fn ruler_row(time: f32, duration: f32) -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(6),
        }
        Children [
            (Node { width: px(56), flex_shrink: 0.0 } Children [ (label_small("Time")) ]),
            (@FeathersSlider { @value: time, @min: 0.0, @max: duration }
                template_value(AnimScrub)
                Node { flex_grow: 1.0 }),
        ]
    }
}

fn lane(track: &AnimTrack, duration: f32) -> impl Scene {
    let times: Vec<f32> = track.keys.iter().map(|k| k.time).collect();
    let lane_comp = AnimLane { times, duration };
    let name = track.channel.label().to_string();
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(6),
            min_height: px(18),
        }
        Children [
            (Node { width: px(56), flex_shrink: 0.0 } Children [ (label_small(name)) ]),
            (
                Node {
                    flex_grow: 1.0,
                    height: px(16),
                    position_type: PositionType::Relative,
                    border_radius: BorderRadius::all(px(3)),
                }
                ThemeBackgroundColor(tokens::PANE_HEADER_BG)
                template_value(lane_comp)
            ),
        ]
    }
}

/// One keyframe diamond, positioned by time percent within its lane strip.
fn diamond(pct: f32) -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: percent(pct),
            top: px(4),
            width: px(8),
            height: px(8),
            border_radius: BorderRadius::all(px(2)),
        }
        ThemeBackgroundColor(etokens::INFO)
        Pickable::IGNORE
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Advance + apply each playing animation's pose.
fn advance_animation(time: Res<Time>, mut q: Query<(&mut EditedAnimation, &mut Transform)>) {
    let dt = time.delta_secs();
    for (mut anim, mut tr) in q.iter_mut() {
        if !anim.playing {
            continue;
        }
        let dur = anim.duration.max(1e-3);
        anim.time += dt;
        if anim.time >= dur {
            if anim.looping {
                anim.time %= dur;
            } else {
                anim.time = dur;
                anim.playing = false;
            }
        }
        if !anim.tracks.is_empty() {
            *tr = apply_animation(&anim);
        }
    }
}

/// A cheap structural signature: changes when the selection, track/key counts, play state, or
/// duration change — but not as the playhead time advances.
fn anim_sig(visible: bool, selection: &EditorSelection, anims: &Query<&EditedAnimation>) -> u64 {
    let mut s = visible as u64;
    if let Some(e) = selection.primary {
        s = s.wrapping_mul(1_000_003).wrapping_add(e.to_bits());
        if let Ok(a) = anims.get(e) {
            let keys: usize = a.tracks.iter().map(|t| t.keys.len()).sum();
            s = s.wrapping_mul(1_000_003).wrapping_add(1);
            s = s
                .wrapping_mul(1_000_003)
                .wrapping_add(a.tracks.len() as u64);
            s = s.wrapping_mul(1_000_003).wrapping_add(keys as u64);
            s = s.wrapping_mul(1_000_003).wrapping_add(a.playing as u64);
            s = s
                .wrapping_mul(1_000_003)
                .wrapping_add((a.duration * 10.0) as u64);
        }
    }
    s
}

/// Rebuild the timeline when its structure changes.
fn rebuild_anim_panel(
    dock: Res<BottomDock>,
    selection: Res<EditorSelection>,
    anims: Query<&EditedAnimation>,
    container: Query<Entity, With<AnimPanelContent>>,
    mut sig: ResMut<AnimPanelSig>,
    mut commands: Commands,
) {
    let visible = dock.open && dock.active == BottomTab::Animation;
    let cur = anim_sig(visible, &selection, &anims);
    if cur == sig.0 {
        return;
    }
    sig.0 = cur;
    let Ok(content) = container.single() else {
        return;
    };

    let mut rows: Vec<Box<dyn SceneList>> = Vec::new();
    match selection.primary.and_then(|e| anims.get(e).ok()) {
        None => {
            rows.push(Box::new(EntityScene(ctl_btn(
                "Add Animation",
                AnimAction::AddAnimation,
                ButtonVariant::Primary,
            ))));
            rows.push(Box::new(EntityScene(bsn! {
                Node { padding: UiRect::axes(px(2), px(4)) }
                Children [ (label_dim("Add a keyframe animation track to the selected entity.".to_string())) ]
            })));
        }
        Some(anim) => {
            rows.push(Box::new(EntityScene(controls_row(anim.playing))));
            rows.push(Box::new(EntityScene(ruler_row(anim.time, anim.duration))));
            rows.push(Box::new(EntityScene(bsn! {
                Node { padding: UiRect::axes(px(2), px(2)) }
                Children [ (label_dim(String::new()) AnimTimeLabel) ]
            })));
            if anim.tracks.iter().all(|t| t.keys.is_empty()) {
                rows.push(Box::new(EntityScene(bsn! {
                    Node { padding: UiRect::axes(px(2), px(4)) }
                    Children [ (label_dim("No keyframes yet — pose the object, then Add Key.".to_string())) ]
                })));
            } else {
                for track in &anim.tracks {
                    if !track.keys.is_empty() {
                        rows.push(Box::new(EntityScene(lane(track, anim.duration))));
                    }
                }
            }
        }
    }

    commands.entity(content).despawn_children();
    commands
        .entity(content)
        .queue_spawn_related_scenes::<Children>(rows);
}

/// Fill a freshly-built lane strip with its keyframe diamonds.
fn populate_lane_diamonds(
    lanes: Query<(Entity, &AnimLane), Added<AnimLane>>,
    mut commands: Commands,
) {
    for (entity, lane) in lanes.iter() {
        let dur = lane.duration.max(1e-3);
        let diamonds: Vec<Box<dyn SceneList>> = lane
            .times
            .iter()
            .map(|&t| {
                let pct = (t / dur).clamp(0.0, 1.0) * 100.0;
                Box::new(EntityScene(diamond(pct))) as Box<dyn SceneList>
            })
            .collect();
        commands
            .entity(entity)
            .queue_spawn_related_scenes::<Children>(diamonds);
    }
}

/// Update the live time readout each frame while the tab is open.
fn update_anim_readout(
    dock: Res<BottomDock>,
    selection: Res<EditorSelection>,
    anims: Query<&EditedAnimation>,
    mut labels: Query<&mut Text, With<AnimTimeLabel>>,
) {
    if !(dock.open && dock.active == BottomTab::Animation) {
        return;
    }
    let text = match selection.primary.and_then(|e| anims.get(e).ok()) {
        Some(a) => format!("t = {:.2}s  /  {:.2}s", a.time, a.duration),
        None => String::new(),
    };
    for mut label in labels.iter_mut() {
        if label.0 != text {
            label.0 = text.clone();
        }
    }
}

/// Keep the scrub slider's handle in step with the playhead while playing (re-seeding the
/// immutable `SliderValue`).
fn sync_scrub_slider(
    dock: Res<BottomDock>,
    selection: Res<EditorSelection>,
    anims: Query<&EditedAnimation>,
    sliders: Query<(Entity, &SliderValue), With<AnimScrub>>,
    mut commands: Commands,
) {
    if !(dock.open && dock.active == BottomTab::Animation) {
        return;
    }
    let Some(anim) = selection.primary.and_then(|e| anims.get(e).ok()) else {
        return;
    };
    for (entity, value) in sliders.iter() {
        if (value.0 - anim.time).abs() > 1e-3 {
            commands.entity(entity).insert(SliderValue(anim.time));
        }
    }
}

/// Drag the scrub slider → set the playhead + preview the pose.
fn on_anim_scrub(
    change: On<ValueChange<f32>>,
    scrubs: Query<(), With<AnimScrub>>,
    selection: Res<EditorSelection>,
    mut anims: Query<&mut EditedAnimation>,
    mut transforms: Query<&mut Transform>,
) {
    if !scrubs.contains(change.source) {
        return;
    }
    let Some(e) = selection.primary else {
        return;
    };
    let Ok(mut anim) = anims.get_mut(e) else {
        return;
    };
    anim.time = change.value.clamp(0.0, anim.duration);
    if !anim.tracks.is_empty() {
        let posed = apply_animation(&anim);
        if let Ok(mut tr) = transforms.get_mut(e) {
            *tr = posed;
        }
    }
}

/// Handle a control button.
fn on_anim_ctl(
    act: On<Activate>,
    ctls: Query<&AnimCtl>,
    selection: Res<EditorSelection>,
    mut anims: Query<&mut EditedAnimation>,
    mut transforms: Query<&mut Transform>,
    mut commands: Commands,
) {
    let Ok(ctl) = ctls.get(act.entity) else {
        return;
    };
    let Some(e) = selection.primary else {
        return;
    };
    match ctl.0 {
        AnimAction::AddAnimation => {
            if anims.get(e).is_err() {
                let base = transforms.get(e).copied().unwrap_or_default();
                commands.entity(e).insert(EditedAnimation {
                    base,
                    ..Default::default()
                });
            }
        }
        AnimAction::PlayPause => {
            if let Ok(mut anim) = anims.get_mut(e) {
                anim.playing = !anim.playing;
            }
        }
        AnimAction::Stop => {
            if let Ok(mut anim) = anims.get_mut(e) {
                anim.playing = false;
                anim.time = 0.0;
                let base = anim.base;
                if let Ok(mut tr) = transforms.get_mut(e) {
                    *tr = base;
                }
            }
        }
        AnimAction::AddKey => {
            let cur = transforms.get(e).copied().unwrap_or_default();
            if let Ok(mut anim) = anims.get_mut(e) {
                let t = anim.time;
                for ch in AnimChannel::ALL {
                    let v = channel_value(&cur, ch);
                    insert_key(&mut anim.tracks, ch, t, v);
                }
            }
        }
        AnimAction::RemoveKey => {
            if let Ok(mut anim) = anims.get_mut(e) {
                let t = anim.time;
                remove_keys_at(&mut anim.tracks, t);
            }
        }
        AnimAction::DurInc => {
            if let Ok(mut anim) = anims.get_mut(e) {
                anim.duration = (anim.duration + 0.5).min(60.0);
            }
        }
        AnimAction::DurDec => {
            if let Ok(mut anim) = anims.get_mut(e) {
                anim.duration = (anim.duration - 0.5).max(0.5);
            }
        }
    }
}

/// Installs the keyframe animation editor.
pub struct AnimationEditorPlugin;

impl Plugin for AnimationEditorPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<EditedAnimation>()
            .register_type::<AnimTrack>()
            .register_type::<AnimChannel>()
            .register_type::<Keyframe>()
            .init_resource::<AnimPanelSig>()
            .add_systems(
                Update,
                (
                    advance_animation,
                    rebuild_anim_panel,
                    populate_lane_diamonds,
                    update_anim_readout,
                    sync_scrub_slider,
                ),
            )
            .add_observer(on_anim_ctl)
            .add_observer(on_anim_scrub);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(time: f32, value: f32) -> Keyframe {
        Keyframe { time, value }
    }

    #[test]
    fn sample_clamps_and_interpolates() {
        let keys = vec![k(0.0, 0.0), k(1.0, 10.0)];
        assert_eq!(sample_track(&keys, -1.0), Some(0.0), "before first clamps");
        assert_eq!(sample_track(&keys, 2.0), Some(10.0), "after last clamps");
        assert_eq!(sample_track(&keys, 0.5), Some(5.0), "midpoint interpolates");
        assert_eq!(sample_track(&[], 0.5), None, "empty has no value");
    }

    #[test]
    fn insert_replaces_within_epsilon_and_sorts() {
        let mut tracks = Vec::new();
        insert_key(&mut tracks, AnimChannel::PosX, 1.0, 5.0);
        insert_key(&mut tracks, AnimChannel::PosX, 0.0, 1.0);
        assert_eq!(tracks.len(), 1, "same channel reuses its track");
        assert_eq!(tracks[0].keys.len(), 2);
        assert_eq!(tracks[0].keys[0].time, 0.0, "kept sorted by time");
        // Re-key at ~1.0 replaces rather than appends.
        insert_key(&mut tracks, AnimChannel::PosX, 1.0001, 9.0);
        assert_eq!(tracks[0].keys.len(), 2, "near-duplicate time replaced");
        assert_eq!(tracks[0].keys[1].value, 9.0);
    }

    #[test]
    fn remove_keys_drops_empty_tracks() {
        let mut tracks = vec![AnimTrack {
            channel: AnimChannel::PosY,
            keys: vec![k(0.5, 1.0)],
        }];
        remove_keys_at(&mut tracks, 0.5);
        assert!(
            tracks.is_empty(),
            "track with only the removed key is dropped"
        );
    }

    #[test]
    fn apply_drives_keyed_channels_only() {
        let mut anim = EditedAnimation {
            base: Transform::from_xyz(0.0, 7.0, 0.0),
            duration: 1.0,
            time: 0.5,
            ..Default::default()
        };
        insert_key(&mut anim.tracks, AnimChannel::PosX, 0.0, 0.0);
        insert_key(&mut anim.tracks, AnimChannel::PosX, 1.0, 4.0);
        let t = apply_animation(&anim);
        assert_eq!(t.translation.x, 2.0, "keyed X interpolates");
        assert_eq!(t.translation.y, 7.0, "unkeyed Y keeps the base pose");
    }
}
