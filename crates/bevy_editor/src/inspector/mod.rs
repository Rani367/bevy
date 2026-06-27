//! The inspector panel: a generic, reflection-driven view of the selected entity's
//! components. It enumerates the entity's components via the type registry, walks each
//! component's fields with `bevy_reflect`, and renders an editable widget per field:
//! numbers (`f32`/`f64`/integers) become Feathers number inputs, `bool`s become
//! checkboxes, `String`s become text inputs, unit enums become a cycle button, and
//! `Color` is split into editable R/G/B/A channels. Edits are written back through
//! `ReflectComponent::reflect_mut` + a reflect path, and a top "＋ Add Component" button
//! plus per-section "✕" buttons add/remove components by reflection.

use core::any::TypeId;

use bevy_app::{App, Plugin, Update};
use bevy_color::Color;
use bevy_ecs::prelude::*;
use bevy_ecs::reflect::{AppTypeRegistry, ReflectComponent};
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_feathers::controls::{
    ButtonVariant, FeathersButton, FeathersCheckbox, FeathersNumberInput, FeathersTextInput,
    FeathersTextInputContainer, NumberInputPrecision, NumberInputValue,
};
use bevy_feathers::display::{label, label_dim, label_small};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy_feathers::tokens;
use bevy_input_focus::InputFocus;
use bevy_picking::events::{Click, Pointer};
use bevy_reflect::enums::{DynamicEnum, DynamicVariant, VariantInfo};
use bevy_reflect::std_traits::ReflectDefault;
use bevy_reflect::{GetPath, PartialReflect, Reflect, ReflectRef, TypeInfo};
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_text::EditableText;
use bevy_ui::widget::Text;
use bevy_ui::{
    percent, px, AlignItems, Checked, Display, FlexDirection, GlobalZIndex, Node, Overflow,
    PositionType, UiRect,
};
use bevy_ui_widgets::{Activate, ScrollArea, ValueChange};

use crate::markers::EditorEntity;
use crate::state::EditorSelection;
use crate::ui::{InspectorContent, SeedText};
use crate::undo::push_undo;

/// A numeric field's concrete type, so edits can be written back to the right Rust type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NumTy {
    F32,
    F64,
    I32,
    I64,
    U32,
    U64,
    Usize,
    ColorR,
    ColorG,
    ColorB,
    ColorA,
}

impl NumTy {
    fn is_integer(self) -> bool {
        matches!(
            self,
            NumTy::I32 | NumTy::I64 | NumTy::U32 | NumTy::U64 | NumTy::Usize
        )
    }
}

/// Which editable kind a [`FieldBinding`] represents.
#[derive(Clone, PartialEq, Debug, Default)]
enum FieldKind {
    Num(NumTy),
    Bool,
    Str,
    Enum,
    #[default]
    ReadOnly,
}

/// Placed on an editable field widget; records how to write the widget's value back into
/// the world via reflection.
#[derive(Component, Clone, Debug)]
pub struct FieldBinding {
    target: Entity,
    component: TypeId,
    path: String,
    kind: FieldKind,
    /// For [`FieldKind::Enum`]: the ordered list of variant names to cycle through.
    variants: Vec<String>,
}

impl Default for FieldBinding {
    fn default() -> Self {
        Self {
            target: Entity::PLACEHOLDER,
            component: TypeId::of::<()>(),
            path: String::new(),
            kind: FieldKind::ReadOnly,
            variants: Vec::new(),
        }
    }
}

/// A button in the "Add Component" dialog; carries the component type to add.
#[derive(Component, Clone)]
struct AddComponentButton(TypeId);

impl Default for AddComponentButton {
    fn default() -> Self {
        Self(TypeId::of::<()>())
    }
}

/// A per-section "remove component" button.
#[derive(Component, Clone)]
struct RemoveComponentButton {
    target: Entity,
    type_id: TypeId,
}

impl Default for RemoveComponentButton {
    fn default() -> Self {
        Self {
            target: Entity::PLACEHOLDER,
            type_id: TypeId::of::<()>(),
        }
    }
}

/// Sets a checkbox's initial `Checked` state after spawn (bsn can't add it conditionally).
#[derive(Component, Clone, Copy, Default)]
struct InitChecked(bool);

/// Marks the "Add Component" overlay backdrop, and the list container within it.
#[derive(Component, Default, Clone, Copy)]
struct InspectorOverlay;
#[derive(Component, Default, Clone, Copy)]
struct AddComponentList;

/// Request to open the "Add Component" dialog.
#[derive(Event, Clone, Copy)]
struct OpenAddComponentDialog;
/// Request to close the inspector overlay.
#[derive(Event, Clone, Copy)]
struct CloseInspectorOverlay;

/// Set when the inspector should be rebuilt (selection changed or components changed).
#[derive(Resource, Default)]
struct InspectorDirty(bool);

/// Tracks which inspector widget is mid-edit, so undo captures one entry per
/// field-editing session rather than one per keystroke / scrub step.
#[derive(Resource, Default)]
struct InspectorEditSession {
    widget: Option<Entity>,
}

/// Installs the inspector systems and write-back observers.
pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InspectorDirty>()
            .init_resource::<InspectorEditSession>()
            .add_systems(
                Update,
                (
                    (mark_inspector_dirty, rebuild_inspector).chain(),
                    sync_number_fields,
                    commit_string_fields,
                    apply_init_checked,
                ),
            )
            .add_observer(on_f32_changed)
            .add_observer(on_f64_changed)
            .add_observer(on_i64_changed)
            .add_observer(on_bool_changed)
            .add_observer(on_enum_cycle)
            .add_observer(on_open_add_component)
            .add_observer(on_add_component_button)
            .add_observer(on_remove_component_button)
            .add_observer(on_close_inspector_overlay);
    }
}

// ---------------------------------------------------------------------------
// Model collection
// ---------------------------------------------------------------------------

struct FieldModel {
    label: String,
    path: String,
    value: FieldValue,
}

enum FieldValue {
    Num {
        value: f64,
        ty: NumTy,
    },
    Bool(bool),
    Str(String),
    Enum {
        current: String,
        variants: Vec<String>,
    },
    ReadOnly(String),
}

struct ComponentModel {
    name: String,
    type_id: TypeId,
    fields: Vec<FieldModel>,
}

/// Flag a rebuild whenever the selection changes.
fn mark_inspector_dirty(
    selection: Res<EditorSelection>,
    mut dirty: ResMut<InspectorDirty>,
    mut session: ResMut<InspectorEditSession>,
) {
    if selection.is_changed() {
        dirty.0 = true;
        session.widget = None;
    }
}

/// Rebuild the inspector UI for the primary selection.
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

    world.entity_mut(content).despawn_children();

    let mut scenes: Vec<Box<dyn SceneList>> = Vec::new();
    if let Some(target) = primary {
        scenes.push(Box::new(EntityScene(add_component_bar())));
        if components.is_empty() {
            scenes.push(Box::new(EntityScene(empty_hint())));
        }
        for comp in &components {
            scenes.push(Box::new(EntityScene(component_header(
                comp.name.clone(),
                target,
                comp.type_id,
            ))));
            for field in &comp.fields {
                scenes.push(field_widget(field, target, comp.type_id));
            }
        }
    } else {
        scenes.push(Box::new(EntityScene(empty_hint())));
    }

    world
        .entity_mut(content)
        .queue_spawn_related_scenes::<Children>(scenes);
}

/// Build the boxed widget scene for a single field.
fn field_widget(field: &FieldModel, target: Entity, component: TypeId) -> Box<dyn SceneList> {
    match &field.value {
        FieldValue::Num { value, ty } => Box::new(EntityScene(number_field(
            field.label.clone(),
            target,
            component,
            field.path.clone(),
            *ty,
            *value,
        ))),
        FieldValue::Bool(v) => Box::new(EntityScene(bool_field(
            field.label.clone(),
            target,
            component,
            field.path.clone(),
            *v,
        ))),
        FieldValue::Str(text) => Box::new(EntityScene(string_field(
            field.label.clone(),
            target,
            component,
            field.path.clone(),
            text.clone(),
        ))),
        FieldValue::Enum { current, variants } => Box::new(EntityScene(enum_field(
            field.label.clone(),
            target,
            component,
            field.path.clone(),
            current.clone(),
            variants.clone(),
        ))),
        FieldValue::ReadOnly(text) => Box::new(EntityScene(readonly_field(
            field.label.clone(),
            text.clone(),
        ))),
    }
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
        // A component that *is* an enum (e.g. `Visibility`): one field at the empty path.
        ReflectRef::Enum(_) => push_field(value, "", "value", 0, out),
        _ => out.push(FieldModel {
            label: "value".into(),
            path: String::new(),
            value: FieldValue::ReadOnly(format!("{value:?}")),
        }),
    }
}

/// Push a single field, recursing into nested structs and mapping known leaf types to
/// editable widgets.
fn push_field(
    field: &dyn PartialReflect,
    path: &str,
    label: &str,
    depth: usize,
    out: &mut Vec<FieldModel>,
) {
    macro_rules! num {
        ($ty:ty, $tag:expr) => {
            if let Some(v) = field.try_downcast_ref::<$ty>() {
                out.push(FieldModel {
                    label: label.into(),
                    path: path.into(),
                    value: FieldValue::Num {
                        value: *v as f64,
                        ty: $tag,
                    },
                });
                return;
            }
        };
    }
    num!(f32, NumTy::F32);
    num!(f64, NumTy::F64);
    num!(i32, NumTy::I32);
    num!(i64, NumTy::I64);
    num!(u32, NumTy::U32);
    num!(u64, NumTy::U64);
    num!(usize, NumTy::Usize);

    if let Some(v) = field.try_downcast_ref::<bool>() {
        out.push(FieldModel {
            label: label.into(),
            path: path.into(),
            value: FieldValue::Bool(*v),
        });
        return;
    }
    if let Some(v) = field.try_downcast_ref::<String>() {
        out.push(FieldModel {
            label: label.into(),
            path: path.into(),
            value: FieldValue::Str(v.clone()),
        });
        return;
    }
    // Color: editable R/G/B/A channels, each writing the whole color at this path.
    if let Some(c) = field.try_downcast_ref::<Color>() {
        let s = c.to_srgba();
        for (suffix, chan, val) in [
            ("r", NumTy::ColorR, s.red),
            ("g", NumTy::ColorG, s.green),
            ("b", NumTy::ColorB, s.blue),
            ("a", NumTy::ColorA, s.alpha),
        ] {
            out.push(FieldModel {
                label: format!("{label}.{suffix}"),
                path: path.into(),
                value: FieldValue::Num {
                    value: val as f64,
                    ty: chan,
                },
            });
        }
        return;
    }

    // Unit-only enums (e.g. `Visibility`): a cycle button.
    if let ReflectRef::Enum(e) = field.reflect_ref()
        && let Some(TypeInfo::Enum(info)) = field.get_represented_type_info()
        && info.iter().all(|v| matches!(v, VariantInfo::Unit(_)))
    {
        out.push(FieldModel {
            label: label.into(),
            path: path.into(),
            value: FieldValue::Enum {
                current: e.variant_name().to_string(),
                variants: info
                    .variant_names()
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            },
        });
        return;
    }

    if depth < 3
        && let ReflectRef::Struct(s) = field.reflect_ref()
    {
        for i in 0..s.field_len() {
            let child_name = s.name_at(i).unwrap_or("?");
            if let Some(child) = s.field_at(i) {
                let child_path = if path.is_empty() {
                    child_name.to_string()
                } else {
                    format!("{path}.{child_name}")
                };
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
        Node { padding: UiRect::axes(px(6), px(4)) }
        Children [ label_dim("No entity selected") ]
    }
}

/// The "＋ Add Component" bar at the top of the inspector.
fn add_component_bar() -> impl Scene {
    bsn! {
        Node {
            padding: UiRect::axes(px(6), px(4)),
        }
        Children [
            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("+ Add Component") ThemedText } }
                on(|_: On<Activate>, mut c: Commands| { c.trigger(OpenAddComponentDialog); })),
        ]
    }
}

fn component_header(name: String, target: Entity, type_id: TypeId) -> impl Scene {
    bsn! {
        Node {
            min_height: px(22),
            padding: UiRect::axes(px(6), px(3)),
            align_items: AlignItems::Center,
            justify_content: bevy_ui::JustifyContent::SpaceBetween,
        }
        ThemeBackgroundColor(tokens::GROUP_HEADER_BG)
        Children [
            label(name),
            (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { Text("x") ThemedText } }
                RemoveComponentButton { target: target, type_id: type_id }),
        ]
    }
}

fn field_row(field_label: String, inner: impl Scene) -> impl Scene {
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
            inner,
        ]
    }
}

fn number_field(
    field_label: String,
    target: Entity,
    component: TypeId,
    path: String,
    ty: NumTy,
    value: f64,
) -> impl Scene {
    let niv = match ty {
        NumTy::F64 => NumberInputValue::F64(value),
        NumTy::I32 | NumTy::I64 | NumTy::U32 | NumTy::U64 | NumTy::Usize => {
            NumberInputValue::I64(value as i64)
        }
        _ => NumberInputValue::F32(value as f32),
    };
    let precision = if ty.is_integer() { 0 } else { 3 };
    field_row(
        field_label,
        bsn! {
            (
                @FeathersNumberInput
                template_value(niv)
                NumberInputPrecision(precision)
                FieldBinding { target: target, component: component, path: path, kind: FieldKind::Num(ty), variants: Vec::new() }
                Node { flex_grow: 1.0, max_width: px(150) }
            )
        },
    )
}

fn bool_field(
    field_label: String,
    target: Entity,
    component: TypeId,
    path: String,
    value: bool,
) -> impl Scene {
    field_row(
        field_label,
        bsn! {
            (
                @FeathersCheckbox
                FieldBinding { target: target, component: component, path: path, kind: FieldKind::Bool, variants: Vec::new() }
                InitChecked(value)
            )
        },
    )
}

fn string_field(
    field_label: String,
    target: Entity,
    component: TypeId,
    path: String,
    value: String,
) -> impl Scene {
    field_row(
        field_label,
        bsn! {
            (@FeathersTextInputContainer
                Children [
                    (@FeathersTextInput
                        SeedText(value)
                        FieldBinding { target: target, component: component, path: path, kind: FieldKind::Str, variants: Vec::new() })
                ])
        },
    )
}

fn enum_field(
    field_label: String,
    target: Entity,
    component: TypeId,
    path: String,
    current: String,
    variants: Vec<String>,
) -> impl Scene {
    field_row(
        field_label,
        bsn! {
            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text(current) ThemedText } }
                FieldBinding { target: target, component: component, path: path, kind: FieldKind::Enum, variants: variants })
        },
    )
}

fn readonly_field(field_label: String, text: String) -> impl Scene {
    field_row(field_label, bsn! { label_dim(text) })
}

// ---------------------------------------------------------------------------
// Write-back
// ---------------------------------------------------------------------------

fn reflect_component_for(world: &World, component: TypeId) -> Option<ReflectComponent> {
    let registry_arc = world.resource::<AppTypeRegistry>().0.clone();
    let registry = registry_arc.read();
    registry.get(component)?.data::<ReflectComponent>().cloned()
}

/// Capture undo once per field-editing session (keyed by the edited widget).
fn begin_field_edit(widget: Entity, session: &mut InspectorEditSession, commands: &mut Commands) {
    if session.widget != Some(widget) {
        push_undo(commands);
        session.widget = Some(widget);
    }
}

fn on_f32_changed(
    change: On<ValueChange<f32>>,
    bindings: Query<&FieldBinding>,
    mut session: ResMut<InspectorEditSession>,
    mut commands: Commands,
) {
    let Ok(binding) = bindings.get(change.source) else {
        return;
    };
    let FieldKind::Num(ty) = binding.kind else {
        return;
    };
    begin_field_edit(change.source, &mut session, &mut commands);
    queue_numeric(&mut commands, binding, ty, change.value as f64);
}

fn on_f64_changed(
    change: On<ValueChange<f64>>,
    bindings: Query<&FieldBinding>,
    mut session: ResMut<InspectorEditSession>,
    mut commands: Commands,
) {
    let Ok(binding) = bindings.get(change.source) else {
        return;
    };
    let FieldKind::Num(ty) = binding.kind else {
        return;
    };
    begin_field_edit(change.source, &mut session, &mut commands);
    queue_numeric(&mut commands, binding, ty, change.value);
}

fn on_i64_changed(
    change: On<ValueChange<i64>>,
    bindings: Query<&FieldBinding>,
    mut session: ResMut<InspectorEditSession>,
    mut commands: Commands,
) {
    let Ok(binding) = bindings.get(change.source) else {
        return;
    };
    let FieldKind::Num(ty) = binding.kind else {
        return;
    };
    begin_field_edit(change.source, &mut session, &mut commands);
    queue_numeric(&mut commands, binding, ty, change.value as f64);
}

fn queue_numeric(commands: &mut Commands, binding: &FieldBinding, ty: NumTy, value: f64) {
    let (target, component, path) = (binding.target, binding.component, binding.path.clone());
    commands.queue(move |world: &mut World| {
        write_numeric(world, target, component, &path, ty, value);
    });
}

fn write_numeric(
    world: &mut World,
    target: Entity,
    component: TypeId,
    path: &str,
    ty: NumTy,
    value: f64,
) {
    let Some(rc) = reflect_component_for(world, component) else {
        return;
    };
    if world.get_entity(target).is_err() {
        return;
    }
    let Some(mut reflected) = rc.reflect_mut(world.entity_mut(target)) else {
        return;
    };
    match ty {
        NumTy::F32 => set_path(&mut *reflected, path, value as f32),
        NumTy::F64 => set_path(&mut *reflected, path, value),
        NumTy::I32 => set_path(&mut *reflected, path, value as i32),
        NumTy::I64 => set_path(&mut *reflected, path, value as i64),
        NumTy::U32 => set_path(&mut *reflected, path, value as u32),
        NumTy::U64 => set_path(&mut *reflected, path, value as u64),
        NumTy::Usize => set_path(&mut *reflected, path, value as usize),
        NumTy::ColorR | NumTy::ColorG | NumTy::ColorB | NumTy::ColorA => {
            if let Ok(color) = reflected.path_mut::<Color>(path) {
                let mut s = color.to_srgba();
                let v = value as f32;
                match ty {
                    NumTy::ColorR => s.red = v,
                    NumTy::ColorG => s.green = v,
                    NumTy::ColorB => s.blue = v,
                    _ => s.alpha = v,
                }
                *color = Color::srgba(s.red, s.green, s.blue, s.alpha);
            }
        }
    }
}

fn set_path<T: Reflect>(reflected: &mut dyn Reflect, path: &str, value: T) {
    if let Ok(field) = reflected.path_mut::<T>(path) {
        *field = value;
    }
}

fn on_bool_changed(
    change: On<ValueChange<bool>>,
    bindings: Query<&FieldBinding>,
    mut commands: Commands,
) {
    let Ok(binding) = bindings.get(change.source) else {
        return;
    };
    if binding.kind != FieldKind::Bool {
        return;
    }
    let (target, component, path) = (binding.target, binding.component, binding.path.clone());
    let value = change.value;
    push_undo(&mut commands);
    commands.queue(move |world: &mut World| {
        let Some(rc) = reflect_component_for(world, component) else {
            return;
        };
        if world.get_entity(target).is_err() {
            return;
        }
        if let Some(mut reflected) = rc.reflect_mut(world.entity_mut(target)) {
            set_path(&mut *reflected, &path, value);
        }
    });
}

fn on_enum_cycle(act: On<Activate>, bindings: Query<&FieldBinding>, mut commands: Commands) {
    let Ok(binding) = bindings.get(act.entity) else {
        return;
    };
    if binding.kind != FieldKind::Enum || binding.variants.is_empty() {
        return;
    }
    let (target, component, path) = (binding.target, binding.component, binding.path.clone());
    let variants = binding.variants.clone();
    push_undo(&mut commands);
    commands.queue(move |world: &mut World| {
        cycle_enum(world, target, component, &path, &variants);
    });
}

fn cycle_enum(
    world: &mut World,
    target: Entity,
    component: TypeId,
    path: &str,
    variants: &[String],
) {
    let Some(rc) = reflect_component_for(world, component) else {
        return;
    };
    let current = world.get_entity(target).ok().and_then(|entity_ref| {
        let reflected = rc.reflect(entity_ref)?;
        let field = reflected.reflect_path(path).ok()?;
        match field.reflect_ref() {
            ReflectRef::Enum(e) => Some(e.variant_name().to_string()),
            _ => None,
        }
    });
    let next = current
        .and_then(|c| variants.iter().position(|v| *v == c))
        .map(|i| (i + 1) % variants.len())
        .unwrap_or(0);
    let next_name = variants[next].clone();

    if let Some(mut reflected) = rc.reflect_mut(world.entity_mut(target))
        && let Ok(field) = reflected.reflect_path_mut(path)
    {
        field.apply(&DynamicEnum::new(next_name, DynamicVariant::Unit));
    }
}

/// Write back string fields whose text changed (text inputs have no `ValueChange`).
fn commit_string_fields(
    changed: Query<(&FieldBinding, &EditableText), Changed<EditableText>>,
    mut commands: Commands,
) {
    for (binding, editable) in changed.iter() {
        if binding.kind != FieldKind::Str {
            continue;
        }
        let value = editable.value().to_string();
        let (target, component, path) = (binding.target, binding.component, binding.path.clone());
        commands.queue(move |world: &mut World| {
            let Some(rc) = reflect_component_for(world, component) else {
                return;
            };
            if world.get_entity(target).is_err() {
                return;
            }
            if let Some(mut reflected) = rc.reflect_mut(world.entity_mut(target))
                && let Ok(field) = reflected.path_mut::<String>(path.as_str())
            {
                *field = value;
            }
        });
    }
}

/// Apply a checkbox's initial `Checked` state after spawn.
fn apply_init_checked(
    q: Query<(Entity, &InitChecked), Added<InitChecked>>,
    mut commands: Commands,
) {
    for (entity, init) in q.iter() {
        if init.0 {
            commands.entity(entity).insert(Checked);
        }
        commands.entity(entity).remove::<InitChecked>();
    }
}

/// Keep `f32` number inputs in sync with the world when the bound entity changes
/// elsewhere (e.g. a gizmo drag). Skips the focused widget so it doesn't clobber typing.
fn sync_number_fields(world: &mut World) {
    let focus = world.resource::<InputFocus>().get();

    let mut binding_q = world.query::<(Entity, &FieldBinding)>();
    let items: Vec<(Entity, Entity, TypeId, String)> = binding_q
        .iter(world)
        .filter(|(_, b)| b.kind == FieldKind::Num(NumTy::F32))
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

// ---------------------------------------------------------------------------
// Add / remove component
// ---------------------------------------------------------------------------

fn on_open_add_component(_: On<OpenAddComponentDialog>, mut commands: Commands) {
    commands.queue(open_add_component_dialog);
}

/// Build the modal "Add Component" list from every registered type that has both
/// `ReflectComponent` and `ReflectDefault` (so it can be default-constructed).
fn open_add_component_dialog(world: &mut World) {
    let registry_arc = world.resource::<AppTypeRegistry>().0.clone();
    let mut items: Vec<(String, TypeId)> = {
        let registry = registry_arc.read();
        registry
            .iter()
            .filter(|reg| reg.data::<ReflectComponent>().is_some())
            .filter(|reg| reg.data::<ReflectDefault>().is_some())
            .map(|reg| {
                (
                    reg.type_info().type_path_table().short_path().to_string(),
                    reg.type_id(),
                )
            })
            .collect()
    };
    items.sort_by(|a, b| a.0.cmp(&b.0));

    let _ = world.spawn_scene(add_component_overlay());

    let mut list_q = world.query_filtered::<Entity, With<AddComponentList>>();
    let Some(list) = list_q.iter(world).next() else {
        return;
    };
    let buttons: Vec<Box<dyn SceneList>> = items
        .into_iter()
        .map(|(name, tid)| {
            Box::new(EntityScene(add_component_item(name, tid))) as Box<dyn SceneList>
        })
        .collect();
    world
        .entity_mut(list)
        .queue_spawn_related_scenes::<Children>(buttons);
}

fn add_component_overlay() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: bevy_ui::JustifyContent::Center,
        }
        EditorEntity
        InspectorOverlay
        GlobalZIndex(2000)
        on(|_: On<Pointer<Click>>, mut c: Commands| { c.trigger(CloseInspectorOverlay); })
        Children [
            (
                Node {
                    width: px(280),
                    max_height: percent(70),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: px(6),
                    row_gap: px(4),
                    overflow: Overflow::scroll_y(),
                }
                EditorEntity
                AddComponentList
                ThemeBackgroundColor(tokens::PANE_HEADER_BG)
                GlobalZIndex(2001)
                ScrollArea
                Children [
                    (Node { padding: UiRect::axes(px(4), px(2)) } Children [ label("Add Component") ]),
                ]
            ),
        ]
    }
}

fn add_component_item(name: String, type_id: TypeId) -> impl Scene {
    bsn! {
        (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { Text(name) ThemedText } }
            AddComponentButton(type_id))
    }
}

fn on_add_component_button(
    act: On<Activate>,
    buttons: Query<&AddComponentButton>,
    selection: Res<EditorSelection>,
    mut commands: Commands,
) {
    let Ok(button) = buttons.get(act.entity) else {
        return;
    };
    let Some(target) = selection.primary else {
        return;
    };
    let type_id = button.0;
    push_undo(&mut commands);
    commands.queue(move |world: &mut World| {
        add_component_default(world, target, type_id);
        world.resource_mut::<InspectorDirty>().0 = true;
    });
    commands.trigger(CloseInspectorOverlay);
}

fn add_component_default(world: &mut World, target: Entity, type_id: TypeId) {
    let registry_arc = world.resource::<AppTypeRegistry>().0.clone();
    let registry = registry_arc.read();
    let Some(registration) = registry.get(type_id) else {
        return;
    };
    let Some(rc) = registration.data::<ReflectComponent>() else {
        return;
    };
    let Some(rd) = registration.data::<ReflectDefault>() else {
        return;
    };
    let value = rd.default();
    if let Ok(mut entity_mut) = world.get_entity_mut(target) {
        rc.insert(&mut entity_mut, value.as_partial_reflect(), &registry);
    }
}

fn on_remove_component_button(
    act: On<Activate>,
    buttons: Query<&RemoveComponentButton>,
    mut commands: Commands,
) {
    let Ok(button) = buttons.get(act.entity) else {
        return;
    };
    let (target, type_id) = (button.target, button.type_id);
    push_undo(&mut commands);
    commands.queue(move |world: &mut World| {
        let registry_arc = world.resource::<AppTypeRegistry>().0.clone();
        let registry = registry_arc.read();
        if let Some(registration) = registry.get(type_id)
            && let Some(rc) = registration.data::<ReflectComponent>()
            && let Ok(mut entity_mut) = world.get_entity_mut(target)
        {
            rc.remove(&mut entity_mut);
        }
        world.resource_mut::<InspectorDirty>().0 = true;
    });
}

fn on_close_inspector_overlay(
    _: On<CloseInspectorOverlay>,
    overlays: Query<Entity, With<InspectorOverlay>>,
    mut commands: Commands,
) {
    for overlay in overlays.iter() {
        commands.entity(overlay).despawn();
    }
}
