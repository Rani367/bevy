//! A draggable splitter handle for resizing dock areas. Feathers has no splitter widget,
//! so we add a minimal one: a thin node that, when dragged, adjusts the pixel width (or height)
//! of a neighboring area. It reuses the same `Pointer<Drag>` mechanism Feathers' own
//! number-input scrubber uses, so the interaction is a known-good pattern.

use bevy_ecs::hierarchy::{ChildOf, Children};
use bevy_ecs::prelude::*;
use bevy_picking::events::{Drag, Pointer};
use bevy_ui::{Node, Val};

/// Which neighbor a [`Splitter`] resizes as it is dragged.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ResizeSide {
    /// Resize the sibling immediately before the splitter (e.g. a left-docked area):
    /// dragging toward the splitter's far edge grows it.
    #[default]
    Prev,
    /// Resize the sibling immediately after the splitter (e.g. a right/bottom-docked area):
    /// dragging toward the splitter's near edge grows it.
    Next,
}

/// Whether a [`Splitter`] resizes along the horizontal (width) or vertical (height) axis.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SplitAxis {
    /// Resize the neighbor's width (a vertical handle between columns).
    #[default]
    Horizontal,
    /// Resize the neighbor's height (a horizontal handle between rows).
    Vertical,
}

/// A draggable resize handle. Placed between two siblings in a flex row/column; on drag it
/// changes the pixel size of the neighbor indicated by [`ResizeSide`] along [`SplitAxis`].
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Splitter {
    /// Which neighbor to resize.
    pub resize: ResizeSide,
    /// Which axis to resize along.
    pub axis: SplitAxis,
}

/// Minimum and maximum size a panel can be dragged to.
const MIN_PANEL: f32 = 120.0;
const MAX_PANEL: f32 = 1200.0;

/// Per-splitter drag observer (attached via `on(on_splitter_drag)` in the shell). Finds the
/// splitter's neighboring area and nudges its size by the pointer delta along the split axis.
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

    let delta_main = match splitter.axis {
        SplitAxis::Horizontal => drag.delta.x,
        SplitAxis::Vertical => drag.delta.y,
    };
    let (target, delta) = match splitter.resize {
        ResizeSide::Prev if idx > 0 => (sibs[idx - 1], delta_main),
        ResizeSide::Next if idx + 1 < sibs.len() => (sibs[idx + 1], -delta_main),
        _ => return,
    };

    if let Ok(mut node) = nodes.get_mut(target) {
        let field = match splitter.axis {
            SplitAxis::Horizontal => &mut node.width,
            SplitAxis::Vertical => &mut node.height,
        };
        let current = match *field {
            Val::Px(w) => w,
            _ => 200.0,
        };
        *field = Val::Px((current + delta).clamp(MIN_PANEL, MAX_PANEL));
    }
}
