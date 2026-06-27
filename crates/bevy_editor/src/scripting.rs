//! A small, self-contained behavior-scripting language for animating entities during play.
//!
//! Bevy has no built-in scripting runtime, so this is a complete little interpreter — a
//! lexer, a recursive-descent parser, and a tree-walking evaluator — with no external
//! dependencies. A [`BehaviorScript`] component holds a program that runs every frame while
//! the editor is playing, reading and writing the entity's [`Transform`].
//!
//! The language is line/`;`-separated statements over `f32` values (booleans are `0`/`1`):
//!
//! - `let name = expr;`                  — bind a per-frame variable
//! - `self.position.y = expr;`           — assign a transform channel
//!   (`self.position|rotation|scale . x|y|z`, plus `self.scale = expr` for uniform scale;
//!   `rotation` channels are Euler angles in radians)
//! - `if expr { … } else { … }`          — conditionals (nonzero is true)
//! - expressions: `+ - * / %`, comparisons `< <= > >= == !=`, unary `- !`, parentheses,
//!   the readonly bindings `time`, `dt`, `pi`, and functions
//!   `sin cos tan abs sqrt floor sign min max`.
//! - legacy one-liners `spin <s>`, `rotate <x|y|z> <s>`, `translate <x> <y> <z>`,
//!   `scale <f>` are still accepted (sugar over the above).
//!
//! Parse/runtime errors are reported (not panicked) via a [`ScriptError`] component, which
//! the reflection-driven inspector shows; a multi-line script editor (the "Edit Script ⛶"
//! button on a `BehaviorScript` section) opens an overlay bound to the script source.

mod lang;

pub use lang::{evaluate, parse, EvalCtx};

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::controls::{
    ButtonVariant, FeathersButton, FeathersTextInput, FeathersTextInputContainer,
};
use bevy_feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy_feathers::tokens;
use bevy_input_focus::AutoFocus;
use bevy_reflect::std_traits::ReflectDefault;
use bevy_reflect::Reflect;
use bevy_scene::prelude::*;
use bevy_state::condition::in_state;
use bevy_text::EditableText;
use bevy_time::Time;
use bevy_transform::components::Transform;
use bevy_ui::widget::Text;
use bevy_ui::{
    percent, px, AlignItems, Display, FlexDirection, GlobalZIndex, JustifyContent, Node,
    PositionType, UiRect,
};
use bevy_ui_widgets::Activate;

use crate::markers::EditorEntity;
use crate::state::EditorState;
use crate::ui::{stop_click, CloseOverlay, EditorOverlay, MultilineSeed, SeedText};

/// A behavior program that animates its entity's transform during play mode. See the module
/// docs for the language.
#[derive(Component, Reflect, Default, Clone, Debug)]
#[reflect(Component, Default)]
pub struct BehaviorScript {
    /// The script source: statements separated by newlines or `;`.
    pub source: String,
}

/// The most recent parse/runtime error for an entity's [`BehaviorScript`], surfaced in the
/// inspector. Cleared when the script next parses and runs cleanly.
#[derive(Component, Reflect, Default, Clone, Debug)]
#[reflect(Component, Default)]
pub struct ScriptError {
    /// Human-readable error message.
    pub message: String,
}

/// Open the multi-line script editor for the given entity.
#[derive(Event, Clone, Copy)]
pub(crate) struct OpenScriptEditor(pub(crate) Entity);

/// The multi-line text input inside the script editor overlay.
#[derive(Component, Default, Clone, Copy)]
struct ScriptEditorInput;

/// The Save button in the script editor overlay; carries the edited entity.
#[derive(Component, Clone, Copy)]
struct ScriptSaveButton(Entity);

impl Default for ScriptSaveButton {
    fn default() -> Self {
        Self(Entity::PLACEHOLDER)
    }
}

/// Installs the behavior-script interpreter, validator, and editor overlay.
pub struct ScriptingPlugin;

impl Plugin for ScriptingPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<BehaviorScript>()
            .register_type::<ScriptError>()
            .add_systems(Update, run_scripts.run_if(in_state(EditorState::Playing)))
            .add_systems(Update, validate_scripts)
            .add_observer(on_open_script_editor)
            .add_observer(on_script_save);
    }
}

/// Execute every scene entity's behavior script for this frame.
fn run_scripts(
    time: Res<Time>,
    mut commands: Commands,
    mut scripts: Query<(
        Entity,
        &BehaviorScript,
        &mut Transform,
        Option<&ScriptError>,
    )>,
) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    for (entity, script, mut transform, existing_err) in scripts.iter_mut() {
        match run_one(&script.source, t, dt, &mut transform) {
            Ok(()) => {
                if existing_err.is_some() {
                    commands.entity(entity).remove::<ScriptError>();
                }
            }
            Err(message) => set_error(&mut commands, entity, existing_err, message),
        }
    }
}

/// Live-validate scripts as they're edited (in any mode), so parse errors show immediately.
fn validate_scripts(
    mut commands: Commands,
    changed: Query<(Entity, &BehaviorScript, Option<&ScriptError>), Changed<BehaviorScript>>,
) {
    for (entity, script, existing_err) in changed.iter() {
        match parse(&script.source) {
            Ok(_) => {
                if existing_err.is_some() {
                    commands.entity(entity).remove::<ScriptError>();
                }
            }
            Err(message) => set_error(&mut commands, entity, existing_err, message),
        }
    }
}

/// Parse + evaluate one script against a transform.
fn run_one(source: &str, time: f32, dt: f32, transform: &mut Transform) -> Result<(), String> {
    let program = parse(source)?;
    let mut ctx = EvalCtx::new(transform, time, dt);
    evaluate(&program, &mut ctx)
}

fn set_error(
    commands: &mut Commands,
    entity: Entity,
    existing: Option<&ScriptError>,
    message: String,
) {
    if existing.map(|e| &e.message) != Some(&message) {
        commands.entity(entity).insert(ScriptError { message });
    }
}

// ---------------------------------------------------------------------------
// Multi-line script editor overlay
// ---------------------------------------------------------------------------

fn on_open_script_editor(
    req: On<OpenScriptEditor>,
    scripts: Query<&BehaviorScript>,
    mut commands: Commands,
) {
    let entity = req.0;
    let source = scripts
        .get(entity)
        .map(|s| s.source.clone())
        .unwrap_or_default();
    commands.spawn_scene(script_editor(entity, source));
}

fn on_script_save(
    act: On<Activate>,
    buttons: Query<&ScriptSaveButton>,
    inputs: Query<&EditableText, With<ScriptEditorInput>>,
    mut scripts: Query<&mut BehaviorScript>,
    mut commands: Commands,
) {
    let Ok(button) = buttons.get(act.entity) else {
        return;
    };
    let Some(text) = inputs.iter().next().map(|e| e.value().to_string()) else {
        return;
    };
    if let Ok(mut script) = scripts.get_mut(button.0) {
        script.source = text;
    }
    commands.trigger(CloseOverlay);
}

/// A centered overlay hosting a multi-line text editor bound to `entity`'s script.
fn script_editor(entity: Entity, source: String) -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        EditorEntity
        EditorOverlay
        GlobalZIndex(2000)
        on(|_: On<bevy_picking::events::Pointer<bevy_picking::events::Click>>, mut c: Commands| { c.trigger(CloseOverlay); })
        Children [
            (
                Node {
                    width: px(460),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Column,
                    padding: px(12),
                    row_gap: px(8),
                }
                EditorEntity
                ThemeBackgroundColor(tokens::PANE_HEADER_BG)
                GlobalZIndex(2001)
                on(stop_click)
                Children [
                    (Node { padding: UiRect::axes(px(2), px(2)) } Children [ label_title("Script Editor") ]),
                    (@FeathersTextInputContainer
                        Node { min_height: px(160) }
                        Children [
                            (@FeathersTextInput
                                SeedText(source)
                                MultilineSeed
                                ScriptEditorInput
                                AutoFocus)
                        ]),
                    (
                        Node { display: Display::Flex, flex_direction: FlexDirection::Row, column_gap: px(8) }
                        Children [
                            (@FeathersButton { @variant: ButtonVariant::Primary, @caption: bsn! { Text("Save") ThemedText } }
                                ScriptSaveButton(entity)),
                            (@FeathersButton { @variant: ButtonVariant::Normal, @caption: bsn! { Text("Cancel") ThemedText } }
                                on(|_: On<Activate>, mut c: Commands| { c.trigger(CloseOverlay); })),
                        ]
                    ),
                ]
            ),
        ]
    }
}

fn label_title(text: &str) -> impl Scene {
    let text = text.to_string();
    bsn! { (Text(text) ThemedText) }
}
