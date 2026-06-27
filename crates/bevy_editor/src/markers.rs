//! Marker components and shared constants used throughout the editor.

use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;

/// Marks an entity as belonging to the *edited scene* (the user's game world).
///
/// Everything tagged with this appears in the hierarchy panel, is shown in the
/// inspector, is written to scene files on save, and is captured/restored by the
/// play-mode snapshot. Editor infrastructure must **not** carry this marker.
#[derive(Component, Reflect, Debug, Clone, Copy, Default)]
#[reflect(Component)]
pub struct SceneEntity;

/// Marks editor infrastructure (UI panels, the editor/scene camera, the grid, gizmo
/// handles, ...). Entities with this marker are excluded from the hierarchy, scene
/// serialization, and play-mode snapshot/restore, so the editor chrome never leaks
/// into the user's scene.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct EditorEntity;

/// Marks the offscreen camera that renders the edited scene into the viewport panel.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct GameCamera;

/// Render layer that the scene camera, the infinite grid, and viewport gizmos all
/// share, so those overlays render *into the viewport image* rather than over the
/// editor UI (which is drawn by a separate UI camera on the default layer).
pub const SCENE_LAYER: usize = 0;
