//! UI / Canvas authoring. UI nodes spawned into the scene (see [`crate::actions::SpawnKind::UiNode`]
//! / `UiText`) are bound to the **viewport's** scene camera so they preview inside the viewport
//! panel instead of overlapping the editor's own chrome.
//!
//! The **UI** bottom-dock tab is a small Godot-style canvas layout editor for the selected UI
//! node: a 3×3 **anchor-preset** grid (corner pins, edge stretches, centering via auto margins),
//! a Relative/Absolute position toggle, a Fill-Parent button, and quick width/height presets — all
//! editing the node's `bevy_ui::Node` directly, so the viewport preview updates live.

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton};
use bevy_feathers::display::{label_dim, label_small};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy_feathers::tokens;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_ui::widget::Text;
use bevy_ui::{
    px, AlignItems, Display, FlexDirection, JustifyContent, Node, Overflow, PositionType, UiRect,
    UiTargetCamera, Val,
};
use bevy_ui_widgets::{Activate, ScrollArea};

use crate::markers::{GameCamera, SceneEntity};
use crate::state::EditorSelection;
use crate::ui::style::section_header;
use crate::ui::BottomDock;

/// Installs the UI-edit support systems + the canvas layout panel.
pub struct UiEditPlugin;

impl Plugin for UiEditPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (bind_ui_to_viewport, rebuild_ui_panel))
            .add_observer(on_anchor_preset)
            .add_observer(on_ui_postype)
            .add_observer(on_ui_size);
    }
}

/// Ensure every scene UI node (`SceneEntity` + `Node`) targets the current viewport camera, so
/// game UI renders into the viewport image.
///
/// Steady-state cost is near-zero: it only repoints *bound* nodes when the camera entity is
/// rebuilt (e.g. a 2D/3D switch, detected via `Added<GameCamera>`), and otherwise only binds the
/// (usually empty) set of nodes that have no target yet — newly spawned or freshly loaded ones.
fn bind_ui_to_viewport(
    camera: Query<Entity, With<GameCamera>>,
    new_camera: Query<(), Added<GameCamera>>,
    bound_ui: Query<(Entity, &UiTargetCamera), (With<SceneEntity>, With<Node>)>,
    unbound_ui: Query<Entity, (With<SceneEntity>, With<Node>, Without<UiTargetCamera>)>,
    mut commands: Commands,
) {
    let Ok(cam) = camera.single() else {
        return;
    };
    // Camera rebuilt → repoint every already-bound node whose target is now stale.
    if !new_camera.is_empty() {
        for (entity, target) in bound_ui.iter() {
            if target.0 != cam {
                commands.entity(entity).insert(UiTargetCamera(cam));
            }
        }
    }
    // Bind any node that has no target yet (handles spawn-before-camera and scene loads, where
    // `UiTargetCamera` is excluded from the file).
    for entity in unbound_ui.iter() {
        commands.entity(entity).insert(UiTargetCamera(cam));
    }
}

// ---------------------------------------------------------------------------
// Anchor / size logic (pure, unit-tested)
// ---------------------------------------------------------------------------

/// Apply a Godot-style anchor preset to `node`. `col`/`row` are 0=start, 1=center, 2=end. Start
/// pins to the near edge, end to the far edge, center pins both edges to 0 with auto margins
/// (centering a definite-sized node, CSS-style).
fn apply_anchor(node: &mut Node, col: u8, row: u8) {
    node.position_type = PositionType::Absolute;
    let zero = Val::Px(0.0);
    let (left, right, ml, mr) = match col {
        0 => (zero, Val::Auto, zero, zero),
        1 => (zero, zero, Val::Auto, Val::Auto),
        _ => (Val::Auto, zero, zero, zero),
    };
    let (top, bottom, mt, mb) = match row {
        0 => (zero, Val::Auto, zero, zero),
        1 => (zero, zero, Val::Auto, Val::Auto),
        _ => (Val::Auto, zero, zero, zero),
    };
    node.left = left;
    node.right = right;
    node.top = top;
    node.bottom = bottom;
    node.margin = UiRect {
        left: ml,
        right: mr,
        top: mt,
        bottom: mb,
    };
}

/// Stretch `node` to fill its parent (all insets + margins zeroed, absolute).
fn apply_fill(node: &mut Node) {
    node.position_type = PositionType::Absolute;
    let zero = Val::Px(0.0);
    node.left = zero;
    node.right = zero;
    node.top = zero;
    node.bottom = zero;
    node.margin = UiRect::ZERO;
}

/// A quick width/height choice.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
enum SizeChoice {
    /// Content-sized.
    #[default]
    Auto,
    /// Fixed pixels.
    Px(f32),
    /// Percent of parent.
    Pct(f32),
}

impl SizeChoice {
    fn to_val(self) -> Val {
        match self {
            SizeChoice::Auto => Val::Auto,
            SizeChoice::Px(p) => Val::Px(p),
            SizeChoice::Pct(p) => Val::Percent(p),
        }
    }

    fn label(self) -> String {
        match self {
            SizeChoice::Auto => "Auto".to_string(),
            SizeChoice::Px(p) => format!("{p:.0}px"),
            SizeChoice::Pct(p) => format!("{p:.0}%"),
        }
    }
}

// ---------------------------------------------------------------------------
// Panel markers
// ---------------------------------------------------------------------------

/// The rebuildable UI-panel container.
#[derive(Component, Default, Clone, Copy)]
struct UiPanelContent;
/// An anchor-preset button: `col`/`row` cell, or `fill` to stretch.
#[derive(Component, Default, Clone, Copy)]
struct AnchorPreset {
    col: u8,
    row: u8,
    fill: bool,
}
/// A position-type toggle button (`true` = absolute).
#[derive(Component, Default, Clone, Copy)]
struct UiPosType(bool);
/// A size-preset button: `axis` 0 = width, 1 = height.
#[derive(Component, Default, Clone, Copy)]
struct UiSizeBtn {
    axis: u8,
    choice: SizeChoice,
}

// ---------------------------------------------------------------------------
// Scene
// ---------------------------------------------------------------------------

/// The UI tab body: a scroll container the layout controls are rebuilt into.
pub fn ui_body() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(10), px(8)),
            row_gap: px(8),
            overflow: Overflow::scroll_y(),
        }
        ThemeBackgroundColor(tokens::PANE_BODY_BG)
        ScrollArea
        UiPanelContent
    }
}

fn anchor_btn(arrow: &'static str, col: u8, row: u8) -> impl Scene {
    let cap = arrow.to_string();
    let comp = AnchorPreset {
        col,
        row,
        fill: false,
    };
    bsn! {
        (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { (Text(cap) ThemedText) } }
            template_value(comp)
            Node { width: px(34), height: px(28), justify_content: JustifyContent::Center, align_items: AlignItems::Center })
    }
}

fn anchor_grid_row(arrows: [&'static str; 3], row: u8) -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Row, column_gap: px(4) }
        Children [
            (anchor_btn(arrows[0], 0, row)),
            (anchor_btn(arrows[1], 1, row)),
            (anchor_btn(arrows[2], 2, row)),
        ]
    }
}

fn postype_btn(label: &'static str, absolute: bool) -> impl Scene {
    let cap = label.to_string();
    bsn! {
        (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { (Text(cap) ThemedText) } }
            UiPosType(absolute))
    }
}

fn fill_btn() -> impl Scene {
    let comp = AnchorPreset {
        col: 0,
        row: 0,
        fill: true,
    };
    let cap = "Fill Parent".to_string();
    bsn! {
        (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { (Text(cap) ThemedText) } }
            template_value(comp))
    }
}

fn size_btn(axis: u8, choice: SizeChoice) -> impl Scene {
    let cap = choice.label();
    let comp = UiSizeBtn { axis, choice };
    bsn! {
        (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { (Text(cap) ThemedText) } }
            template_value(comp))
    }
}

fn size_row(label: &'static str, axis: u8) -> impl Scene {
    bsn! {
        Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(6) }
        Children [
            (Node { width: px(20), flex_shrink: 0.0 } Children [ (label_small(label)) ]),
            (size_btn(axis, SizeChoice::Auto)),
            (size_btn(axis, SizeChoice::Px(120.0))),
            (size_btn(axis, SizeChoice::Pct(100.0))),
        ]
    }
}

/// Rebuild the UI panel when the selection or active tab changes.
fn rebuild_ui_panel(
    dock: Res<BottomDock>,
    selection: Res<EditorSelection>,
    ui_nodes: Query<(), (With<Node>, With<SceneEntity>)>,
    container: Query<Entity, With<UiPanelContent>>,
    mut commands: Commands,
) {
    if !(selection.is_changed() || dock.is_changed()) {
        return;
    }
    let Ok(content) = container.single() else {
        return;
    };
    let has_ui = selection.primary.is_some_and(|e| ui_nodes.contains(e));

    let mut rows: Vec<Box<dyn SceneList>> = Vec::new();
    if has_ui {
        rows.push(Box::new(EntityScene(section_header(
            "Position".to_string(),
            bsn! { Node {} },
        ))));
        rows.push(Box::new(EntityScene(bsn! {
            Node { flex_direction: FlexDirection::Row, column_gap: px(6) }
            Children [ (postype_btn("Relative", false)), (postype_btn("Absolute", true)) ]
        })));
        rows.push(Box::new(EntityScene(section_header(
            "Anchor".to_string(),
            bsn! { Node {} },
        ))));
        rows.push(Box::new(EntityScene(anchor_grid_row(["↖", "↑", "↗"], 0))));
        rows.push(Box::new(EntityScene(anchor_grid_row(["←", "✛", "→"], 1))));
        rows.push(Box::new(EntityScene(anchor_grid_row(["↙", "↓", "↘"], 2))));
        rows.push(Box::new(EntityScene(fill_btn())));
        rows.push(Box::new(EntityScene(section_header(
            "Size".to_string(),
            bsn! { Node {} },
        ))));
        rows.push(Box::new(EntityScene(size_row("W", 0))));
        rows.push(Box::new(EntityScene(size_row("H", 1))));
    } else {
        rows.push(Box::new(EntityScene(bsn! {
            Node { padding: UiRect::axes(px(2), px(6)) }
            Children [ (label_dim("Select a UI node to lay it out  (Entity ▸ UI Node).".to_string())) ]
        })));
    }

    commands.entity(content).despawn_children();
    commands
        .entity(content)
        .queue_spawn_related_scenes::<Children>(rows);
}

// ---------------------------------------------------------------------------
// Observers
// ---------------------------------------------------------------------------

fn on_anchor_preset(
    act: On<Activate>,
    presets: Query<&AnchorPreset>,
    selection: Res<EditorSelection>,
    mut nodes: Query<&mut Node>,
) {
    let Ok(preset) = presets.get(act.entity) else {
        return;
    };
    let Some(e) = selection.primary else {
        return;
    };
    let Ok(mut node) = nodes.get_mut(e) else {
        return;
    };
    if preset.fill {
        apply_fill(&mut node);
    } else {
        apply_anchor(&mut node, preset.col, preset.row);
    }
}

fn on_ui_postype(
    act: On<Activate>,
    buttons: Query<&UiPosType>,
    selection: Res<EditorSelection>,
    mut nodes: Query<&mut Node>,
) {
    let Ok(button) = buttons.get(act.entity) else {
        return;
    };
    let Some(e) = selection.primary else {
        return;
    };
    if let Ok(mut node) = nodes.get_mut(e) {
        node.position_type = if button.0 {
            PositionType::Absolute
        } else {
            PositionType::Relative
        };
    }
}

fn on_ui_size(
    act: On<Activate>,
    buttons: Query<&UiSizeBtn>,
    selection: Res<EditorSelection>,
    mut nodes: Query<&mut Node>,
) {
    let Ok(button) = buttons.get(act.entity) else {
        return;
    };
    let Some(e) = selection.primary else {
        return;
    };
    if let Ok(mut node) = nodes.get_mut(e) {
        let val = button.choice.to_val();
        if button.axis == 0 {
            node.width = val;
        } else {
            node.height = val;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_top_left_pins_near_edges() {
        let mut node = Node::default();
        apply_anchor(&mut node, 0, 0);
        assert_eq!(node.position_type, PositionType::Absolute);
        assert_eq!(node.left, Val::Px(0.0));
        assert_eq!(node.top, Val::Px(0.0));
        assert_eq!(node.right, Val::Auto);
        assert_eq!(node.bottom, Val::Auto);
    }

    #[test]
    fn anchor_center_uses_auto_margins() {
        let mut node = Node::default();
        apply_anchor(&mut node, 1, 1);
        assert_eq!(node.left, Val::Px(0.0));
        assert_eq!(node.right, Val::Px(0.0));
        assert_eq!(node.margin.left, Val::Auto);
        assert_eq!(node.margin.right, Val::Auto);
        assert_eq!(node.margin.top, Val::Auto);
        assert_eq!(node.margin.bottom, Val::Auto);
    }

    #[test]
    fn anchor_bottom_right_pins_far_edges() {
        let mut node = Node::default();
        apply_anchor(&mut node, 2, 2);
        assert_eq!(node.left, Val::Auto);
        assert_eq!(node.top, Val::Auto);
        assert_eq!(node.right, Val::Px(0.0));
        assert_eq!(node.bottom, Val::Px(0.0));
    }

    #[test]
    fn fill_zeroes_all_insets() {
        let mut node = Node::default();
        apply_fill(&mut node);
        assert_eq!(node.left, Val::Px(0.0));
        assert_eq!(node.right, Val::Px(0.0));
        assert_eq!(node.top, Val::Px(0.0));
        assert_eq!(node.bottom, Val::Px(0.0));
    }

    #[test]
    fn size_choice_maps_to_val() {
        assert_eq!(SizeChoice::Auto.to_val(), Val::Auto);
        assert_eq!(SizeChoice::Px(120.0).to_val(), Val::Px(120.0));
        assert_eq!(SizeChoice::Pct(100.0).to_val(), Val::Percent(100.0));
    }
}
