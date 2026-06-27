//! A draggable splitter handle for resizing panels. Feathers has no splitter widget,
//! so we add a minimal one: a thin node that, when dragged, adjusts the pixel width of
//! a neighboring panel. It reuses the same `Pointer<Drag>` mechanism Feathers' own
//! number-input scrubber uses, so the interaction is a known-good pattern.

use bevy_ecs::hierarchy::{ChildOf, Children};
use bevy_ecs::prelude::*;
use bevy_picking::events::{Drag, Pointer};
use bevy_ui::{Node, Val};

/// Which neighbor a [`Splitter`] resizes as it is dragged horizontally.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ResizeSide {
    /// Resize the sibling immediately before the splitter (e.g. a left-docked panel):
    /// dragging right grows it.
    #[default]
    Prev,
    /// Resize the sibling immediately after the splitter (e.g. a right-docked panel):
    /// dragging left grows it.
    Next,
}

/// A draggable resize handle. Placed between two sibling panels in a flex row; on drag
/// it changes the pixel width of the neighbor indicated by [`ResizeSide`].
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Splitter {
    /// Which neighbor to resize.
    pub resize: ResizeSide,
}

/// Minimum and maximum width a panel can be dragged to.
const MIN_PANEL_WIDTH: f32 = 120.0;
const MAX_PANEL_WIDTH: f32 = 900.0;

/// Per-splitter drag observer (attached via `on(on_splitter_drag)` in the shell). Finds
/// the splitter's neighboring panel and nudges its width by the pointer delta.
pub fn on_splitter_drag(
    drag: On<Pointer<Drag>>,
    splitters: Query<(&Splitter, &ChildOf)>,
    children: Query<&Children>,
    mut nodes: Query<&mut Node>,
) {
    let dragged = drag.entity;
    let Ok((splitter, child_of)) = splitters.get(dragged) else {
        return;
    };
    let Ok(siblings) = children.get(child_of.parent()) else {
        return;
    };
    let sibs: Vec<Entity> = siblings.iter().collect();
    let Some(idx) = sibs.iter().position(|&e| e == dragged) else {
        return;
    };

    let (target, delta) = match splitter.resize {
        ResizeSide::Prev if idx > 0 => (sibs[idx - 1], drag.delta.x),
        ResizeSide::Next if idx + 1 < sibs.len() => (sibs[idx + 1], -drag.delta.x),
        _ => return,
    };

    if let Ok(mut node) = nodes.get_mut(target) {
        let current = match node.width {
            Val::Px(w) => w,
            _ => 200.0,
        };
        node.width = Val::Px((current + delta).clamp(MIN_PANEL_WIDTH, MAX_PANEL_WIDTH));
    }
}
