//! Viewport transform manipulation: a visual translate gizmo (axis arrows + a selection
//! marker drawn with `bevy_gizmos`) plus drag-to-move. Left-dragging a scene entity in
//! the viewport translates it in the camera's view plane (3D) or screen plane (2D); the
//! inspector's per-axis number fields give precise, axis-constrained editing. (Axis-
//! constrained handle dragging is a later phase.)

use bevy_color::Color;
use bevy_ecs::prelude::*;
use bevy_gizmos::gizmos::Gizmos;
use bevy_math::Vec3;
use bevy_picking::events::{Drag, DragStart, Pointer};
use bevy_picking::pointer::PointerButton;
use bevy_transform::components::{GlobalTransform, Transform};

use crate::markers::{GameCamera, SceneEntity};
use crate::state::{EditorSelection, ViewportMode};

/// Draw translate-gizmo axis arrows and a selection marker at each selected entity.
pub fn draw_gizmos(
    selection: Res<EditorSelection>,
    transforms: Query<&GlobalTransform, With<SceneEntity>>,
    mut gizmos: Gizmos,
) {
    for &entity in selection.all.iter() {
        let Ok(global) = transforms.get(entity) else {
            continue;
        };
        let pos = global.translation();
        let len = 1.5;
        gizmos.arrow(pos, pos + Vec3::X * len, Color::srgb(0.95, 0.25, 0.25));
        gizmos.arrow(pos, pos + Vec3::Y * len, Color::srgb(0.35, 0.9, 0.35));
        gizmos.arrow(pos, pos + Vec3::Z * len, Color::srgb(0.35, 0.55, 1.0));
        // Negative-axis stubs so the pivot reads as a full cross/marker.
        gizmos.line(pos, pos - Vec3::X * 0.4, Color::srgb(0.95, 0.25, 0.25));
        gizmos.line(pos, pos - Vec3::Y * 0.4, Color::srgb(0.35, 0.9, 0.35));
        gizmos.line(pos, pos - Vec3::Z * 0.4, Color::srgb(0.35, 0.55, 1.0));
    }
}

/// Select a scene entity when the user starts dragging it in the viewport.
pub fn select_on_drag_start(
    drag: On<Pointer<DragStart>>,
    scene_q: Query<(), With<SceneEntity>>,
    mut selection: ResMut<EditorSelection>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let entity = drag.entity;
    if scene_q.contains(entity) && selection.primary != Some(entity) {
        selection.set_single(entity);
    }
}

/// Left-drag a scene entity to translate it: in the camera's view plane (3D) or the
/// screen plane (2D).
pub fn drag_to_translate(
    drag: On<Pointer<Drag>>,
    scene_q: Query<(), With<SceneEntity>>,
    camera_q: Query<&GlobalTransform, With<GameCamera>>,
    mut transforms: Query<&mut Transform>,
    mode: Res<ViewportMode>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let entity = drag.entity;
    if !scene_q.contains(entity) {
        return;
    }
    let Ok(mut transform) = transforms.get_mut(entity) else {
        return;
    };

    match *mode {
        ViewportMode::ThreeD => {
            let Ok(camera) = camera_q.single() else {
                return;
            };
            let right = camera.right();
            let up = camera.up();
            let distance = (camera.translation() - transform.translation)
                .length()
                .max(1.0);
            let scale = distance * 0.0015;
            transform.translation += right * drag.delta.x * scale - up * drag.delta.y * scale;
        }
        ViewportMode::TwoD => {
            transform.translation.x += drag.delta.x;
            transform.translation.y -= drag.delta.y;
        }
    }
}
