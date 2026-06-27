//! Viewport transform manipulation: a mode-aware gizmo (translate / rotate / scale)
//! drawn with `bevy_gizmos`, plus drag-to-manipulate.
//!
//! Translate is **axis-constrained** when the initial drag direction lines up with a
//! screen-projected world (or local) axis, and falls back to free view-plane dragging
//! otherwise — no separate handle entities are needed, which keeps the gizmo out of the
//! `SceneEntity`/picking queries. Rotate and scale apply to the whole selection. One undo
//! entry is captured per drag gesture (on drag start).

use core::f32::consts::FRAC_PI_2;

use bevy_camera::Camera;
use bevy_color::Color;
use bevy_ecs::prelude::*;
use bevy_gizmos::gizmos::Gizmos;
use bevy_math::{Isometry3d, Quat, Vec2, Vec3};
use bevy_picking::events::{Drag, DragEnd, DragStart, Pointer};
use bevy_picking::pointer::PointerButton;
use bevy_transform::components::{GlobalTransform, Transform};

use crate::markers::{GameCamera, SceneEntity};
use crate::state::{EditorSelection, GizmoDrag, GizmoMode, GizmoSpace, ViewportMode};
use crate::undo::push_undo;

const ROTATE_SENSITIVITY: f32 = 0.01;
const SCALE_SENSITIVITY: f32 = 0.005;
/// World-units moved per screen pixel, scaled by distance to the camera (matches the
/// original free-drag feel).
const TRANSLATE_SCALE: f32 = 0.0015;
/// Minimum `|drag_dir · axis_screen|` for a drag to lock onto a single axis.
const AXIS_LOCK_THRESHOLD: f32 = 0.6;

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Draw the gizmo for the active [`GizmoMode`] at each selected entity.
pub fn draw_gizmos(
    selection: Res<EditorSelection>,
    gizmo_drag: Res<GizmoDrag>,
    mode: Res<GizmoMode>,
    transforms: Query<&GlobalTransform, With<SceneEntity>>,
    mut gizmos: Gizmos,
) {
    for &entity in selection.all.iter() {
        let Ok(global) = transforms.get(entity) else {
            continue;
        };
        let pos = global.translation();
        match *mode {
            GizmoMode::Translate => draw_translate(&mut gizmos, pos, gizmo_drag.axis),
            GizmoMode::Rotate => draw_rotate(&mut gizmos, pos),
            GizmoMode::Scale => draw_scale(&mut gizmos, pos),
        }
    }
}

const AXIS_X: Color = Color::srgb(0.95, 0.25, 0.25);
const AXIS_Y: Color = Color::srgb(0.35, 0.9, 0.35);
const AXIS_Z: Color = Color::srgb(0.35, 0.55, 1.0);

/// Translate gizmo: X/Y/Z arrows. When a drag has locked onto an axis, that direction is
/// overdrawn in white.
fn draw_translate(gizmos: &mut Gizmos, pos: Vec3, engaged: Option<Vec3>) {
    let len = 1.5;
    gizmos.arrow(pos, pos + Vec3::X * len, AXIS_X);
    gizmos.arrow(pos, pos + Vec3::Y * len, AXIS_Y);
    gizmos.arrow(pos, pos + Vec3::Z * len, AXIS_Z);
    // Negative-axis stubs so the pivot reads as a full cross/marker.
    gizmos.line(pos, pos - Vec3::X * 0.4, AXIS_X);
    gizmos.line(pos, pos - Vec3::Y * 0.4, AXIS_Y);
    gizmos.line(pos, pos - Vec3::Z * 0.4, AXIS_Z);
    if let Some(axis) = engaged {
        let dir = axis.normalize_or_zero();
        if dir != Vec3::ZERO {
            gizmos.arrow(pos, pos + dir * len, Color::WHITE);
        }
    }
}

/// Rotate gizmo: three color-coded rings, one per axis plane.
fn draw_rotate(gizmos: &mut Gizmos, pos: Vec3) {
    let r = 1.2;
    gizmos.circle(
        Isometry3d::new(pos, Quat::from_rotation_y(FRAC_PI_2)),
        r,
        AXIS_X,
    );
    gizmos.circle(
        Isometry3d::new(pos, Quat::from_rotation_x(FRAC_PI_2)),
        r,
        AXIS_Y,
    );
    gizmos.circle(Isometry3d::new(pos, Quat::IDENTITY), r, AXIS_Z);
}

/// Scale gizmo: X/Y/Z arms capped with small boxes ("handles").
fn draw_scale(gizmos: &mut Gizmos, pos: Vec3) {
    let len = 1.3;
    for (axis, color) in [(Vec3::X, AXIS_X), (Vec3::Y, AXIS_Y), (Vec3::Z, AXIS_Z)] {
        let tip = pos + axis * len;
        gizmos.line(pos, tip, color);
        gizmos.sphere(Isometry3d::new(tip, Quat::IDENTITY), 0.12, color);
    }
}

// ---------------------------------------------------------------------------
// Dragging
// ---------------------------------------------------------------------------

/// Begin a gizmo drag: select the entity (preserving a multi-selection if it's already a
/// member), capture one undo snapshot, and reset the per-gesture axis state.
pub fn begin_gizmo_drag(
    drag: On<Pointer<DragStart>>,
    scene_q: Query<(), With<SceneEntity>>,
    mut selection: ResMut<EditorSelection>,
    mut gizmo_drag: ResMut<GizmoDrag>,
    mut commands: Commands,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let entity = drag.entity;
    if !scene_q.contains(entity) {
        return;
    }
    if !selection.contains(entity) {
        selection.set_single(entity);
    }
    push_undo(&mut commands);
    gizmo_drag.active = true;
    gizmo_drag.chosen = false;
    gizmo_drag.axis = None;
}

/// End a gizmo drag: clear the per-gesture axis state.
pub fn end_gizmo_drag(_: On<Pointer<DragEnd>>, mut gizmo_drag: ResMut<GizmoDrag>) {
    gizmo_drag.active = false;
    gizmo_drag.chosen = false;
    gizmo_drag.axis = None;
}

/// Apply a gizmo drag to the selection according to the active [`GizmoMode`].
pub fn gizmo_drag(
    drag: On<Pointer<Drag>>,
    scene_q: Query<(), With<SceneEntity>>,
    cam_q: Query<(&Camera, &GlobalTransform), With<GameCamera>>,
    globals: Query<&GlobalTransform>,
    mut transforms: Query<&mut Transform>,
    selection: Res<EditorSelection>,
    mode: Res<GizmoMode>,
    space: Res<GizmoSpace>,
    vmode: Res<ViewportMode>,
    mut gizmo_drag: ResMut<GizmoDrag>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let entity = drag.entity;
    if !scene_q.contains(entity) {
        return;
    }
    let Ok((cam, cam_global)) = cam_q.single() else {
        return;
    };
    let Ok(pivot_global) = globals.get(entity) else {
        return;
    };
    let pivot = pivot_global.translation();
    let local_rot = transforms
        .get(entity)
        .map(|t| t.rotation)
        .unwrap_or(Quat::IDENTITY);

    // Decide the axis constraint once, on the first drag frame (translate / 3D only).
    if !gizmo_drag.chosen {
        gizmo_drag.axis = if *mode == GizmoMode::Translate && *vmode == ViewportMode::ThreeD {
            choose_axis(drag.delta, pivot, cam, cam_global, *space, local_rot)
        } else {
            None
        };
        gizmo_drag.chosen = true;
    }

    let targets: Vec<Entity> = if selection.contains(entity) {
        selection.all.clone()
    } else {
        vec![entity]
    };

    match *mode {
        GizmoMode::Translate => {
            let world_delta = match (*vmode, gizmo_drag.axis) {
                (ViewportMode::TwoD, _) => Vec3::new(drag.delta.x, -drag.delta.y, 0.0),
                (ViewportMode::ThreeD, Some(axis)) => {
                    let (Some(p_ndc), Some(pa_ndc)) = (
                        cam.world_to_ndc(cam_global, pivot),
                        cam.world_to_ndc(cam_global, pivot + axis),
                    ) else {
                        return;
                    };
                    let axis_screen =
                        Vec2::new(pa_ndc.x - p_ndc.x, -(pa_ndc.y - p_ndc.y)).normalize_or_zero();
                    let along = drag.delta.dot(axis_screen);
                    let dist = (cam_global.translation() - pivot).length().max(1.0);
                    axis.normalize_or_zero() * along * dist * TRANSLATE_SCALE
                }
                (ViewportMode::ThreeD, None) => {
                    let dist = (cam_global.translation() - pivot).length().max(1.0);
                    let scale = dist * TRANSLATE_SCALE;
                    cam_global.right() * drag.delta.x * scale
                        - cam_global.up() * drag.delta.y * scale
                }
            };
            for e in targets {
                if let Ok(mut t) = transforms.get_mut(e) {
                    t.translation += world_delta;
                }
            }
        }
        GizmoMode::Rotate => {
            let angle = drag.delta.x * ROTATE_SENSITIVITY;
            for e in targets {
                let Ok(mut t) = transforms.get_mut(e) else {
                    continue;
                };
                match (*vmode, *space) {
                    (ViewportMode::TwoD, _) => {
                        t.rotation = Quat::from_rotation_z(angle) * t.rotation;
                    }
                    (_, GizmoSpace::World) => {
                        t.rotation = Quat::from_axis_angle(Vec3::Y, angle) * t.rotation;
                    }
                    (_, GizmoSpace::Local) => {
                        t.rotation *= Quat::from_axis_angle(Vec3::Y, angle);
                    }
                }
            }
        }
        GizmoMode::Scale => {
            let factor = (1.0 + (drag.delta.x - drag.delta.y) * SCALE_SENSITIVITY).max(0.01);
            for e in targets {
                if let Ok(mut t) = transforms.get_mut(e) {
                    t.scale *= factor;
                }
            }
        }
    }
}

/// Pick the world-space axis whose screen projection best matches the drag direction, or
/// `None` to drag freely in the view plane.
fn choose_axis(
    drag_dir: Vec2,
    pivot: Vec3,
    cam: &Camera,
    cam_global: &GlobalTransform,
    space: GizmoSpace,
    rotation: Quat,
) -> Option<Vec3> {
    let axes = match space {
        GizmoSpace::World => [Vec3::X, Vec3::Y, Vec3::Z],
        GizmoSpace::Local => [rotation * Vec3::X, rotation * Vec3::Y, rotation * Vec3::Z],
    };
    let dir = drag_dir.normalize_or_zero();
    if dir == Vec2::ZERO {
        return None;
    }
    let p_ndc = cam.world_to_ndc(cam_global, pivot)?;
    let mut best: Option<Vec3> = None;
    let mut best_dot = AXIS_LOCK_THRESHOLD;
    for axis in axes {
        let Some(pa_ndc) = cam.world_to_ndc(cam_global, pivot + axis) else {
            continue;
        };
        let axis_screen = Vec2::new(pa_ndc.x - p_ndc.x, -(pa_ndc.y - p_ndc.y)).normalize_or_zero();
        if axis_screen == Vec2::ZERO {
            continue;
        }
        let d = dir.dot(axis_screen).abs();
        if d > best_dot {
            best_dot = d;
            best = Some(axis);
        }
    }
    best
}
