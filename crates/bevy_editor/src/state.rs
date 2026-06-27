//! Core editor state: the edit/play mode, the current selection, and the active
//! gizmo / viewport modes. These are the resources the rest of the editor reads
//! and writes; they are intentionally kept tiny and dependency-free.

use bevy_ecs::prelude::*;
use bevy_math::Vec3;
use bevy_state::state::States;

/// Whether the editor is currently editing the scene, running it (play mode), or
/// paused mid-play. Game-logic systems run only [`EditorState::Playing`]; entering
/// play mode snapshots the scene and stopping restores it.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EditorState {
    /// Authoring the scene. Game logic is frozen; gizmos and picking are active.
    #[default]
    Editing,
    /// Running the scene's game logic.
    Playing,
    /// Play mode, but logic is frozen (rendering continues).
    Paused,
}

/// The set of currently selected scene entities. `primary` is the most-recently
/// selected entity (the one the inspector and gizmo act on); `all` is the full
/// selection (for future multi-select operations).
#[derive(Resource, Debug, Default)]
pub struct EditorSelection {
    /// The primary selection — inspector target and gizmo pivot.
    pub primary: Option<Entity>,
    /// All selected entities, including `primary`.
    pub all: Vec<Entity>,
}

impl EditorSelection {
    /// Replace the selection with a single entity.
    pub fn set_single(&mut self, entity: Entity) {
        self.primary = Some(entity);
        self.all.clear();
        self.all.push(entity);
    }

    /// Add (or, if already present, remove) an entity from the selection, keeping
    /// it as the primary when added.
    pub fn toggle(&mut self, entity: Entity) {
        if let Some(idx) = self.all.iter().position(|&e| e == entity) {
            self.all.remove(idx);
            if self.primary == Some(entity) {
                self.primary = self.all.last().copied();
            }
        } else {
            self.all.push(entity);
            self.primary = Some(entity);
        }
    }

    /// Clear the entire selection.
    pub fn clear(&mut self) {
        self.primary = None;
        self.all.clear();
    }

    /// Whether `entity` is part of the current selection.
    pub fn contains(&self, entity: Entity) -> bool {
        self.all.contains(&entity)
    }

    /// Whether nothing is selected.
    pub fn is_empty(&self) -> bool {
        self.all.is_empty()
    }
}

/// Marker placed on the currently-selected scene entities. Kept in sync with
/// [`EditorSelection`]; named to avoid clashing with `bevy_ui::Selected`, which
/// Feathers list rows already use.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct EditorSelected;

/// Which transform the gizmo manipulates.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GizmoMode {
    /// Drag axis handles to translate.
    #[default]
    Translate,
    /// Drag to rotate (Phase 2).
    Rotate,
    /// Drag to scale (Phase 2).
    Scale,
}

/// Per-drag gizmo state. The world-space axis chosen at drag start constrains the
/// translate/scale gesture to a single axis; `None` means free-plane translate. `active`
/// gates the per-gesture undo snapshot (taken once on drag start).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct GizmoDrag {
    /// World-space axis the current drag is constrained to, if any.
    pub axis: Option<Vec3>,
    /// Whether a gizmo drag gesture is currently in progress.
    pub active: bool,
    /// Whether the axis constraint has been decided for the current gesture (decided on
    /// the first drag frame from the initial drag direction).
    pub chosen: bool,
    /// Raw angle accumulated over a rotate gesture (radians), used so snapping can apply
    /// clean stepped increments rather than quantizing each tiny per-frame delta.
    pub accum: f32,
    /// Snapped angle already applied this rotate gesture (radians).
    pub applied: f32,
}

/// Grid/angle snapping for gizmo manipulation. When [`enabled`](Self::enabled) (toolbar
/// toggle) or while a modifier key is held, translate snaps each moved entity to a position
/// grid, scale snaps to a scale grid, and rotate applies in fixed angular steps.
#[derive(Resource, Debug, Clone, Copy)]
pub struct GizmoSnap {
    /// Whether snapping is toggled on (independent of the held-modifier shortcut).
    pub enabled: bool,
    /// Translate grid increment, in world units.
    pub translate: f32,
    /// Rotate increment, in radians.
    pub rotate: f32,
    /// Scale grid increment.
    pub scale: f32,
}

impl Default for GizmoSnap {
    fn default() -> Self {
        Self {
            enabled: false,
            translate: 0.5,
            rotate: core::f32::consts::FRAC_PI_8, // 22.5°
            scale: 0.25,
        }
    }
}

/// Whether the gizmo operates in world or local space.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GizmoSpace {
    /// World-aligned axes.
    #[default]
    World,
    /// Entity-local axes.
    Local,
}

/// Whether the viewport renders a 2D or 3D scene. Switching rebuilds the scene
/// camera and its controller.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewportMode {
    /// 2D scene: `Camera2d`, pan/zoom controller, sprite picking.
    TwoD,
    /// 3D scene: `Camera3d`, orbit controller, mesh picking.
    #[default]
    ThreeD,
}

impl ViewportMode {
    /// Flip between 2D and 3D.
    pub fn toggle(&mut self) {
        *self = match *self {
            ViewportMode::TwoD => ViewportMode::ThreeD,
            ViewportMode::ThreeD => ViewportMode::TwoD,
        };
    }
}
