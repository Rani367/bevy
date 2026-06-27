//! The inspector panel: a generic, reflection-driven view of the selected entity's
//! components. It enumerates the entity's components via the type registry, walks each
//! component's fields with `bevy_reflect`, and renders an editable widget per field
//! (numeric fields become Feathers number inputs; everything else is shown read-only for
//! now). Edits are written back through `ReflectComponent::reflect_mut` + a reflect path,
//! and numeric widgets are kept in sync when the entity changes elsewhere (e.g. a gizmo
//! drag).

use core::any::TypeId;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_ecs::reflect::{AppTypeRegistry, ReflectComponent};
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_feathers::controls::{FeathersNumberInput, NumberInputPrecision, NumberInputValue};
use bevy_feathers::display::{label, label_dim, label_small};
use bevy_feathers::theme::ThemeBackgroundColor;
use bevy_feathers::tokens;
use bevy_input_focus::InputFocus;
use bevy_reflect::{GetPath, PartialReflect, ReflectRef};
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_ui::{px, AlignItems, Display, FlexDirection, Node, UiRect};
use bevy_ui_widgets::ValueChange;

use crate::state::EditorSelection;
use crate::ui::InspectorContent;

/// Which editable kind a [`FieldBinding`] represents (only numbers are editable for now).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FieldKind {
    /// An `f32`-valued field bound to a number input.
    F32,
    /// A read-only field.
    #[default]
    ReadOnly,
}

/// Placed on an editable field widget; records how to write the widget's value back
/// into the world via reflection.
#[derive(Component, Clone, Debug)]
pub struct FieldBinding {
    /// The scene entity owning the component.
    pub target: Entity,
    /// The component's type id.
    pub component: TypeId,
    /// The dotted reflect path to the field within the component (e.g. `translation.x`).
    pub path: String,
    /// The field kind.
    pub kind: FieldKind,
}

impl Default for FieldBinding {
    fn default() -> Self {
        Self {
            target: Entity::PLACEHOLDER,
            component: TypeId::of::<()>(),
            path: String::new(),
            kind: FieldKind::ReadOnly,
        }
    }
}

/// Set when the inspector should be rebuilt (selection changed).
#[derive(Resource, Default)]
struct InspectorDirty(bool);

/// Installs the inspector systems and write-back observer.
pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InspectorDirty>()
            .add_systems(
                Update,
                (
                    (mark_inspector_dirty, rebuild_inspector).chain(),
                    sync_number_fields,
                ),
            )
            .add_observer(on_number_changed);
    }
}

// ---------------------------------------------------------------------------
// Model collection
// ---------------------------------------------------------------------------

/// A single inspectable field.
struct FieldModel {
    label: String,
    path: String,
    value: FieldValue,
}

enum FieldValue {
    F32(f32),
    ReadOnly(String),
}

/// A component section: a name and its fields.
struct ComponentModel {
    name: String,
    type_id: TypeId,
    fields: Vec<FieldModel>,
}

/// Flag a rebuild whenever the selection changes.
fn mark_inspector_dirty(selection: Res<EditorSelection>, mut dirty: ResMut<InspectorDirty>) {
    if selection.is_changed() {
        dirty.0 = true;
    }
}

/// Rebuild the inspector UI for the primary selection. Exclusive so it can read any
/// component generically through the type registry and reflection.
fn rebuild_inspector(world: &mut World) {
    if !world.resource::<InspectorDirty>().0 {
        return;
    }
    world.resource_mut::<InspectorDirty>().0 = false;

    let mut content_q = world.query_filtered::<Entity, With<InspectorContent>>();
    let Ok(content) = content_q.single(world) else {
        return;
    };

    let primary = world.resource::<EditorSelection>().primary;
    let components = primary
        .map(|entity| collect_components(world, entity))
        .unwrap_or_default();

    // Despawn the old field widgets, then spawn the new ones.
    world.entity_mut(content).despawn_children();

    let mut scenes: Vec<Box<dyn SceneList>> = Vec::new();
    if components.is_empty() {
        scenes.push(Box::new(EntityScene(empty_hint())));
    }
    for comp in &components {
        scenes.push(Box::new(EntityScene(component_header(comp.name.clone()))));
        for field in &comp.fields {
            let boxed: Box<dyn SceneList> = match &field.value {
                FieldValue::F32(v) => Box::new(EntityScene(number_field(
                    field.label.clone(),
                    primary.unwrap(),
                    comp.type_id,
                    field.path.clone(),
                    *v,
                ))),
                FieldValue::ReadOnly(text) => Box::new(EntityScene(readonly_field(
                    field.label.clone(),
                    text.clone(),
                ))),
            };
            scenes.push(boxed);
        }
    }

    world
        .entity_mut(content)
        .queue_spawn_related_scenes::<Children>(scenes);
}

/// Enumerate the entity's reflectable components and walk their fields.
fn collect_components(world: &World, entity: Entity) -> Vec<ComponentModel> {
    let type_ids: Vec<TypeId> = match world.inspect_entity(entity) {
        Ok(infos) => infos
            .filter_map(bevy_ecs::component::ComponentInfo::type_id)
            .collect(),
        Err(_) => return Vec::new(),
    };

    let registry_arc = world.resource::<AppTypeRegistry>().0.clone();
    let registry = registry_arc.read();
    let Ok(entity_ref) = world.get_entity(entity) else {
        return Vec::new();
    };

    let mut components = Vec::new();
    for type_id in type_ids {
        let Some(registration) = registry.get(type_id) else {
            continue;
        };
        let Some(reflect_component) = registration.data::<ReflectComponent>() else {
            continue;
        };
        let Some(reflected) = reflect_component.reflect(entity_ref) else {
            continue;
        };
        let name = registration
            .type_info()
            .type_path_table()
            .short_path()
            .to_string();
        let mut fields = Vec::new();
        collect_component_fields(reflected.as_partial_reflect(), &mut fields);
        components.push(ComponentModel {
            name,
            type_id,
            fields,
        });
    }
    components
}

/// Walk a component's top-level fields into [`FieldModel`]s.
fn collect_component_fields(value: &dyn PartialReflect, out: &mut Vec<FieldModel>) {
    match value.reflect_ref() {
        ReflectRef::Struct(s) => {
            for i in 0..s.field_len() {
                let name = s.name_at(i).unwrap_or("?").to_string();
                if let Some(field) = s.field_at(i) {
                    push_field(field, &name, &name, 0, out);
                }
            }
        }
        _ => {
            out.push(FieldModel {
                label: "value".into(),
                path: String::new(),
                value: FieldValue::ReadOnly(format!("{value:?}")),
            });
        }
    }
}

/// Push a single field (recursing into nested structs like `Vec3`/`Quat`).
fn push_field(
    field: &dyn PartialReflect,
    path: &str,
    label: &str,
    depth: usize,
    out: &mut Vec<FieldModel>,
) {
    if let Some(v) = field.try_downcast_ref::<f32>() {
        out.push(FieldModel {
            label: label.into(),
            path: path.into(),
            value: FieldValue::F32(*v),
        });
        return;
    }
    if let Some(v) = field.try_downcast_ref::<f64>() {
        out.push(FieldModel {
            label: label.into(),
            path: path.into(),
            value: FieldValue::F32(*v as f32),
        });
        return;
    }

    if depth < 3
        && let ReflectRef::Struct(s) = field.reflect_ref()
    {
        for i in 0..s.field_len() {
            let child_name = s.name_at(i).unwrap_or("?");
            if let Some(child) = s.field_at(i) {
                let child_path = format!("{path}.{child_name}");
                push_field(child, &child_path, child_name, depth + 1, out);
            }
        }
        return;
    }

    out.push(FieldModel {
        label: label.into(),
        path: path.into(),
        value: FieldValue::ReadOnly(format!("{field:?}")),
    });
}

// ---------------------------------------------------------------------------
// Field widgets
// ---------------------------------------------------------------------------

fn empty_hint() -> impl Scene {
    bsn! {
        Node {
            padding: UiRect::axes(px(6), px(4)),
        }
        Children [ label_dim("No entity selected") ]
    }
}

fn component_header(name: String) -> impl Scene {
    bsn! {
        Node {
            min_height: px(22),
            padding: UiRect::axes(px(6), px(3)),
            align_items: AlignItems::Center,
        }
        ThemeBackgroundColor(tokens::GROUP_HEADER_BG)
        Children [ label(name) ]
    }
}

fn number_field(
    field_label: String,
    target: Entity,
    component: TypeId,
    path: String,
    value: f32,
) -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(6),
            padding: UiRect::axes(px(6), px(2)),
        }
        Children [
            (Node { width: px(110) } Children [ label_small(field_label) ]),
            (
                @FeathersNumberInput
                template_value(NumberInputValue::F32(value))
                NumberInputPrecision(3)
                FieldBinding { target: target, component: component, path: path, kind: FieldKind::F32 }
                Node { flex_grow: 1.0, max_width: px(150) }
            ),
        ]
    }
}

fn readonly_field(field_label: String, text: String) -> impl Scene {
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(6),
            padding: UiRect::axes(px(6), px(2)),
        }
        Children [
            (Node { width: px(110) } Children [ label_small(field_label) ]),
            label_dim(text),
        ]
    }
}

// ---------------------------------------------------------------------------
// Write-back and sync
// ---------------------------------------------------------------------------

/// When a bound number input changes, write the new value back into the world.
fn on_number_changed(
    change: On<ValueChange<f32>>,
    bindings: Query<&FieldBinding>,
    mut commands: Commands,
) {
    let Ok(binding) = bindings.get(change.source) else {
        return;
    };
    let target = binding.target;
    let component = binding.component;
    let path = binding.path.clone();
    let value = change.value;
    commands.queue(move |world: &mut World| {
        write_f32_field(world, target, component, &path, value);
    });
}

/// Apply an `f32` value to a component field via reflection.
fn write_f32_field(world: &mut World, target: Entity, component: TypeId, path: &str, value: f32) {
    let reflect_component = {
        let registry_arc = world.resource::<AppTypeRegistry>().0.clone();
        let registry = registry_arc.read();
        let Some(registration) = registry.get(component) else {
            return;
        };
        match registration.data::<ReflectComponent>() {
            Some(rc) => rc.clone(),
            None => return,
        }
    };
    if world.get_entity(target).is_err() {
        return;
    }
    if let Some(mut reflected) = reflect_component.reflect_mut(world.entity_mut(target))
        && let Ok(field) = reflected.path_mut::<f32>(path)
    {
        *field = value;
    }
}

/// Keep number inputs in sync with the world when the bound entity changes elsewhere
/// (e.g. a gizmo drag). Skips the focused widget so it doesn't clobber typing.
fn sync_number_fields(world: &mut World) {
    let focus = world.resource::<InputFocus>().get();

    let mut binding_q = world.query::<(Entity, &FieldBinding)>();
    let items: Vec<(Entity, Entity, TypeId, String)> = binding_q
        .iter(world)
        .filter(|(_, b)| b.kind == FieldKind::F32)
        .map(|(widget, b)| (widget, b.target, b.component, b.path.clone()))
        .collect();
    if items.is_empty() {
        return;
    }

    let registry_arc = world.resource::<AppTypeRegistry>().0.clone();
    let mut updates: Vec<(Entity, f32)> = Vec::new();
    {
        let registry = registry_arc.read();
        for (widget, target, type_id, path) in &items {
            if Some(*widget) == focus {
                continue;
            }
            let Some(registration) = registry.get(*type_id) else {
                continue;
            };
            let Some(reflect_component) = registration.data::<ReflectComponent>() else {
                continue;
            };
            let Ok(entity_ref) = world.get_entity(*target) else {
                continue;
            };
            if let Some(reflected) = reflect_component.reflect(entity_ref)
                && let Ok(v) = reflected.path::<f32>(path.as_str())
            {
                updates.push((*widget, *v));
            }
        }
    }

    for (widget, value) in updates {
        if let Ok(mut widget_mut) = world.get_entity_mut(widget) {
            widget_mut.insert(NumberInputValue::F32(value));
        }
    }
}
