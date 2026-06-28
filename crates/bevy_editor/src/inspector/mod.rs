//! The inspector panel: a generic, reflection-driven view of the selected entity's
//! components. It enumerates the entity's components via the type registry, walks each
//! component's fields with `bevy_reflect`, and renders an editable widget per field:
//! numbers (`f32`/`f64`/integers) become Feathers number inputs, `bool`s become
//! checkboxes, `String`s become text inputs, unit enums become a cycle button, and
//! `Color` is split into editable R/G/B/A channels. Edits are written back through
//! `ReflectComponent::reflect_mut` + a reflect path, and a top "＋ Add Component" button
//! plus per-section "✕" buttons add/remove components by reflection.

use core::any::TypeId;

use bevy_app::{App, Plugin, Startup, Update};
use bevy_color::{Color, Hsla};
use bevy_ecs::prelude::*;
use bevy_ecs::reflect::{AppTypeRegistry, ReflectComponent};
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_feathers::controls::{
    ButtonVariant, ColorChannel, ColorPlaneValue, ColorSwatchValue, FeathersButton,
    FeathersCheckbox, FeathersColorPlane, FeathersColorSlider, FeathersColorSwatch,
    FeathersNumberInput, FeathersTextInput, FeathersTextInputContainer, NumberInputPrecision,
    NumberInputValue, SliderBaseColor,
};
use bevy_feathers::display::{icon, label, label_dim, label_small};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemeToken, ThemedText};
use bevy_feathers::tokens;
use bevy_input_focus::InputFocus;
use bevy_math::{Vec2, Vec3};
use bevy_platform::collections::HashMap;
use bevy_reflect::enums::{DynamicEnum, DynamicVariant, VariantInfo};
use bevy_reflect::std_traits::ReflectDefault;
use bevy_reflect::structs::Struct;
use bevy_reflect::tuple::DynamicTuple;
use bevy_reflect::{
    GetPath, PartialReflect, Reflect, ReflectMut, ReflectRef, TypeInfo, TypeRegistry,
};
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_text::EditableText;
use bevy_ui::widget::Text;
use bevy_ui::{px, AlignItems, Checked, Display, FlexDirection, Node, UiRect};
use bevy_ui_widgets::{Activate, SliderValue, ValueChange};

use crate::scripting::{BehaviorScript, OpenScriptEditor};
use crate::state::EditorSelection;
use crate::ui::style::dialog_frame;
use crate::ui::{icons, InspectorContent, SeedText, ShowToast};
use crate::undo::push_undo;

/// A numeric field's concrete type, so edits can be written back to the right Rust type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NumTy {
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
pub(crate) enum FieldKind {
    Num(NumTy),
    Bool,
    Str,
    Enum,
    #[default]
    ReadOnly,
}

/// How a scalar edit is routed back into the world. Plain struct/list fields are addressed
/// by reflect path (`Scalar`); `Option` payloads and `Map` values can't be path-addressed,
/// so they navigate to the container and patch its element. Structural ops (add/remove/
/// toggle) drive collection-editing buttons.
#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) enum FieldOp {
    /// The widget's `path` addresses the value directly (the common case, incl. list elements
    /// via `[i]` indexing).
    #[default]
    Scalar,
    /// `path` addresses an `Option`; patch its `Some(_)` payload.
    OptionInner,
    /// `path` addresses a `Map`; patch the value of its `index`-th entry.
    MapValue(usize),
    /// Button: append a default element to the list at `path`.
    ListAdd,
    /// Button: remove the last element of the list at `path`.
    ListRemove,
    /// Button: toggle the `Option` at `path` between `Some(default)` and `None`.
    OptionToggle,
    /// Button: insert a default entry into the map at `path`.
    MapAdd,
    /// Button: remove the `index`-th entry from the map at `path`.
    MapRemove(usize),
}

/// Placed on an editable field widget; records how to write the widget's value back into
/// the world via reflection.
#[derive(Component, Clone, Debug)]
pub struct FieldBinding {
    pub(crate) target: Entity,
    pub(crate) component: TypeId,
    pub(crate) path: String,
    pub(crate) kind: FieldKind,
    /// For [`FieldKind::Enum`]: the ordered list of variant names to cycle through.
    pub(crate) variants: Vec<String>,
    /// How the edit is routed back (path vs. container element vs. structural button).
    pub(crate) op: FieldOp,
}

impl Default for FieldBinding {
    fn default() -> Self {
        Self {
            target: Entity::PLACEHOLDER,
            component: TypeId::of::<()>(),
            path: String::new(),
            kind: FieldKind::ReadOnly,
            variants: Vec::new(),
            op: FieldOp::Scalar,
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

/// An "Edit Script" button shown on a [`BehaviorScript`] section; opens the script editor.
#[derive(Component, Clone, Copy)]
struct ScriptEditButton(Entity);

impl Default for ScriptEditButton {
    fn default() -> Self {
        Self(Entity::PLACEHOLDER)
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

/// The list container within the "Add Component" dialog.
#[derive(Component, Default, Clone, Copy)]
struct AddComponentList;
/// The search input in the "Add Component" dialog.
#[derive(Component, Default, Clone, Copy)]
struct AddComponentSearch;

/// The full set of addable components (name + type), cached while the dialog is open so the
/// search filter doesn't re-scan the registry on every keystroke.
#[derive(Resource, Default)]
struct AddComponentItems(Vec<(String, TypeId)>);

/// Request to open the "Add Component" dialog.
#[derive(Event, Clone, Copy)]
struct OpenAddComponentDialog;

/// Placed on the inspector's color swatch button; carries where to write the picked color back.
#[derive(Component, Clone)]
struct ColorSwatchTarget {
    target: Entity,
    component: TypeId,
    path: String,
}

impl Default for ColorSwatchTarget {
    fn default() -> Self {
        Self {
            target: Entity::PLACEHOLDER,
            component: TypeId::of::<()>(),
            path: String::new(),
        }
    }
}

/// Open the interactive color picker popup for a reflected `Color` field.
#[derive(Event, Clone)]
struct OpenColorPicker {
    target: Entity,
    component: TypeId,
    path: String,
}

/// The color currently being edited in the open picker popup, plus where to write it back.
#[derive(Resource, Clone)]
struct ActiveColorEdit {
    target: Entity,
    component: TypeId,
    path: String,
    color: Hsla,
}

/// Marker on the picker's hue/saturation plane.
#[derive(Component, Default, Clone, Copy)]
struct PickerPlane;
/// Marker on the picker's lightness slider.
#[derive(Component, Default, Clone, Copy)]
struct PickerLightness;
/// Marker on the picker's alpha slider.
#[derive(Component, Default, Clone, Copy)]
struct PickerAlpha;
/// Marker on the picker's live preview swatch.
#[derive(Component, Default, Clone, Copy)]
struct PickerPreview;

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
            .init_resource::<PropertyEditorRegistry>()
            .add_systems(Startup, install_default_property_editors)
            .add_systems(
                Update,
                (
                    (mark_inspector_dirty, rebuild_inspector).chain(),
                    sync_number_fields.run_if(transform_may_change_externally),
                    commit_string_fields,
                    apply_init_checked,
                    filter_add_component,
                    sync_picker_widgets,
                ),
            )
            .add_observer(on_f32_changed)
            .add_observer(on_f64_changed)
            .add_observer(on_i64_changed)
            .add_observer(on_bool_changed)
            .add_observer(on_enum_cycle)
            .add_observer(on_field_op)
            .add_observer(on_open_add_component)
            .add_observer(on_add_component_button)
            .add_observer(on_remove_component_button)
            .add_observer(on_script_edit_button)
            .add_observer(on_color_swatch_button)
            .add_observer(on_open_color_picker)
            .add_observer(on_picker_plane)
            .add_observer(on_picker_lightness)
            .add_observer(on_picker_alpha);
    }
}

// ---------------------------------------------------------------------------
// Property-editor registry
// ---------------------------------------------------------------------------

/// Context handed to a [`PropertyEditorFn`] when it builds field rows for a value.
pub(crate) struct FieldEditorCtx<'a> {
    /// The reflected field value.
    pub value: &'a dyn PartialReflect,
    /// Reflect path of the value within its component.
    pub path: &'a str,
    /// Display label for the field.
    pub label: &'a str,
}

/// A per-type property editor: pushes one or more [`FieldModel`]s for the field and returns
/// `true` if it handled it (short-circuiting the built-in dispatch).
pub(crate) type PropertyEditorFn = fn(&FieldEditorCtx, &mut Vec<FieldModel>) -> bool;

/// Maps a concrete `TypeId` to a custom field editor. Consulted first by [`push_field`], so
/// downstream code can override how any reflected type is rendered in the inspector; falls
/// back to the built-in scalar/enum/collection/struct dispatch when no editor is registered.
#[derive(Resource, Default)]
pub struct PropertyEditorRegistry {
    editors: HashMap<TypeId, PropertyEditorFn>,
}

impl PropertyEditorRegistry {
    /// Register a custom editor for type `T`.
    pub(crate) fn register<T: 'static>(&mut self, editor: PropertyEditorFn) {
        self.editors.insert(TypeId::of::<T>(), editor);
    }

    /// Register a custom editor for a `TypeId` known only at runtime.
    pub(crate) fn register_type_id(&mut self, type_id: TypeId, editor: PropertyEditorFn) {
        self.editors.insert(type_id, editor);
    }

    fn get(&self, type_id: TypeId) -> Option<PropertyEditorFn> {
        self.editors.get(&type_id).copied()
    }
}

/// Seed the registry with the built-in editors (color picker + grouped vectors), proving the
/// mechanism and keeping those special cases data-driven rather than hard-coded in dispatch.
fn install_default_property_editors(mut registry: ResMut<PropertyEditorRegistry>) {
    use bevy_math::{Quat, Vec4};
    registry.register::<Color>(color_editor);
    registry.register::<Vec2>(vector_editor);
    registry.register::<Vec3>(vector_editor);
    // Also exercise the runtime-`TypeId` registration path.
    registry.register_type_id(TypeId::of::<Vec4>(), vector_editor);
    registry.register_type_id(TypeId::of::<Quat>(), vector_editor);
}

/// Built-in editor for [`Color`]: a clickable preview swatch (opens the picker popup) followed
/// by editable R/G/B/A channels.
fn color_editor(ctx: &FieldEditorCtx, out: &mut Vec<FieldModel>) -> bool {
    let Some(c) = ctx.value.try_downcast_ref::<Color>() else {
        return false;
    };
    let s = c.to_srgba();
    out.push(FieldModel::leaf(
        ctx.label,
        ctx.path,
        FieldValue::ColorSwatch {
            rgba: [s.red, s.green, s.blue, s.alpha],
        },
    ));
    for (suffix, chan, val) in [
        ("r", NumTy::ColorR, s.red),
        ("g", NumTy::ColorG, s.green),
        ("b", NumTy::ColorB, s.blue),
        ("a", NumTy::ColorA, s.alpha),
    ] {
        out.push(FieldModel::leaf(
            format!("{}.{suffix}", ctx.label),
            ctx.path,
            FieldValue::Num {
                value: val as f64,
                ty: chan,
            },
        ));
    }
    true
}

/// Built-in editor for `Vec2`/`Vec3`/`Vec4`/`Quat`: one labeled row of colored axis inputs.
fn vector_editor(ctx: &FieldEditorCtx, out: &mut Vec<FieldModel>) -> bool {
    if let ReflectRef::Struct(s) = ctx.value.reflect_ref()
        && let Some(axes) = vector_axes(s)
    {
        out.push(FieldModel::leaf(
            ctx.label,
            ctx.path,
            FieldValue::Vec { axes },
        ));
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Model collection
// ---------------------------------------------------------------------------

pub(crate) struct FieldModel {
    label: String,
    path: String,
    value: FieldValue,
    op: FieldOp,
}

impl FieldModel {
    /// A plain path-addressed leaf field.
    fn leaf(label: impl Into<String>, path: impl Into<String>, value: FieldValue) -> Self {
        Self {
            label: label.into(),
            path: path.into(),
            value,
            op: FieldOp::Scalar,
        }
    }
}

/// Whether a collection header is a list (`[i]` add/remove) or a map (key/value add/remove).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CollKind {
    List,
    Map,
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
    /// A list/map header with element count and add/remove controls.
    Collection {
        kind: CollKind,
        len: usize,
    },
    /// An `Option`'s `Some`/`None` toggle button.
    OptionToggle {
        is_some: bool,
    },
    /// A `Vec2`/`Vec3`/`Vec4`/`Quat`: one labeled row with per-axis colored inputs (each writing
    /// `path.x` / `.y` / `.z` / `.w`). `axes` is `(axis_name, value)` in declaration order.
    Vec {
        axes: Vec<(String, f64)>,
    },
    /// A non-editable color preview swatch (the editable R/G/B/A channels follow it).
    ColorSwatch {
        rgba: [f32; 4],
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
            // A `BehaviorScript` gets an "Edit Script" button opening the multi-line editor.
            if comp.type_id == TypeId::of::<BehaviorScript>() {
                scenes.push(Box::new(EntityScene(script_edit_button(target))));
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
            field.op.clone(),
        ))),
        FieldValue::Bool(v) => Box::new(EntityScene(bool_field(
            field.label.clone(),
            target,
            component,
            field.path.clone(),
            *v,
            field.op.clone(),
        ))),
        FieldValue::Str(text) => Box::new(EntityScene(string_field(
            field.label.clone(),
            target,
            component,
            field.path.clone(),
            text.clone(),
            field.op.clone(),
        ))),
        FieldValue::Enum { current, variants } => Box::new(EntityScene(enum_field(
            field.label.clone(),
            target,
            component,
            field.path.clone(),
            current.clone(),
            variants.clone(),
        ))),
        FieldValue::Collection { kind, len } => Box::new(EntityScene(collection_header(
            field.label.clone(),
            target,
            component,
            field.path.clone(),
            *kind,
            *len,
        ))),
        FieldValue::OptionToggle { is_some } => Box::new(EntityScene(option_toggle_field(
            field.label.clone(),
            target,
            component,
            field.path.clone(),
            *is_some,
        ))),
        FieldValue::Vec { axes } => vec_field(
            field.label.clone(),
            target,
            component,
            field.path.clone(),
            axes.clone(),
        ),
        FieldValue::ColorSwatch { rgba } => Box::new(EntityScene(color_swatch_field(
            field.label.clone(),
            target,
            component,
            field.path.clone(),
            *rgba,
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
    let editor_registry = world.resource::<PropertyEditorRegistry>();
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
        collect_component_fields(editor_registry, reflected.as_partial_reflect(), &mut fields);
        components.push(ComponentModel {
            name,
            type_id,
            fields,
        });
    }
    components
}

/// Walk a component's top-level fields into [`FieldModel`]s.
fn collect_component_fields(
    registry: &PropertyEditorRegistry,
    value: &dyn PartialReflect,
    out: &mut Vec<FieldModel>,
) {
    match value.reflect_ref() {
        ReflectRef::Struct(s) => {
            for i in 0..s.field_len() {
                let name = s.name_at(i).unwrap_or("?").to_string();
                if let Some(field) = s.field_at(i) {
                    push_field(registry, field, &name, &name, 0, out);
                }
            }
        }
        // A component that *is* an enum (e.g. `Visibility`): one field at the empty path.
        ReflectRef::Enum(_) => push_field(registry, value, "", "value", 0, out),
        _ => out.push(FieldModel::leaf(
            "value",
            "",
            FieldValue::ReadOnly(format!("{value:?}")),
        )),
    }
}

/// If `field` is a known scalar leaf type, return the [`FieldValue`] for an editable widget.
fn scalar_value(field: &dyn PartialReflect) -> Option<FieldValue> {
    macro_rules! num {
        ($ty:ty, $tag:expr) => {
            if let Some(v) = field.try_downcast_ref::<$ty>() {
                return Some(FieldValue::Num {
                    value: *v as f64,
                    ty: $tag,
                });
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
        return Some(FieldValue::Bool(*v));
    }
    if let Some(v) = field.try_downcast_ref::<String>() {
        return Some(FieldValue::Str(v.clone()));
    }
    None
}

/// Whether `field` is a `core::option::Option<_>`.
fn is_option(field: &dyn PartialReflect) -> bool {
    field
        .get_represented_type_info()
        .map(|info| info.type_path().starts_with("core::option::Option<"))
        .unwrap_or(false)
}

/// Push a single field, recursing into nested structs / collections and mapping known leaf
/// types to editable widgets.
fn push_field(
    registry: &PropertyEditorRegistry,
    field: &dyn PartialReflect,
    path: &str,
    label: &str,
    depth: usize,
    out: &mut Vec<FieldModel>,
) {
    // A registered per-type editor (e.g. Color, Vec3) wins over the built-in dispatch.
    if let Some(type_id) = field.get_represented_type_info().map(TypeInfo::type_id)
        && let Some(editor) = registry.get(type_id)
    {
        let ctx = FieldEditorCtx {
            value: field,
            path,
            label,
        };
        if editor(&ctx, out) {
            return;
        }
    }

    if let Some(value) = scalar_value(field) {
        out.push(FieldModel::leaf(label, path, value));
        return;
    }

    // `Option<T>`: a Some/None toggle, plus an editor for the payload when present.
    if is_option(field)
        && let ReflectRef::Enum(e) = field.reflect_ref()
    {
        let is_some = e.variant_name() == "Some";
        out.push(FieldModel::leaf(
            label,
            path,
            FieldValue::OptionToggle { is_some },
        ));
        if is_some && let Some(inner) = e.field_at(0) {
            if let Some(value) = scalar_value(inner) {
                out.push(FieldModel {
                    label: format!("{label} = Some"),
                    path: path.into(),
                    value,
                    op: FieldOp::OptionInner,
                });
            } else {
                out.push(FieldModel::leaf(
                    format!("{label} = Some"),
                    path,
                    FieldValue::ReadOnly(format!("{inner:?}")),
                ));
            }
        }
        return;
    }

    // Unit-only enums (e.g. `Visibility`): a cycle button.
    if let ReflectRef::Enum(e) = field.reflect_ref()
        && let Some(TypeInfo::Enum(info)) = field.get_represented_type_info()
        && info.iter().all(|v| matches!(v, VariantInfo::Unit(_)))
    {
        out.push(FieldModel::leaf(
            label,
            path,
            FieldValue::Enum {
                current: e.variant_name().to_string(),
                variants: info
                    .variant_names()
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            },
        ));
        return;
    }

    // List / array: a header with element count (+ add/remove for lists) and editable
    // elements addressed by `[i]` indexing.
    if let ReflectRef::List(list) = field.reflect_ref() {
        out.push(FieldModel::leaf(
            label,
            path,
            FieldValue::Collection {
                kind: CollKind::List,
                len: list.len(),
            },
        ));
        for i in 0..list.len() {
            if let Some(elem) = list.get(i) {
                push_field(
                    registry,
                    elem,
                    &format!("{path}[{i}]"),
                    &format!("[{i}]"),
                    depth + 1,
                    out,
                );
            }
        }
        return;
    }
    if let ReflectRef::Array(arr) = field.reflect_ref() {
        out.push(FieldModel::leaf(
            label,
            path,
            FieldValue::Collection {
                kind: CollKind::List,
                len: arr.len(),
            },
        ));
        for i in 0..arr.len() {
            if let Some(elem) = arr.get(i) {
                push_field(
                    registry,
                    elem,
                    &format!("{path}[{i}]"),
                    &format!("[{i}]"),
                    depth + 1,
                    out,
                );
            }
        }
        return;
    }

    // Map: a header (+ add/remove) and editable scalar values keyed by their iteration index.
    if let ReflectRef::Map(map) = field.reflect_ref() {
        out.push(FieldModel::leaf(
            label,
            path,
            FieldValue::Collection {
                kind: CollKind::Map,
                len: map.len(),
            },
        ));
        for (i, (key, value)) in map.iter().enumerate() {
            let key_label = format!("{label}[{key:?}]");
            if let Some(scalar) = scalar_value(value) {
                out.push(FieldModel {
                    label: key_label,
                    path: path.into(),
                    value: scalar,
                    op: FieldOp::MapValue(i),
                });
            } else {
                out.push(FieldModel::leaf(
                    key_label,
                    path,
                    FieldValue::ReadOnly(format!("{value:?}")),
                ));
            }
        }
        return;
    }

    if let ReflectRef::Struct(s) = field.reflect_ref() {
        // (`Vec2`/`Vec3`/`Vec4`/`Quat` are handled above by the registry's `vector_editor`.)
        if depth < 3 {
            for i in 0..s.field_len() {
                let child_name = s.name_at(i).unwrap_or("?");
                if let Some(child) = s.field_at(i) {
                    let child_path = if path.is_empty() {
                        child_name.to_string()
                    } else {
                        format!("{path}.{child_name}")
                    };
                    push_field(registry, child, &child_path, child_name, depth + 1, out);
                }
            }
            return;
        }
    }

    out.push(FieldModel::leaf(
        label,
        path,
        FieldValue::ReadOnly(format!("{field:?}")),
    ));
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
            (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { (icon(icons::X)) } }
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
    op: FieldOp,
) -> impl Scene {
    let niv = match ty {
        NumTy::F64 => NumberInputValue::F64(value),
        NumTy::I32 | NumTy::I64 | NumTy::U32 | NumTy::U64 | NumTy::Usize => {
            NumberInputValue::I64(value as i64)
        }
        _ => NumberInputValue::F32(value as f32),
    };
    let precision = if ty.is_integer() { 0 } else { 3 };
    let sigil = axis_sigil(&field_label, ty);
    // For axis components (x/y/z/w), show the letter inside the colored input (Unity-style)
    // and drop the redundant left label; everything else keeps its caption column.
    let axis = axis_label(&field_label);
    let row_label = if axis.is_some() {
        String::new()
    } else {
        field_label
    };
    field_row(
        row_label,
        bsn! {
            (
                @FeathersNumberInput { @sigil_color: sigil, @label_text: axis }
                template_value(niv)
                NumberInputPrecision(precision)
                FieldBinding { target: target, component: component, path: path, kind: FieldKind::Num(ty), variants: Vec::new(), op: op }
                Node { flex_grow: 1.0, max_width: px(150) }
            )
        },
    )
}

/// The colored left-edge "sigil" for a number field: red/green/blue for X/Y/Z axes and the
/// R/G/B color channels, transparent otherwise. Mirrors Unity/Godot axis coloring.
fn axis_sigil(label: &str, ty: NumTy) -> ThemeToken {
    match ty {
        NumTy::ColorR => tokens::TEXT_INPUT_X_AXIS,
        NumTy::ColorG => tokens::TEXT_INPUT_Y_AXIS,
        NumTy::ColorB => tokens::TEXT_INPUT_Z_AXIS,
        _ => match label {
            "x" | "X" => tokens::TEXT_INPUT_X_AXIS,
            "y" | "Y" => tokens::TEXT_INPUT_Y_AXIS,
            "z" | "Z" => tokens::TEXT_INPUT_Z_AXIS,
            _ => tokens::TEXT_INPUT_BG,
        },
    }
}

/// The in-input axis letter for an `x`/`y`/`z`/`w` field, if it is one.
fn axis_label(label: &str) -> Option<&'static str> {
    match label {
        "x" | "X" => Some("X"),
        "y" | "Y" => Some("Y"),
        "z" | "Z" => Some("Z"),
        "w" | "W" => Some("W"),
        _ => None,
    }
}

/// Detect a `Vec2`/`Vec3`/`Vec4`/`Quat`-shaped struct (2–4 `f32` fields named x/y/z/w) and
/// return its `(axis, value)` pairs in declaration order, or `None` if it isn't one.
fn vector_axes(s: &dyn Struct) -> Option<Vec<(String, f64)>> {
    let n = s.field_len();
    if !(2..=4).contains(&n) {
        return None;
    }
    let mut axes = Vec::with_capacity(n);
    for i in 0..n {
        let name = s.name_at(i)?;
        if !matches!(name, "x" | "y" | "z" | "w") {
            return None;
        }
        let v = s.field_at(i)?.try_downcast_ref::<f32>().copied()?;
        axes.push((name.to_string(), v as f64));
    }
    Some(axes)
}

/// One colored axis number input for a grouped vector row, writing back to `base.axis`.
fn axis_input(
    target: Entity,
    component: TypeId,
    base_path: &str,
    axis: &str,
    value: f64,
) -> impl Scene {
    let path = if base_path.is_empty() {
        axis.to_string()
    } else {
        format!("{base_path}.{axis}")
    };
    let sigil = axis_sigil(axis, NumTy::F32);
    let label = axis_label(axis);
    let niv = NumberInputValue::F32(value as f32);
    bsn! {
        (
            @FeathersNumberInput { @sigil_color: sigil, @label_text: label }
            template_value(niv)
            NumberInputPrecision(3)
            FieldBinding { target: target, component: component, path: path, kind: FieldKind::Num(NumTy::F32), variants: Vec::new(), op: FieldOp::Scalar }
            Node { flex_grow: 1.0, min_width: px(0) }
        )
    }
}

/// A grouped vector row: the field label plus 2–4 colored axis inputs side by side
/// (e.g. `Translation: [X][Y][Z]`).
fn vec_field(
    field_label: String,
    target: Entity,
    component: TypeId,
    base_path: String,
    axes: Vec<(String, f64)>,
) -> Box<dyn SceneList> {
    let bp = base_path.as_str();
    match axes.as_slice() {
        [a0, a1] => Box::new(EntityScene(field_row(
            field_label.clone(),
            bsn! {
                (Node { flex_grow: 1.0, flex_direction: FlexDirection::Row, column_gap: px(4) }
                    Children [
                        (axis_input(target, component, bp, &a0.0, a0.1)),
                        (axis_input(target, component, bp, &a1.0, a1.1)),
                    ])
            },
        ))),
        [a0, a1, a2] => Box::new(EntityScene(field_row(
            field_label.clone(),
            bsn! {
                (Node { flex_grow: 1.0, flex_direction: FlexDirection::Row, column_gap: px(4) }
                    Children [
                        (axis_input(target, component, bp, &a0.0, a0.1)),
                        (axis_input(target, component, bp, &a1.0, a1.1)),
                        (axis_input(target, component, bp, &a2.0, a2.1)),
                    ])
            },
        ))),
        [a0, a1, a2, a3] => Box::new(EntityScene(field_row(
            field_label.clone(),
            bsn! {
                (Node { flex_grow: 1.0, flex_direction: FlexDirection::Row, column_gap: px(4) }
                    Children [
                        (axis_input(target, component, bp, &a0.0, a0.1)),
                        (axis_input(target, component, bp, &a1.0, a1.1)),
                        (axis_input(target, component, bp, &a2.0, a2.1)),
                        (axis_input(target, component, bp, &a3.0, a3.1)),
                    ])
            },
        ))),
        _ => Box::new(EntityScene(readonly_field(
            field_label,
            format!("{axes:?}"),
        ))),
    }
}

/// A clickable color preview swatch row: clicking opens the interactive picker popup; the
/// R/G/B/A channel inputs follow it for direct numeric editing.
fn color_swatch_field(
    field_label: String,
    target: Entity,
    component: TypeId,
    path: String,
    rgba: [f32; 4],
) -> impl Scene {
    let color = Color::srgba(rgba[0], rgba[1], rgba[2], rgba[3]);
    let swatch_value = ColorSwatchValue(color);
    field_row(
        field_label,
        bsn! {
            (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! {
                (@FeathersColorSwatch
                    template_value(swatch_value)
                    Node { width: px(48), height: px(16) })
            } }
                ColorSwatchTarget { target: target, component: component, path: path })
        },
    )
}

fn bool_field(
    field_label: String,
    target: Entity,
    component: TypeId,
    path: String,
    value: bool,
    op: FieldOp,
) -> impl Scene {
    field_row(
        field_label,
        bsn! {
            (
                @FeathersCheckbox
                FieldBinding { target: target, component: component, path: path, kind: FieldKind::Bool, variants: Vec::new(), op: op }
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
    op: FieldOp,
) -> impl Scene {
    field_row(
        field_label,
        bsn! {
            (@FeathersTextInputContainer
                Children [
                    (@FeathersTextInput
                        SeedText(value)
                        FieldBinding { target: target, component: component, path: path, kind: FieldKind::Str, variants: Vec::new(), op: op })
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
                FieldBinding { target: target, component: component, path: path, kind: FieldKind::Enum, variants: variants, op: FieldOp::Scalar })
        },
    )
}

fn readonly_field(field_label: String, text: String) -> impl Scene {
    field_row(field_label, bsn! { label_dim(text) })
}

/// A list/map header row: `name [len]` plus `＋` / `−` buttons that add and remove elements.
fn collection_header(
    field_label: String,
    target: Entity,
    component: TypeId,
    path: String,
    kind: CollKind,
    len: usize,
) -> impl Scene {
    let (add_op, remove_op) = match kind {
        CollKind::List => (FieldOp::ListAdd, FieldOp::ListRemove),
        CollKind::Map => (FieldOp::MapAdd, FieldOp::MapRemove(len.saturating_sub(1))),
    };
    let add_path = path.clone();
    bsn! {
        Node {
            min_height: px(22),
            padding: UiRect::axes(px(6), px(2)),
            align_items: AlignItems::Center,
            column_gap: px(6),
        }
        ThemeBackgroundColor(tokens::GROUP_HEADER_BG)
        Children [
            (Node { flex_grow: 1.0 } Children [ label_small(format!("{field_label} [{len}]")) ]),
            (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { Text("+") ThemedText } }
                FieldBinding { target: target, component: component, path: add_path, kind: FieldKind::ReadOnly, variants: Vec::new(), op: add_op }),
            (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { Text("-") ThemedText } }
                FieldBinding { target: target, component: component, path: path, kind: FieldKind::ReadOnly, variants: Vec::new(), op: remove_op }),
        ]
    }
}

/// An `Option` Some/None toggle button.
fn option_toggle_field(
    field_label: String,
    target: Entity,
    component: TypeId,
    path: String,
    is_some: bool,
) -> impl Scene {
    let caption = if is_some {
        "Some (clear)"
    } else {
        "None (set)"
    };
    field_row(
        field_label,
        bsn! {
            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text(caption) ThemedText } }
                FieldBinding { target: target, component: component, path: path, kind: FieldKind::ReadOnly, variants: Vec::new(), op: FieldOp::OptionToggle })
        },
    )
}

/// The "Edit Script ⛶" button for a `BehaviorScript` section.
fn script_edit_button(target: Entity) -> impl Scene {
    bsn! {
        Node { padding: UiRect::axes(px(6), px(3)) }
        Children [
            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { (Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(6) } Children [ (icon(icons::CODE) ThemedText), (Text("Edit Script") ThemedText) ]) } }
                ScriptEditButton(target)),
        ]
    }
}

fn on_script_edit_button(
    act: On<Activate>,
    buttons: Query<&ScriptEditButton>,
    mut commands: Commands,
) {
    if let Ok(button) = buttons.get(act.entity) {
        commands.trigger(OpenScriptEditor(button.0));
    }
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
    let (target, component, path, op) = (
        binding.target,
        binding.component,
        binding.path.clone(),
        binding.op.clone(),
    );
    commands.queue(move |world: &mut World| {
        // `Option` payloads / `Map` values can't be reflect-path-addressed; patch the
        // container element instead.
        if matches!(op, FieldOp::OptionInner | FieldOp::MapValue(_)) {
            apply_patch(world, target, component, &path, &op, boxed_num(ty, value));
        } else {
            write_numeric(world, target, component, &path, ty, value);
        }
    });
}

/// Box a numeric value as its concrete reflected type (for patching collection elements).
fn boxed_num(ty: NumTy, value: f64) -> Box<dyn PartialReflect> {
    match ty {
        NumTy::F32 | NumTy::ColorR | NumTy::ColorG | NumTy::ColorB | NumTy::ColorA => {
            Box::new(value as f32)
        }
        NumTy::F64 => Box::new(value),
        NumTy::I32 => Box::new(value as i32),
        NumTy::I64 => Box::new(value as i64),
        NumTy::U32 => Box::new(value as u32),
        NumTy::U64 => Box::new(value as u64),
        NumTy::Usize => Box::new(value as usize),
    }
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
        warn_set(world, path);
        return;
    };
    if world.get_entity(target).is_err() {
        warn_set(world, path);
        return;
    }
    let ok = {
        let Some(mut reflected) = rc.reflect_mut(world.entity_mut(target)) else {
            warn_set(world, path);
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
                    true
                } else {
                    false
                }
            }
        }
    };
    if !ok {
        warn_set(world, path);
    }
}

/// Write `value` at `path` within `reflected`; returns whether the write succeeded.
fn set_path<T: Reflect>(reflected: &mut dyn Reflect, path: &str, value: T) -> bool {
    if let Ok(field) = reflected.path_mut::<T>(path) {
        *field = value;
        true
    } else {
        false
    }
}

/// Surface a failed inspector write-back as a warning toast naming the field.
fn warn_set(world: &mut World, path: &str) {
    let field = if path.is_empty() { "field" } else { path };
    world.trigger(ShowToast::warning(format!("Couldn't set {field}")));
}

/// Default value for a registered type, if it has [`ReflectDefault`].
fn default_value(registry: &TypeRegistry, type_id: TypeId) -> Option<Box<dyn PartialReflect>> {
    let rd = registry.get(type_id)?.data::<ReflectDefault>()?;
    Some(rd.default().into_partial_reflect())
}

/// Patch a scalar into a container element that can't be reflect-path-addressed: an
/// `Option`'s `Some(_)` payload, or a `Map` value (by iteration index).
fn apply_patch(
    world: &mut World,
    target: Entity,
    component: TypeId,
    path: &str,
    op: &FieldOp,
    patch: Box<dyn PartialReflect>,
) {
    let Some(rc) = reflect_component_for(world, component) else {
        warn_set(world, path);
        return;
    };
    if world.get_entity(target).is_err() {
        warn_set(world, path);
        return;
    }
    {
        let Some(mut reflected) = rc.reflect_mut(world.entity_mut(target)) else {
            warn_set(world, path);
            return;
        };
        let Ok(container) = reflected.reflect_path_mut(path) else {
            warn_set(world, path);
            return;
        };
        apply_element_patch(container, op, &*patch);
    }
}

/// Patch a scalar into an `Option` payload or a `Map` value held by `container`.
fn apply_element_patch(
    container: &mut dyn PartialReflect,
    op: &FieldOp,
    patch: &dyn PartialReflect,
) {
    match op {
        FieldOp::OptionInner => {
            if let ReflectMut::Enum(e) = container.reflect_mut()
                && let Some(inner) = e.field_at_mut(0)
            {
                let _ = inner.try_apply(patch);
            }
        }
        FieldOp::MapValue(i) => {
            if let ReflectMut::Map(map) = container.reflect_mut() {
                let Some(key) = map.iter().nth(*i).map(|(k, _)| k.to_dynamic()) else {
                    return;
                };
                let Some(newval) = map.get(&*key).map(|v| {
                    let mut d = v.to_dynamic();
                    let _ = d.try_apply(patch);
                    d
                }) else {
                    return;
                };
                map.insert_boxed(key, newval);
            }
        }
        _ => {}
    }
}

/// Perform a structural collection edit (add/remove element, toggle `Option`).
fn structural_op(world: &mut World, target: Entity, component: TypeId, path: &str, op: &FieldOp) {
    let Some(rc) = reflect_component_for(world, component) else {
        return;
    };
    if world.get_entity(target).is_err() {
        return;
    }
    let registry_arc = world.resource::<AppTypeRegistry>().0.clone();
    let registry = registry_arc.read();
    let Some(mut reflected) = rc.reflect_mut(world.entity_mut(target)) else {
        return;
    };
    let Ok(container) = reflected.reflect_path_mut(path) else {
        return;
    };
    apply_structural(container, op, &registry);
}

/// Add/remove a list element, toggle an `Option`, or add/remove a map entry on `container`.
fn apply_structural(container: &mut dyn PartialReflect, op: &FieldOp, registry: &TypeRegistry) {
    match op {
        FieldOp::ListAdd => {
            // Add a copy of the last element, or a default if the list is empty.
            let item_default = match container.get_represented_type_info() {
                Some(TypeInfo::List(l)) => default_value(registry, l.item_ty().id()),
                _ => None,
            };
            if let ReflectMut::List(list) = container.reflect_mut() {
                let new = if list.is_empty() {
                    item_default
                } else {
                    list.get(list.len() - 1).map(PartialReflect::to_dynamic)
                };
                if let Some(new) = new {
                    list.push(new);
                }
            }
        }
        FieldOp::ListRemove => {
            if let ReflectMut::List(list) = container.reflect_mut()
                && !list.is_empty()
            {
                list.pop();
            }
        }
        FieldOp::OptionToggle => {
            let is_some = matches!(container.reflect_ref(), ReflectRef::Enum(e) if e.variant_name() == "Some");
            if is_some {
                let _ = container.try_apply(&DynamicEnum::new("None", DynamicVariant::Unit));
            } else {
                let inner_ty = match container.get_represented_type_info() {
                    Some(TypeInfo::Enum(en)) => {
                        en.iter()
                            .find(|v| v.name() == "Some")
                            .and_then(|v| match v {
                                VariantInfo::Tuple(t) => t.field_at(0).map(|f| f.ty().id()),
                                _ => None,
                            })
                    }
                    _ => None,
                };
                if let Some(inner) = inner_ty.and_then(|tid| default_value(registry, tid)) {
                    let mut tuple = DynamicTuple::default();
                    tuple.insert_boxed(inner);
                    let _ = container
                        .try_apply(&DynamicEnum::new("Some", DynamicVariant::Tuple(tuple)));
                }
            }
        }
        FieldOp::MapAdd => {
            let kv = match container.get_represented_type_info() {
                Some(TypeInfo::Map(m)) => default_value(registry, m.key_ty().id())
                    .zip(default_value(registry, m.value_ty().id())),
                _ => None,
            };
            if let Some((k, v)) = kv
                && let ReflectMut::Map(map) = container.reflect_mut()
            {
                map.insert_boxed(k, v);
            }
        }
        FieldOp::MapRemove(i) => {
            if let ReflectMut::Map(map) = container.reflect_mut() {
                let key = map.iter().nth(*i).map(|(k, _)| k.to_dynamic());
                if let Some(key) = key {
                    map.remove(&*key);
                }
            }
        }
        _ => {}
    }
}

/// Drive a collection add/remove/toggle button.
fn on_field_op(act: On<Activate>, bindings: Query<&FieldBinding>, mut commands: Commands) {
    let Ok(binding) = bindings.get(act.entity) else {
        return;
    };
    if !matches!(
        binding.op,
        FieldOp::ListAdd
            | FieldOp::ListRemove
            | FieldOp::OptionToggle
            | FieldOp::MapAdd
            | FieldOp::MapRemove(_)
    ) {
        return;
    }
    let (target, component, path, op) = (
        binding.target,
        binding.component,
        binding.path.clone(),
        binding.op.clone(),
    );
    push_undo(&mut commands);
    commands.queue(move |world: &mut World| {
        structural_op(world, target, component, &path, &op);
        world.resource_mut::<InspectorDirty>().0 = true;
    });
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
    let (target, component, path, op) = (
        binding.target,
        binding.component,
        binding.path.clone(),
        binding.op.clone(),
    );
    let value = change.value;
    push_undo(&mut commands);
    commands.queue(move |world: &mut World| {
        if matches!(op, FieldOp::OptionInner | FieldOp::MapValue(_)) {
            apply_patch(world, target, component, &path, &op, Box::new(value));
            return;
        }
        let Some(rc) = reflect_component_for(world, component) else {
            warn_set(world, &path);
            return;
        };
        if world.get_entity(target).is_err() {
            warn_set(world, &path);
            return;
        }
        let ok = {
            let Some(mut reflected) = rc.reflect_mut(world.entity_mut(target)) else {
                warn_set(world, &path);
                return;
            };
            set_path(&mut *reflected, &path, value)
        };
        if !ok {
            warn_set(world, &path);
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
    if variants.is_empty() {
        return;
    }
    let Some(rc) = reflect_component_for(world, component) else {
        warn_set(world, path);
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
    let Some(next_name) = variants.get(next).cloned() else {
        return;
    };

    let ok = {
        if let Some(mut reflected) = rc.reflect_mut(world.entity_mut(target))
            && let Ok(field) = reflected.reflect_path_mut(path)
        {
            field.apply(&DynamicEnum::new(next_name, DynamicVariant::Unit));
            true
        } else {
            false
        }
    };
    if !ok {
        warn_set(world, path);
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
        let (target, component, path, op) = (
            binding.target,
            binding.component,
            binding.path.clone(),
            binding.op.clone(),
        );
        commands.queue(move |world: &mut World| {
            if matches!(op, FieldOp::OptionInner | FieldOp::MapValue(_)) {
                apply_patch(world, target, component, &path, &op, Box::new(value));
                return;
            }
            let Some(rc) = reflect_component_for(world, component) else {
                warn_set(world, &path);
                return;
            };
            if world.get_entity(target).is_err() {
                warn_set(world, &path);
                return;
            }
            let ok = {
                if let Some(mut reflected) = rc.reflect_mut(world.entity_mut(target))
                    && let Ok(field) = reflected.path_mut::<String>(path.as_str())
                {
                    *field = value;
                    true
                } else {
                    false
                }
            };
            if !ok {
                warn_set(world, &path);
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
/// `sync_number_fields` only needs to run when something *other* than the inspector can move
/// the inspected values: a live gizmo drag, or play-mode scripts. Selection changes already
/// rebuild the panel with fresh values, so the rest of the time this work is skipped.
fn transform_may_change_externally(
    drag: Res<crate::state::GizmoDrag>,
    state: Res<bevy_state::state::State<crate::state::EditorState>>,
) -> bool {
    drag.active || *state.get() == crate::state::EditorState::Playing
}

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

/// Build the modal "Add Component" dialog from every registered type that has both
/// `ReflectComponent` and `ReflectDefault` (so it can be default-constructed). The full list
/// is cached in [`AddComponentItems`] for the live search filter.
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
    world.insert_resource(AddComponentItems(items.clone()));

    let _ = world.spawn_scene(add_component_overlay());

    let mut list_q = world.query_filtered::<Entity, With<AddComponentList>>();
    let Some(list) = list_q.iter(world).next() else {
        return;
    };
    let buttons = add_component_rows(&items, "");
    world
        .entity_mut(list)
        .queue_spawn_related_scenes::<Children>(buttons);
}

/// Build the (filtered) result rows for the add-component list, capped so a blank query
/// doesn't spawn hundreds of nodes.
fn add_component_rows(items: &[(String, TypeId)], query: &str) -> Vec<Box<dyn SceneList>> {
    let query = query.to_lowercase();
    items
        .iter()
        .filter(|(name, _)| query.is_empty() || name.to_lowercase().contains(&query))
        .take(60)
        .map(|(name, tid)| {
            Box::new(EntityScene(add_component_item(name.clone(), *tid))) as Box<dyn SceneList>
        })
        .collect()
}

/// Re-filter the add-component list as the search text changes.
fn filter_add_component(
    search: Query<&EditableText, (With<AddComponentSearch>, Changed<EditableText>)>,
    items: Option<Res<AddComponentItems>>,
    list: Query<Entity, With<AddComponentList>>,
    mut commands: Commands,
) {
    let (Ok(text), Some(items), Ok(container)) = (search.single(), items, list.single()) else {
        return;
    };
    let rows = add_component_rows(&items.0, &text.value().to_string());
    commands.entity(container).despawn_children();
    commands
        .entity(container)
        .queue_spawn_related_scenes::<Children>(rows);
}

fn add_component_overlay() -> impl Scene {
    dialog_frame(
        "Add Component",
        px(360),
        bsn! {
            (
                Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(8) }
                Children [
                    (@FeathersTextInputContainer Children [
                        (@FeathersTextInput SeedText(String::new()) AddComponentSearch bevy_input_focus::AutoFocus)
                    ]),
                    (Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(2) } AddComponentList),
                ]
            )
        },
    )
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
    commands.trigger(crate::ui::CloseOverlay);
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

// ---------------------------------------------------------------------------
// Color picker popup
// ---------------------------------------------------------------------------

/// Open the picker when the inspector's color swatch is clicked.
fn on_color_swatch_button(
    act: On<Activate>,
    swatches: Query<&ColorSwatchTarget>,
    mut commands: Commands,
) {
    let Ok(swatch) = swatches.get(act.entity) else {
        return;
    };
    commands.trigger(OpenColorPicker {
        target: swatch.target,
        component: swatch.component,
        path: swatch.path.clone(),
    });
}

/// Read the field's current color, seed [`ActiveColorEdit`], and spawn the picker dialog.
fn on_open_color_picker(ev: On<OpenColorPicker>, mut commands: Commands) {
    let (target, component, path) = (ev.target, ev.component, ev.path.clone());
    commands.queue(move |world: &mut World| {
        let color = read_color(world, target, component, &path).unwrap_or(Color::WHITE);
        let hsla: Hsla = color.into();
        world.insert_resource(ActiveColorEdit {
            target,
            component,
            path,
            color: hsla,
        });
        let _ = world.spawn_scene(color_picker_overlay(hsla));
    });
}

/// Read a reflected `Color` at `path`, if present.
fn read_color(world: &World, target: Entity, component: TypeId, path: &str) -> Option<Color> {
    let rc = reflect_component_for(world, component)?;
    let entity_ref = world.get_entity(target).ok()?;
    let reflected = rc.reflect(entity_ref)?;
    reflected.path::<Color>(path).ok().copied()
}

/// Write a `Color` back through the reflect path, toasting on failure.
fn write_color(world: &mut World, target: Entity, component: TypeId, path: &str, color: Color) {
    let Some(rc) = reflect_component_for(world, component) else {
        warn_set(world, path);
        return;
    };
    if world.get_entity(target).is_err() {
        warn_set(world, path);
        return;
    }
    let ok = {
        let Some(mut reflected) = rc.reflect_mut(world.entity_mut(target)) else {
            warn_set(world, path);
            return;
        };
        if let Ok(c) = reflected.path_mut::<Color>(path) {
            *c = color;
            true
        } else {
            false
        }
    };
    if !ok {
        warn_set(world, path);
    }
}

/// Queue a write of the picker's current color to the world; on `refresh` also rebuild the
/// inspector so its swatch + channels catch up (done only on interaction end to avoid thrash).
fn commit_active_color(commands: &mut Commands, edit: &ActiveColorEdit, refresh: bool) {
    let (target, component, path, color) = (
        edit.target,
        edit.component,
        edit.path.clone(),
        Color::from(edit.color),
    );
    commands.queue(move |world: &mut World| {
        write_color(world, target, component, &path, color);
        if refresh {
            world.resource_mut::<InspectorDirty>().0 = true;
        }
    });
}

/// Hue/saturation from the 2D plane (x = hue, y = 1 − saturation, matching the HS shader).
fn on_picker_plane(
    change: On<ValueChange<Vec2>>,
    planes: Query<(), With<PickerPlane>>,
    edit: Option<ResMut<ActiveColorEdit>>,
    mut commands: Commands,
) {
    if !planes.contains(change.source) {
        return;
    }
    let Some(mut edit) = edit else {
        return;
    };
    edit.color.hue = (change.value.x * 360.0).clamp(0.0, 360.0);
    edit.color.saturation = (1.0 - change.value.y).clamp(0.0, 1.0);
    commit_active_color(&mut commands, &edit, change.is_final);
}

fn on_picker_lightness(
    change: On<ValueChange<f32>>,
    sliders: Query<(), With<PickerLightness>>,
    edit: Option<ResMut<ActiveColorEdit>>,
    mut commands: Commands,
) {
    if !sliders.contains(change.source) {
        return;
    }
    let Some(mut edit) = edit else {
        return;
    };
    edit.color.lightness = change.value.clamp(0.0, 1.0);
    commit_active_color(&mut commands, &edit, change.is_final);
}

fn on_picker_alpha(
    change: On<ValueChange<f32>>,
    sliders: Query<(), With<PickerAlpha>>,
    edit: Option<ResMut<ActiveColorEdit>>,
    mut commands: Commands,
) {
    if !sliders.contains(change.source) {
        return;
    }
    let Some(mut edit) = edit else {
        return;
    };
    edit.color.alpha = change.value.clamp(0.0, 1.0);
    commit_active_color(&mut commands, &edit, change.is_final);
}

/// Keep the picker's widgets in sync with [`ActiveColorEdit`] (thumb positions, gradients,
/// preview). Setting these components doesn't re-emit `ValueChange`, so there's no feedback loop.
fn sync_picker_widgets(
    edit: Option<Res<ActiveColorEdit>>,
    mut planes: Query<&mut ColorPlaneValue, With<PickerPlane>>,
    mut previews: Query<&mut ColorSwatchValue, With<PickerPreview>>,
    lightness: Query<Entity, (With<PickerLightness>, Without<PickerAlpha>)>,
    alpha: Query<Entity, (With<PickerAlpha>, Without<PickerLightness>)>,
    mut commands: Commands,
) {
    let Some(edit) = edit else {
        return;
    };
    if !edit.is_changed() {
        return;
    }
    let hsla = edit.color;
    let color = Color::from(hsla);
    let plane_v = Vec3::new(
        (hsla.hue / 360.0).clamp(0.0, 1.0),
        (1.0 - hsla.saturation).clamp(0.0, 1.0),
        hsla.lightness,
    );
    for mut v in planes.iter_mut() {
        v.0 = plane_v;
    }
    for mut s in previews.iter_mut() {
        s.0 = color;
    }
    // `SliderValue` is an immutable component, so update sliders by re-inserting.
    for e in lightness.iter() {
        commands
            .entity(e)
            .insert((SliderValue(hsla.lightness), SliderBaseColor(color)));
    }
    for e in alpha.iter() {
        commands
            .entity(e)
            .insert((SliderValue(hsla.alpha), SliderBaseColor(color)));
    }
}

/// The picker popup: a hue/saturation plane, lightness + alpha sliders, and a live preview.
fn color_picker_overlay(color: Hsla) -> impl Scene {
    let plane_v = Vec3::new(
        (color.hue / 360.0).clamp(0.0, 1.0),
        (1.0 - color.saturation).clamp(0.0, 1.0),
        color.lightness,
    );
    let col = Color::from(color);
    let plane_value = ColorPlaneValue(plane_v);
    let light_base = SliderBaseColor(col);
    let alpha_base = SliderBaseColor(col);
    let preview = ColorSwatchValue(col);
    let lightness = color.lightness;
    let alpha = color.alpha;
    dialog_frame(
        "Color",
        px(280),
        bsn! {
            (Node { display: Display::Flex, flex_direction: FlexDirection::Column, row_gap: px(10) }
                Children [
                    (@FeathersColorPlane::HueSaturation
                        template_value(plane_value)
                        PickerPlane
                        Node { min_height: px(150) }),
                    (@FeathersColorSlider { @value: lightness, @channel: ColorChannel::HslLightness }
                        template_value(light_base)
                        PickerLightness),
                    (@FeathersColorSlider { @value: alpha, @channel: ColorChannel::Alpha }
                        template_value(alpha_base)
                        PickerAlpha),
                    (@FeathersColorSwatch { @opaque_color_percentage: 40.0 }
                        template_value(preview)
                        PickerPreview
                        Node { min_height: px(24) }),
                    (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Done") ThemedText } }
                        on(|_: On<Activate>, mut c: Commands| { c.trigger(crate::ui::CloseOverlay); })),
                ])
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{
        add_component_rows, apply_element_patch, apply_structural, axis_sigil, color_editor,
        push_field, vector_axes, vector_editor, FieldEditorCtx, FieldModel, FieldOp, FieldValue,
        NumTy, PropertyEditorRegistry,
    };
    use alloc::collections::BTreeMap;
    use bevy_feathers::tokens;
    use bevy_reflect::{PartialReflect, Reflect, ReflectRef, TypeRegistry};
    use core::any::TypeId;

    fn registry() -> TypeRegistry {
        let mut r = TypeRegistry::default();
        r.register::<f32>();
        r.register::<String>();
        r
    }

    #[test]
    fn axis_sigil_colors_xyz_and_rgb() {
        assert_eq!(axis_sigil("x", NumTy::F32), tokens::TEXT_INPUT_X_AXIS);
        assert_eq!(axis_sigil("y", NumTy::F32), tokens::TEXT_INPUT_Y_AXIS);
        assert_eq!(axis_sigil("z", NumTy::F32), tokens::TEXT_INPUT_Z_AXIS);
        assert_eq!(axis_sigil("w", NumTy::F32), tokens::TEXT_INPUT_BG);
        assert_eq!(
            axis_sigil("anything", NumTy::ColorR),
            tokens::TEXT_INPUT_X_AXIS
        );
        assert_eq!(
            axis_sigil("anything", NumTy::ColorG),
            tokens::TEXT_INPUT_Y_AXIS
        );
    }

    #[test]
    fn vector_axes_detects_vec3_and_quat() {
        use bevy_math::{Quat, Vec3};
        let v = Vec3::new(1.0, 2.0, 3.0);
        let ReflectRef::Struct(s) = v.reflect_ref() else {
            panic!("Vec3 is a struct");
        };
        let axes = vector_axes(s).expect("Vec3 is a vector");
        assert_eq!(axes.len(), 3);
        assert_eq!(axes[0], ("x".to_string(), 1.0));
        assert_eq!(axes[2], ("z".to_string(), 3.0));

        let q = Quat::IDENTITY;
        let ReflectRef::Struct(s) = q.reflect_ref() else {
            panic!("Quat is a struct");
        };
        let axes = vector_axes(s).expect("Quat is a vector");
        assert_eq!(axes.len(), 4, "x/y/z/w");
        assert_eq!(axes[3].0, "w");
    }

    #[test]
    fn vector_axes_rejects_non_vectors() {
        #[derive(Reflect, Default)]
        struct NotVec {
            a: f32,
            b: f32,
        }
        let n = NotVec::default();
        let ReflectRef::Struct(s) = n.reflect_ref() else {
            panic!();
        };
        assert!(vector_axes(s).is_none(), "fields not named x/y/z/w");

        // A struct whose axis-named fields aren't f32 is also rejected.
        #[derive(Reflect, Default)]
        struct IntVec {
            x: i32,
            y: i32,
        }
        let iv = IntVec::default();
        let ReflectRef::Struct(s) = iv.reflect_ref() else {
            panic!();
        };
        assert!(vector_axes(s).is_none(), "axes must be f32");
    }

    #[test]
    fn add_component_rows_filter_and_cap() {
        let many: Vec<(String, TypeId)> = (0..100)
            .map(|i| (format!("Comp{i}"), TypeId::of::<f32>()))
            .collect();
        // A blank query is capped so we don't spawn hundreds of nodes.
        assert_eq!(add_component_rows(&many, "").len(), 60);

        let two = vec![
            ("Transform".to_string(), TypeId::of::<f32>()),
            ("MeshMaterial".to_string(), TypeId::of::<f32>()),
        ];
        assert_eq!(add_component_rows(&two, "trans").len(), 1);
        assert_eq!(add_component_rows(&two, "MESH").len(), 1); // case-insensitive
        assert_eq!(add_component_rows(&two, "zzz").len(), 0);
    }

    #[test]
    fn list_add_clones_last_then_remove_pops() {
        let reg = registry();
        let mut v: Vec<f32> = vec![1.0, 2.0];
        apply_structural(&mut v, &FieldOp::ListAdd, &reg);
        assert_eq!(v, vec![1.0, 2.0, 2.0], "add copies the last element");
        apply_structural(&mut v, &FieldOp::ListRemove, &reg);
        assert_eq!(v, vec![1.0, 2.0], "remove pops the last element");
    }

    #[test]
    fn list_add_to_empty_uses_default() {
        let reg = registry();
        let mut v: Vec<f32> = vec![];
        apply_structural(&mut v, &FieldOp::ListAdd, &reg);
        assert_eq!(
            v,
            vec![0.0],
            "adding to an empty list uses the element default"
        );
    }

    #[test]
    fn list_element_patch_via_option_path_is_noop() {
        // Sanity: element scalar edits go through reflect paths, not apply_element_patch;
        // apply_element_patch only handles Option/Map.
        let reg = registry();
        let mut v: Vec<f32> = vec![5.0];
        apply_structural(&mut v, &FieldOp::ListRemove, &reg);
        assert!(v.is_empty());
    }

    #[test]
    fn option_toggle_and_inner_patch() {
        let reg = registry();
        let mut o: Option<f32> = None;
        apply_structural(&mut o, &FieldOp::OptionToggle, &reg);
        assert_eq!(o, Some(0.0), "None -> Some(default)");
        apply_element_patch(&mut o, &FieldOp::OptionInner, &5.0_f32);
        assert_eq!(o, Some(5.0), "Some payload edited in place");
        apply_structural(&mut o, &FieldOp::OptionToggle, &reg);
        assert_eq!(o, None, "Some -> None");
    }

    #[test]
    fn map_add_value_edit_remove() {
        let reg = registry();
        let mut m: BTreeMap<String, f32> = BTreeMap::new();
        apply_structural(&mut m, &FieldOp::MapAdd, &reg);
        assert_eq!(m.len(), 1, "add inserts a default entry");
        assert_eq!(m.get(""), Some(&0.0));
        apply_element_patch(&mut m, &FieldOp::MapValue(0), &7.0_f32);
        assert_eq!(m.get(""), Some(&7.0), "the 0th entry's value was patched");
        apply_structural(&mut m, &FieldOp::MapRemove(0), &reg);
        assert!(m.is_empty(), "remove drops the 0th entry");
    }

    #[test]
    fn registry_custom_editor_overrides_builtin() {
        fn dummy(ctx: &FieldEditorCtx, out: &mut Vec<FieldModel>) -> bool {
            out.push(FieldModel::leaf(
                "custom",
                ctx.path,
                FieldValue::ReadOnly("CUSTOM".into()),
            ));
            true
        }
        let mut reg = PropertyEditorRegistry::default();
        reg.register::<f32>(dummy);
        let mut out = Vec::new();
        let v = 1.5_f32;
        push_field(&reg, &v, "x", "x", 0, &mut out);
        assert_eq!(out.len(), 1, "custom editor handled the field");
        assert!(matches!(out[0].value, FieldValue::ReadOnly(ref s) if s == "CUSTOM"));
    }

    #[test]
    fn registry_falls_back_to_builtin_when_unregistered() {
        let reg = PropertyEditorRegistry::default();
        let mut out = Vec::new();
        let v = 2.0_f32;
        push_field(&reg, &v, "x", "x", 0, &mut out);
        assert!(
            matches!(out[0].value, FieldValue::Num { .. }),
            "unregistered f32 uses the built-in scalar editor"
        );
    }

    #[test]
    fn builtin_color_editor_emits_swatch_and_channels() {
        use bevy_color::Color;
        let c = Color::srgba(0.1, 0.2, 0.3, 0.4);
        let ctx = FieldEditorCtx {
            value: &c,
            path: "color",
            label: "color",
        };
        let mut out = Vec::new();
        assert!(color_editor(&ctx, &mut out));
        assert_eq!(out.len(), 5, "swatch + R/G/B/A channels");
        assert!(matches!(out[0].value, FieldValue::ColorSwatch { .. }));
        assert!(matches!(
            out[1].value,
            FieldValue::Num {
                ty: NumTy::ColorR,
                ..
            }
        ));
    }

    #[test]
    fn builtin_vector_editor_groups_axes_and_declines_non_vectors() {
        use bevy_math::Vec3;
        let v = Vec3::new(1.0, 2.0, 3.0);
        let ctx = FieldEditorCtx {
            value: &v,
            path: "translation",
            label: "translation",
        };
        let mut out = Vec::new();
        assert!(vector_editor(&ctx, &mut out));
        match &out[0].value {
            FieldValue::Vec { axes } => assert_eq!(axes.len(), 3),
            _ => panic!("expected one grouped vector row"),
        }

        let s = String::from("hi");
        let ctx2 = FieldEditorCtx {
            value: &s,
            path: "n",
            label: "n",
        };
        let mut out2 = Vec::new();
        assert!(!vector_editor(&ctx2, &mut out2), "non-vector declined");
    }
}
