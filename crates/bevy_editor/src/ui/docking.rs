//! In-window dockable panels: each side panel can be collapsed (its body hidden) or torn
//! off to float freely within the editor window (and dragged around by its header, then
//! re-docked). The splitter resize handles are unaffected.
//!
//! Layout is data-driven: [`DockState`] records each panel's collapsed/floating state, and
//! [`apply_dock_layout`] reflects that onto the panels' `Node`s (content `Display`, root
//! `position_type` + offset + z-index). The shell tags each panel with [`Panel`], its body
//! with [`PanelContent`], and header controls with [`PanelHeader`] / [`PanelCollapseButton`]
//! / [`PanelFloatButton`].

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use bevy_picking::events::{Drag, Pointer};
use bevy_platform::collections::HashMap;
use bevy_ui::{px, Display, GlobalZIndex, Node, PositionType, Val};
use bevy_ui_widgets::Activate;
use bevy_window::{PrimaryWindow, Window};

/// The dockable side panels (the viewport is the fixed center).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PanelId {
    /// The entity hierarchy panel.
    Hierarchy,
    /// The component inspector panel.
    Inspector,
    /// The asset browser panel.
    Assets,
}

/// Marks a dockable panel's root node.
#[derive(Component, Clone, Copy)]
pub struct Panel(pub PanelId);

/// Marks a dockable panel's body (hidden when the panel is collapsed).
#[derive(Component, Clone, Copy)]
pub struct PanelContent(pub PanelId);

/// Marks a panel header (dragging it floats / moves the panel).
#[derive(Component, Clone, Copy)]
pub struct PanelHeader(pub PanelId);

/// The collapse/expand toggle button on a panel header.
#[derive(Component, Clone, Copy)]
pub struct PanelCollapseButton(pub PanelId);

/// The float/dock toggle button on a panel header.
#[derive(Component, Clone, Copy)]
pub struct PanelFloatButton(pub PanelId);

// `Default` impls so these can appear in `bsn!` scenes.
impl Default for Panel {
    fn default() -> Self {
        Self(PanelId::Hierarchy)
    }
}
impl Default for PanelContent {
    fn default() -> Self {
        Self(PanelId::Hierarchy)
    }
}
impl Default for PanelHeader {
    fn default() -> Self {
        Self(PanelId::Hierarchy)
    }
}
impl Default for PanelCollapseButton {
    fn default() -> Self {
        Self(PanelId::Hierarchy)
    }
}
impl Default for PanelFloatButton {
    fn default() -> Self {
        Self(PanelId::Hierarchy)
    }
}

/// Per-panel dock state.
#[derive(Clone, Copy, Default)]
pub struct PanelDock {
    /// Whether the panel body is hidden.
    pub collapsed: bool,
    /// `Some(pos)` if the panel is torn off and floating at `pos` (window pixels); `None` when
    /// docked in its normal flex slot.
    pub floating: Option<Vec2>,
}

/// The dock layout for all panels.
#[derive(Resource, Default)]
pub struct DockState {
    panels: HashMap<PanelId, PanelDock>,
}

impl DockState {
    /// The dock state for `id` (read-only view; absent panels are docked + expanded).
    pub fn get(&self, id: PanelId) -> PanelDock {
        self.panels.get(&id).copied().unwrap_or_default()
    }
    fn entry(&mut self, id: PanelId) -> &mut PanelDock {
        self.panels.entry(id).or_default()
    }
}

/// Z-index for a floating panel (above the docked panels / viewport).
const FLOAT_Z: i32 = 50;
/// Where a panel first appears when torn off (top-left, window pixels).
const FLOAT_ORIGIN: Vec2 = Vec2::new(80.0, 80.0);
/// Height a torn-off panel floats at, so it reads as a window rather than a full-height strip.
const FLOAT_HEIGHT: f32 = 320.0;
/// Keep at least this many pixels of a floating panel on-screen on each axis.
const KEEP_VISIBLE: f32 = 120.0;

/// Installs the docking systems + header-control observers.
pub struct DockingPlugin;

impl Plugin for DockingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DockState>()
            .add_systems(Update, apply_dock_layout)
            .add_observer(on_collapse_button)
            .add_observer(on_float_button)
            .add_observer(on_header_drag);
    }
}

/// Toggle a panel collapsed/expanded.
fn on_collapse_button(
    act: On<Activate>,
    buttons: Query<&PanelCollapseButton>,
    mut dock: ResMut<DockState>,
) {
    if let Ok(button) = buttons.get(act.entity) {
        let d = dock.entry(button.0);
        d.collapsed = !d.collapsed;
    }
}

/// Toggle a panel between floating and docked.
fn on_float_button(
    act: On<Activate>,
    buttons: Query<&PanelFloatButton>,
    mut dock: ResMut<DockState>,
) {
    if let Ok(button) = buttons.get(act.entity) {
        let d = dock.entry(button.0);
        d.floating = if d.floating.is_some() {
            None
        } else {
            Some(FLOAT_ORIGIN)
        };
    }
}

/// Dragging a panel header floats it (if docked) and moves it.
fn on_header_drag(
    drag: On<Pointer<Drag>>,
    headers: Query<&PanelHeader>,
    mut dock: ResMut<DockState>,
) {
    if let Ok(header) = headers.get(drag.entity) {
        let delta = Vec2::new(drag.delta.x, drag.delta.y);
        let d = dock.entry(header.0);
        d.floating = Some(d.floating.unwrap_or(FLOAT_ORIGIN) + delta);
    }
}

/// Clamp a floating panel's top-left so at least [`KEEP_VISIBLE`] px stay within `window`.
fn clamp_float_pos(pos: Vec2, window: Vec2) -> Vec2 {
    Vec2::new(
        pos.x.clamp(0.0, (window.x - KEEP_VISIBLE).max(0.0)),
        pos.y.clamp(0.0, (window.y - KEEP_VISIBLE).max(0.0)),
    )
}

/// Reflect [`DockState`] onto the panel nodes whenever it changes.
///
/// Gated on `DockState` changes (not the window), so resizing the window while a panel floats
/// won't re-clamp until that panel is next interacted with — acceptable, and keeps this off the
/// per-frame relayout path.
fn apply_dock_layout(
    dock: Res<DockState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut contents: Query<(&PanelContent, &mut Node), Without<Panel>>,
    mut panels: Query<(&Panel, &mut Node, &mut GlobalZIndex), Without<PanelContent>>,
) {
    if !dock.is_changed() {
        return;
    }
    // No window (briefly, at startup/shutdown) → an infinite extent makes the clamp a no-op.
    let win = windows
        .single()
        .map(|w| Vec2::new(w.width(), w.height()))
        .unwrap_or(Vec2::splat(f32::INFINITY));
    for (content, mut node) in &mut contents {
        node.display = if dock.get(content.0).collapsed {
            Display::None
        } else {
            Display::Flex
        };
    }
    for (panel, mut node, mut z) in &mut panels {
        match dock.get(panel.0).floating {
            Some(pos) => {
                let pos = clamp_float_pos(pos, win);
                node.position_type = PositionType::Absolute;
                node.left = px(pos.x);
                node.top = px(pos.y);
                // Give the panel a window-like height; width is left to the splitter.
                node.height = px(FLOAT_HEIGHT);
                z.0 = FLOAT_Z;
            }
            None => {
                node.position_type = PositionType::Relative;
                node.left = Val::Auto;
                node.top = Val::Auto;
                // Restore the panel's normal stretch-to-fill height.
                node.height = Val::Auto;
                z.0 = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_float_pos_keeps_panel_on_screen() {
        let window = Vec2::new(1280.0, 720.0);
        // In-bounds is untouched.
        assert_eq!(
            clamp_float_pos(Vec2::new(200.0, 150.0), window),
            Vec2::new(200.0, 150.0)
        );
        // Dragged far off the bottom-right → pinned so KEEP_VISIBLE px remain visible.
        assert_eq!(
            clamp_float_pos(Vec2::new(9999.0, 9999.0), window),
            Vec2::new(1280.0 - KEEP_VISIBLE, 720.0 - KEEP_VISIBLE),
        );
        // Negative (off the top-left) clamps to the origin.
        assert_eq!(clamp_float_pos(Vec2::new(-50.0, -10.0), window), Vec2::ZERO);
        // A window smaller than KEEP_VISIBLE still clamps to a valid (0,0).
        assert_eq!(
            clamp_float_pos(Vec2::new(40.0, 40.0), Vec2::new(80.0, 80.0)),
            Vec2::ZERO
        );
    }
}
