//! A lightweight heads-up overlay drawn inside the viewport: the current gizmo mode, snap
//! state, gizmo space, and camera-control hints. The nodes are `Pickable::IGNORE` so they
//! never intercept viewport picking / camera drags.

use bevy_ecs::hierarchy::Children;
use bevy_ecs::prelude::*;
use bevy_feathers::display::label_small;
use bevy_feathers::theme::{InheritableThemeTextColor, ThemeBackgroundColor};
use bevy_picking::Pickable;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_ui::widget::Text;
use bevy_ui::{px, BorderRadius, Display, FlexDirection, GlobalZIndex, Node, PositionType, UiRect};

use crate::state::{GizmoMode, GizmoSnap, GizmoSpace};
use crate::ui::style::{etokens, z};

/// Marks the HUD's dynamic status line (mode / snap / space), updated in place.
#[derive(Component, Default, Clone, Copy)]
pub(crate) struct ViewportHud;

/// Attach the HUD overlay as a child of the viewport slot node (called once, when the
/// `ViewportNode` is first bound).
pub(crate) fn spawn_viewport_hud(commands: &mut Commands, slot: Entity) {
    commands
        .entity(slot)
        .queue_spawn_related_scenes::<Children>(vec![
            Box::new(EntityScene(hud_panel())) as Box<dyn SceneList>
        ]);
}

fn hud_panel() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(8),
            top: px(8),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: px(2),
            padding: UiRect::axes(px(8), px(6)),
            border_radius: {BorderRadius::all(px(6))},
        }
        Pickable::IGNORE
        GlobalZIndex(z::HUD)
        ThemeBackgroundColor(etokens::HUD_BG)
        InheritableThemeTextColor(etokens::HUD_TEXT)
        Children [
            (label_small("") ViewportHud),
            (label_small("RMB orbit · MMB pan · Wheel zoom · F frame · W/E/R gizmo")),
        ]
    }
}

/// Refresh the HUD status line when the gizmo mode / snap / space changes (or when the HUD is
/// first spawned), so it's effectively free at idle.
pub(crate) fn update_viewport_hud(
    gizmo: Res<GizmoMode>,
    snap: Res<GizmoSnap>,
    space: Res<GizmoSpace>,
    added: Query<(), Added<ViewportHud>>,
    mut huds: Query<&mut Text, With<ViewportHud>>,
) {
    if !(gizmo.is_changed() || snap.is_changed() || space.is_changed() || !added.is_empty()) {
        return;
    }
    let mode = match *gizmo {
        GizmoMode::Translate => "Move (W)",
        GizmoMode::Rotate => "Rotate (E)",
        GizmoMode::Scale => "Scale (R)",
    };
    let snap_txt = if snap.enabled {
        format!("Snap {:.2}", snap.translate)
    } else {
        "Snap off".to_string()
    };
    let space_txt = match *space {
        GizmoSpace::World => "World",
        GizmoSpace::Local => "Local",
    };
    let line = format!("{mode}  ·  {snap_txt}  ·  {space_txt}");
    for mut text in huds.iter_mut() {
        text.0 = line.clone();
    }
}
