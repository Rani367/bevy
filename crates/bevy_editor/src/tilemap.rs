//! The **Tilemap** editor panel (a bottom-dock tab): a visual tile palette to pick the paint
//! brush (replacing the number-key-only selection), plus grid controls for the selected
//! [`Tilemap`]. Painting itself stays in [`crate::gameplay`] — click a tile of the selected map
//! to paint it with the active brush. The panel rebuilds whenever the brush, selection, or a
//! tilemap changes.

use bevy_app::{App, Plugin, Update};
use bevy_color::Color;
use bevy_ecs::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton};
use bevy_feathers::display::{label_dim, label_small};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy_feathers::tokens;
use bevy_picking::events::{Click, Pointer};
use bevy_picking::Pickable;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_ui::widget::Text;
use bevy_ui::{
    px, AlignItems, BackgroundColor, BorderRadius, Display, FlexDirection, JustifyContent, Node,
    Overflow, UiRect,
};
use bevy_ui_widgets::{Activate, ScrollArea};

use crate::gameplay::{palette_color, TilePaint, Tilemap, PALETTE_LEN};
use crate::state::EditorSelection;
use crate::ui::style::{etokens, section_header};

/// Marks the rebuildable tilemap-panel container.
#[derive(Component, Default, Clone, Copy)]
struct TilemapPanelContent;

/// A clickable palette swatch carrying its brush index.
#[derive(Component, Default, Clone, Copy)]
struct PaletteSwatch(u32);

/// A grid-resize button: applies `(dw, dh)` to the selected tilemap's dimensions.
#[derive(Component, Default, Clone, Copy)]
struct TilemapAdjust {
    dw: i32,
    dh: i32,
}

/// The tab body: a scrollable container the panel is rebuilt into.
pub fn tilemap_body() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(10), px(8)),
            row_gap: px(8),
            overflow: Overflow::scroll_y(),
        }
        ThemeBackgroundColor(tokens::PANE_BODY_BG)
        ScrollArea
        TilemapPanelContent
    }
}

/// The fill color shown for a palette swatch (`0` = the erase slot).
fn swatch_fill(index: u32) -> Color {
    if index == 0 {
        Color::srgb(0.22, 0.22, 0.26)
    } else {
        palette_color(index)
    }
}

/// One palette swatch: a colored chip ringed in the accent when it's the active brush.
fn palette_swatch(index: u32, active: bool) -> impl Scene {
    let ring = if active {
        etokens::INFO
    } else {
        etokens::PANEL_BORDER
    };
    let fill = swatch_fill(index);
    let caption = if index == 0 {
        "∅".to_string()
    } else {
        index.to_string()
    };
    bsn! {
        Node {
            width: px(32),
            height: px(32),
            padding: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(6)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        ThemeBackgroundColor(ring)
        PaletteSwatch(index)
        Children [
            (
                Node {
                    flex_grow: 1.0,
                    align_self: bevy_ui::AlignSelf::Stretch,
                    border_radius: BorderRadius::all(px(4)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                }
                BackgroundColor(fill)
                Pickable::IGNORE
                Children [ (label_small(caption) Pickable::IGNORE) ]
            )
        ]
    }
}

/// The full 9-slot palette row (erase + 8 colors), highlighting `active`.
fn palette_row(active: u32) -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(6),
            row_gap: px(6),
            flex_wrap: bevy_ui::FlexWrap::Wrap,
        }
        Children [
            (palette_swatch(0, active == 0)),
            (palette_swatch(1, active == 1)),
            (palette_swatch(2, active == 2)),
            (palette_swatch(3, active == 3)),
            (palette_swatch(4, active == 4)),
            (palette_swatch(5, active == 5)),
            (palette_swatch(6, active == 6)),
            (palette_swatch(7, active == 7)),
            (palette_swatch(8, active == 8)),
        ]
    }
}

/// A width/height adjust button.
fn adjust_btn(caption: &'static str, dw: i32, dh: i32) -> impl Scene {
    let cap = caption.to_string();
    bsn! {
        (@FeathersButton {
            @variant: ButtonVariant::Normal,
            @caption: bsn! { (Text(cap) ThemedText) }
        } TilemapAdjust { dw: dw, dh: dh })
    }
}

/// The grid-resize controls for the selected map.
fn resize_controls() -> impl Scene {
    bsn! {
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(6),
        }
        Children [
            (label_small("Width")),
            adjust_btn("−", -1, 0),
            adjust_btn("+", 1, 0),
            (Node { width: px(10) }),
            (label_small("Height")),
            adjust_btn("−", 0, -1),
            adjust_btn("+", 0, 1),
        ]
    }
}

/// Rebuild the panel when the brush, selection, or any tilemap changes.
fn rebuild_tilemap_panel(
    paint: Res<TilePaint>,
    selection: Res<EditorSelection>,
    maps: Query<&Tilemap>,
    changed_maps: Query<(), Changed<Tilemap>>,
    container: Query<Entity, With<TilemapPanelContent>>,
    mut commands: Commands,
) {
    let dirty =
        paint.is_changed() || selection.is_changed() || changed_maps.iter().next().is_some();
    if !dirty {
        return;
    }
    let Ok(content) = container.single() else {
        return;
    };

    let brush = if paint.index == 0 {
        "Brush: Erase".to_string()
    } else {
        format!("Brush: {}", paint.index)
    };

    let mut rows: Vec<Box<dyn SceneList>> = vec![
        Box::new(EntityScene(section_header(
            "Tile Palette".to_string(),
            bsn! { Node {} },
        ))),
        Box::new(EntityScene(palette_row(paint.index))),
        Box::new(EntityScene(bsn! {
            Node { padding: UiRect::axes(px(2), px(2)) }
            Children [ (label_dim(brush)) ]
        })),
    ];

    let selected_map = selection
        .primary
        .and_then(|e| maps.get(e).ok().map(|m| (m.width, m.height, m.tile_size)));
    match selected_map {
        Some((w, h, ts)) => {
            rows.push(Box::new(EntityScene(section_header(
                "Grid".to_string(),
                bsn! { Node {} },
            ))));
            rows.push(Box::new(EntityScene(bsn! {
                Node { padding: UiRect::axes(px(2), px(2)) }
                Children [ (label_small(format!("{w} × {h}  ·  tile {ts}"))) ]
            })));
            rows.push(Box::new(EntityScene(resize_controls())));
            rows.push(Box::new(EntityScene(bsn! {
                Node { padding: UiRect::axes(px(2), px(4)) }
                Children [ (label_dim("Click a tile in the viewport to paint it.".to_string())) ]
            })));
        }
        None => {
            rows.push(Box::new(EntityScene(bsn! {
                Node { padding: UiRect::axes(px(2), px(6)) }
                Children [ (label_dim("Select a Tilemap to edit its grid  (GameObject ▸ Tilemap).".to_string())) ]
            })));
        }
    }

    commands.entity(content).despawn_children();
    commands
        .entity(content)
        .queue_spawn_related_scenes::<Children>(rows);
}

/// Click a palette swatch → set the active brush.
fn on_palette_click(
    click: On<Pointer<Click>>,
    swatches: Query<&PaletteSwatch>,
    mut paint: ResMut<TilePaint>,
) {
    if let Ok(swatch) = swatches.get(click.entity) {
        paint.index = swatch.0.min(PALETTE_LEN - 1);
    }
}

/// Resize the selected tilemap, preserving overlapping cells.
fn on_tilemap_adjust(
    act: On<Activate>,
    adjusts: Query<&TilemapAdjust>,
    selection: Res<EditorSelection>,
    mut maps: Query<&mut Tilemap>,
) {
    let Ok(adj) = adjusts.get(act.entity) else {
        return;
    };
    let Some(entity) = selection.primary else {
        return;
    };
    let Ok(mut map) = maps.get_mut(entity) else {
        return;
    };
    let new_w = (map.width as i32 + adj.dw).clamp(1, 64) as u32;
    let new_h = (map.height as i32 + adj.dh).clamp(1, 64) as u32;
    if new_w == map.width && new_h == map.height {
        return;
    }
    let (ow, oh) = (map.width, map.height);
    map.tiles = resized_tiles(&map.tiles, ow, oh, new_w, new_h);
    map.width = new_w;
    map.height = new_h;
}

/// Resize a row-major tile grid, copying every cell that exists in both the old and new bounds
/// (so growing/shrinking the grid doesn't scramble existing painting).
fn resized_tiles(old: &[u32], old_w: u32, old_h: u32, new_w: u32, new_h: u32) -> Vec<u32> {
    let mut out = vec![0u32; (new_w as usize) * (new_h as usize)];
    for y in 0..old_h.min(new_h) {
        for x in 0..old_w.min(new_w) {
            let oi = (y * old_w + x) as usize;
            let ni = (y * new_w + x) as usize;
            if let Some(v) = old.get(oi) {
                out[ni] = *v;
            }
        }
    }
    out
}

/// Installs the tilemap editor panel.
pub struct TilemapEditorPlugin;

impl Plugin for TilemapEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, rebuild_tilemap_panel)
            .add_observer(on_palette_click)
            .add_observer(on_tilemap_adjust);
    }
}

#[cfg(test)]
mod tests {
    use super::resized_tiles;

    #[test]
    fn resize_preserves_overlap_and_zero_fills() {
        // 3x2 grid, row-major: indices 0..6.
        let old = vec![1, 2, 3, 4, 5, 6];
        // Grow to 4x3: overlapping cells copy, new cells are 0.
        let grown = resized_tiles(&old, 3, 2, 4, 3);
        assert_eq!(grown.len(), 12);
        assert_eq!(grown[0], 1, "cell (0,0) preserved");
        assert_eq!(grown[2], 3, "cell (2,0) preserved");
        // Old cell (0,1) had value 4; at the new 4-wide stride it lands at index 1*4+0 = 4.
        assert_eq!(grown[4], 4, "cell (0,1) preserved at new stride");
        assert_eq!(grown[3], 0, "new column is empty");
        assert_eq!(grown[8], 0, "new row is empty");
    }

    #[test]
    fn resize_shrink_drops_outside_cells() {
        let old = vec![1, 2, 3, 4, 5, 6]; // 3x2
        let small = resized_tiles(&old, 3, 2, 2, 2);
        assert_eq!(small, vec![1, 2, 4, 5], "keeps the 2x2 top-left block");
    }
}
