//! Editor camera controllers for the scene viewport: an orbit/pan/zoom controller for
//! 3D and a pan/zoom controller for 2D. Only one is active at a time (whichever
//! component is on the live scene camera). Input is read globally; for the MVP we don't
//! gate on "pointer over viewport", which keeps the interaction simple and robust.

use bevy_ecs::prelude::*;
use bevy_input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButton};
use bevy_input::ButtonInput;
use bevy_math::{EulerRot, Quat, Vec3};
use bevy_transform::components::Transform;

/// Orbit/pan/zoom controller for the 3D scene camera. Holds the pivot the camera looks
/// at plus spherical coordinates; the transform is recomputed from these every frame.
#[derive(Component, Debug, Clone, Copy)]
pub struct Editor3dCamera {
    /// The point the camera orbits and looks at.
    pub focus: Vec3,
    /// Distance from the focus.
    pub radius: f32,
    /// Yaw around the world Y axis (radians).
    pub yaw: f32,
    /// Pitch around the camera's right axis (radians).
    pub pitch: f32,
}

impl Default for Editor3dCamera {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            radius: 14.0,
            yaw: -0.8,
            pitch: -0.5,
        }
    }
}

/// Mouse look sensitivity (radians per pixel).
const ORBIT_SENSITIVITY: f32 = 0.005;
/// Wheel zoom factor per scroll unit.
const ZOOM_SENSITIVITY: f32 = 0.12;

/// Drives [`Editor3dCamera`]: right-drag orbits, middle-drag pans the focus, wheel zooms.
pub fn orbit_camera(
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut q: Query<(&mut Transform, &mut Editor3dCamera)>,
) {
    let Ok((mut transform, mut cam)) = q.single_mut() else {
        return;
    };

    let delta = motion.delta;

    if mouse.pressed(MouseButton::Right) && delta != bevy_math::Vec2::ZERO {
        cam.yaw -= delta.x * ORBIT_SENSITIVITY;
        cam.pitch = (cam.pitch - delta.y * ORBIT_SENSITIVITY).clamp(-1.54, 1.54);
    }

    if mouse.pressed(MouseButton::Middle) && delta != bevy_math::Vec2::ZERO {
        let rot = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
        let right = rot * Vec3::X;
        let up = rot * Vec3::Y;
        let pan_scale = cam.radius * 0.0015;
        cam.focus += (-right * delta.x + up * delta.y) * pan_scale;
    }

    if scroll.delta.y != 0.0 {
        cam.radius = (cam.radius * (1.0 - scroll.delta.y * ZOOM_SENSITIVITY)).clamp(0.5, 500.0);
    }

    let rot = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
    transform.translation = cam.focus + rot * Vec3::new(0.0, 0.0, cam.radius);
    transform.rotation = rot;
}

/// Pan/zoom controller for the 2D scene camera. Zoom is applied as a uniform camera
/// transform scale (a larger scale zooms out).
#[derive(Component, Debug, Clone, Copy)]
pub struct Editor2dCamera {
    /// Uniform zoom (world units per screen pixel-ish); 1.0 is the default.
    pub zoom: f32,
}

impl Default for Editor2dCamera {
    fn default() -> Self {
        Self { zoom: 1.0 }
    }
}

/// Drives [`Editor2dCamera`]: right/middle-drag pans, wheel zooms.
pub fn pan_camera(
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut q: Query<(&mut Transform, &mut Editor2dCamera)>,
) {
    let Ok((mut transform, mut cam)) = q.single_mut() else {
        return;
    };

    let delta = motion.delta;

    if (mouse.pressed(MouseButton::Right) || mouse.pressed(MouseButton::Middle))
        && delta != bevy_math::Vec2::ZERO
    {
        transform.translation.x -= delta.x * cam.zoom;
        transform.translation.y += delta.y * cam.zoom;
    }

    if scroll.delta.y != 0.0 {
        cam.zoom = (cam.zoom * (1.0 - scroll.delta.y * ZOOM_SENSITIVITY)).clamp(0.05, 50.0);
    }

    transform.scale = Vec3::new(cam.zoom, cam.zoom, 1.0);
}
