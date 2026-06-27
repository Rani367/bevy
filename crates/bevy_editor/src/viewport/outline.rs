//! Selection visualization in the viewport: a wireframe bounding box drawn around each
//! selected entity (so selection is visible beyond the gizmo), and "frame selection" (the
//! `F` key / View menu), which moves the editor camera to fit the selection.

use bevy_camera::primitives::Aabb;
use bevy_color::Color;
use bevy_ecs::prelude::*;
use bevy_gizmos::gizmos::Gizmos;
use bevy_math::{Isometry3d, Vec3};
use bevy_transform::components::GlobalTransform;

use crate::markers::SceneEntity;
use crate::state::EditorSelection;

use super::{Editor2dCamera, Editor3dCamera, FrameSelectionRequest};

/// Selection highlight color (a warm orange, the de-facto editor selection color).
const SELECTION_COLOR: Color = Color::srgb(1.0, 0.55, 0.1);

/// Draw a wireframe box around each selected entity each frame.
pub fn draw_selection_outline(
    selection: Res<EditorSelection>,
    entities: Query<(&GlobalTransform, Option<&Aabb>), With<SceneEntity>>,
    mut gizmos: Gizmos,
) {
    for &entity in &selection.all {
        let Ok((transform, aabb)) = entities.get(entity) else {
            continue;
        };
        match aabb {
            Some(aabb) => draw_box(&mut gizmos, transform, aabb, SELECTION_COLOR),
            // Lights / empties have no mesh AABB — mark them with a small sphere.
            None => {
                gizmos.sphere(
                    Isometry3d::from_translation(transform.translation()),
                    0.35,
                    SELECTION_COLOR,
                );
            }
        }
    }
}

/// Draw the 12 edges of an entity's world-space oriented bounding box.
fn draw_box(gizmos: &mut Gizmos, transform: &GlobalTransform, aabb: &Aabb, color: Color) {
    let center = Vec3::from(aabb.center);
    let half = Vec3::from(aabb.half_extents);
    let signs = [-1.0_f32, 1.0];
    let mut corners = [Vec3::ZERO; 8];
    for (i, corner) in corners.iter_mut().enumerate() {
        let local = center
            + Vec3::new(
                signs[i & 1] * half.x,
                signs[(i >> 1) & 1] * half.y,
                signs[(i >> 2) & 1] * half.z,
            );
        *corner = transform.transform_point(local);
    }
    for i in 0..8u32 {
        for bit in 0..3u32 {
            let j = i ^ (1 << bit);
            if i < j {
                gizmos.line(corners[i as usize], corners[j as usize], color);
            }
        }
    }
}

/// World-space bounds of an entity (its transformed AABB, or just its origin).
fn entity_bounds(transform: &GlobalTransform, aabb: Option<&Aabb>) -> (Vec3, Vec3) {
    match aabb {
        Some(aabb) => {
            let center = Vec3::from(aabb.center);
            let half = Vec3::from(aabb.half_extents);
            let signs = [-1.0_f32, 1.0];
            let mut min = Vec3::splat(f32::MAX);
            let mut max = Vec3::splat(f32::MIN);
            for i in 0..8 {
                let local = center
                    + Vec3::new(
                        signs[i & 1] * half.x,
                        signs[(i >> 1) & 1] * half.y,
                        signs[(i >> 2) & 1] * half.z,
                    );
                let p = transform.transform_point(local);
                min = min.min(p);
                max = max.max(p);
            }
            (min, max)
        }
        None => {
            let p = transform.translation();
            (p - Vec3::splat(0.5), p + Vec3::splat(0.5))
        }
    }
}

/// Move the editor camera to frame the current selection (or the whole scene when nothing is
/// selected). Handles both the 3D orbit camera and the 2D pan camera.
pub fn on_frame_selection(
    _: On<FrameSelectionRequest>,
    selection: Res<EditorSelection>,
    entities: Query<(&GlobalTransform, Option<&Aabb>), With<SceneEntity>>,
    mut cam3: Query<&mut Editor3dCamera>,
    mut cam2: Query<(
        &mut bevy_transform::components::Transform,
        &mut Editor2dCamera,
    )>,
) {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    let mut any = false;

    let mut accumulate = |t: &GlobalTransform, a: Option<&Aabb>| {
        let (lo, hi) = entity_bounds(t, a);
        min = min.min(lo);
        max = max.max(hi);
        any = true;
    };

    if selection.all.is_empty() {
        for (t, a) in entities.iter() {
            accumulate(t, a);
        }
    } else {
        for &e in &selection.all {
            if let Ok((t, a)) = entities.get(e) {
                accumulate(t, a);
            }
        }
    }

    if !any {
        return;
    }

    let center = (min + max) * 0.5;
    let extent = (max - min).length() * 0.5;

    if let Ok(mut cam) = cam3.single_mut() {
        cam.focus = center;
        cam.radius = (extent * 2.4).clamp(1.0, 500.0);
    }
    if let Ok((mut transform, mut cam)) = cam2.single_mut() {
        transform.translation.x = center.x;
        transform.translation.y = center.y;
        let span = (max - min).max_element().max(1.0);
        cam.zoom = (span / 400.0).clamp(0.05, 50.0);
    }
}
