//! The **Material** bottom-dock tab: edit the selected entity's `StandardMaterial` asset live
//! with sliders for base-color R/G/B/A, metallic, and perceptual roughness. Edits write straight
//! into `Assets<StandardMaterial>`, so the viewport updates immediately. Sliders re-sync to the
//! material whenever the selection or active tab changes.

use bevy_app::{App, Plugin, Update};
use bevy_asset::Assets;
use bevy_color::{Color, Srgba};
use bevy_ecs::prelude::*;
use bevy_feathers::controls::FeathersSlider;
use bevy_feathers::display::{label, label_dim};
use bevy_pbr::{MeshMaterial3d, StandardMaterial};
use bevy_scene::prelude::*;
use bevy_ui::{px, AlignItems, Display, FlexDirection, Node, Overflow, UiRect};
use bevy_ui_widgets::{SliderValue, ValueChange};

use crate::state::EditorSelection;
use crate::ui::{BottomDock, BottomTab};

/// Which `StandardMaterial` property a slider drives.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum MatProp {
    #[default]
    R,
    G,
    B,
    A,
    Metallic,
    Roughness,
}

/// Marks a material-editor slider with the property it controls.
#[derive(Component, Default, Clone, Copy)]
struct MatSlider(MatProp);

/// Installs the material editor.
pub struct MaterialEditorPlugin;

impl Plugin for MaterialEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_material_sliders)
            .add_observer(on_mat_slider);
    }
}

/// The Material tab body: a labeled slider per editable property.
pub fn material_body() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(8), px(6)),
            row_gap: px(2),
            overflow: Overflow::scroll_y(),
        }
        bevy_feathers::theme::ThemeBackgroundColor(bevy_feathers::tokens::PANE_BODY_BG)
        bevy_ui_widgets::ScrollArea
        Children [
            (label_dim("StandardMaterial of the selection")),
            (mat_row("Red", MatProp::R)),
            (mat_row("Green", MatProp::G)),
            (mat_row("Blue", MatProp::B)),
            (mat_row("Alpha", MatProp::A)),
            (mat_row("Metallic", MatProp::Metallic)),
            (mat_row("Roughness", MatProp::Roughness)),
        ]
    }
}

fn mat_row(name: &str, prop: MatProp) -> impl Scene {
    let label_text = name.to_string();
    bsn! {
        (
            Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(8), min_height: px(26) }
            Children [
                (Node { width: px(80) } Children [ (label(label_text)) ]),
                (@FeathersSlider { @value: 0.0, @min: 0.0, @max: 1.0 }
                    template_value(MatSlider(prop))
                    Node { flex_grow: 1.0 }),
            ]
        )
    }
}

fn on_mat_slider(
    change: On<ValueChange<f32>>,
    sliders: Query<&MatSlider>,
    selection: Res<EditorSelection>,
    mats: Query<&MeshMaterial3d<StandardMaterial>>,
    mut assets: ResMut<Assets<StandardMaterial>>,
) {
    let Ok(slider) = sliders.get(change.source) else {
        return;
    };
    let Some(entity) = selection.primary else {
        return;
    };
    let Ok(handle) = mats.get(entity) else {
        return;
    };
    let Some(mut mat) = assets.get_mut(&handle.0) else {
        return;
    };
    apply_mat_prop(&mut mat, slider.0, change.value);
}

fn apply_mat_prop(mat: &mut StandardMaterial, prop: MatProp, v: f32) {
    match prop {
        MatProp::Metallic => mat.metallic = v,
        MatProp::Roughness => mat.perceptual_roughness = v,
        channel => {
            let mut s = mat.base_color.to_srgba();
            match channel {
                MatProp::R => s.red = v,
                MatProp::G => s.green = v,
                MatProp::B => s.blue = v,
                MatProp::A => s.alpha = v,
                _ => {}
            }
            mat.base_color = Color::Srgba(s);
        }
    }
}

/// Re-seed the sliders from the selected material when the selection or active tab changes.
/// `SliderValue` is an immutable component, so updated values are re-inserted via commands.
fn sync_material_sliders(
    dock: Res<BottomDock>,
    selection: Res<EditorSelection>,
    mats: Query<&MeshMaterial3d<StandardMaterial>>,
    assets: Res<Assets<StandardMaterial>>,
    sliders: Query<(Entity, &MatSlider, &SliderValue)>,
    mut commands: Commands,
) {
    if !(dock.open && dock.active == BottomTab::Material) {
        return;
    }
    if !(selection.is_changed() || dock.is_changed()) {
        return;
    }
    let Some(mat) = selection
        .primary
        .and_then(|e| mats.get(e).ok())
        .and_then(|h| assets.get(&h.0))
    else {
        return;
    };
    let s: Srgba = mat.base_color.to_srgba();
    for (entity, slider, value) in sliders.iter() {
        let want = match slider.0 {
            MatProp::R => s.red,
            MatProp::G => s.green,
            MatProp::B => s.blue,
            MatProp::A => s.alpha,
            MatProp::Metallic => mat.metallic,
            MatProp::Roughness => mat.perceptual_roughness,
        };
        if (value.0 - want).abs() > 1e-4 {
            commands.entity(entity).insert(SliderValue(want));
        }
    }
}
