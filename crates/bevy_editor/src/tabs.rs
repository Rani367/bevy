//! Multi-scene tabs. Each tab holds an in-memory [`DynamicWorld`] snapshot of its scene;
//! switching tabs snapshots the live scene into the current tab and restores the target
//! tab's snapshot. This reuses the same [`crate::snapshot`] machinery as play mode and
//! undo, so runtime-generated meshes survive the round-trip.

use bevy_app::{App, Plugin, Update};
use bevy_ecs::prelude::*;
use bevy_feathers::controls::{ButtonVariant, FeathersButton};
use bevy_feathers::theme::ThemedText;
use bevy_scene::prelude::*;
use bevy_scene::EntityScene;
use bevy_ui::widget::Text;
use bevy_ui::{px, AlignItems, Node, UiRect};
use bevy_ui_widgets::Activate;
use bevy_world_serialization::DynamicWorld;

use crate::markers::SceneEntity;
use crate::snapshot::{restore_scene_snapshot, take_scene_snapshot};
use crate::state::EditorSelection;
use crate::ui::TabBarContent;

/// One open scene tab.
#[derive(Default)]
struct SceneTab {
    name: String,
    /// The tab's scene, captured when it was last switched away from. `None` for a tab
    /// that has never been left (its scene is whatever is currently live) or an empty tab.
    snapshot: Option<DynamicWorld>,
}

/// The set of open scene tabs and which one is active.
#[derive(Resource)]
struct Tabs {
    tabs: Vec<SceneTab>,
    active: usize,
    dirty: bool,
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            tabs: vec![SceneTab {
                name: "Scene 1".into(),
                snapshot: None,
            }],
            active: 0,
            dirty: true,
        }
    }
}

/// Switch to the tab at the given index.
#[derive(Event, Clone, Copy)]
struct SwitchTab(usize);

/// Open a new, empty scene tab.
#[derive(Event, Clone, Copy)]
struct NewTab;

/// Component on a tab button; stores the tab index it activates.
#[derive(Component, Clone, Copy)]
struct TabButton(usize);

impl Default for TabButton {
    fn default() -> Self {
        Self(usize::MAX)
    }
}

/// Installs the scene-tabs subsystem.
pub struct TabsPlugin;

impl Plugin for TabsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tabs>()
            .add_systems(Update, rebuild_tab_bar)
            .add_observer(on_switch_tab)
            .add_observer(on_new_tab)
            .add_observer(on_tab_button);
    }
}

fn on_tab_button(act: On<Activate>, buttons: Query<&TabButton>, mut commands: Commands) {
    if let Ok(button) = buttons.get(act.entity) {
        commands.trigger(SwitchTab(button.0));
    }
}

fn on_switch_tab(switch: On<SwitchTab>, mut commands: Commands) {
    let index = switch.0;
    commands.queue(move |world: &mut World| {
        let active = world.resource::<Tabs>().active;
        if index == active || index >= world.resource::<Tabs>().tabs.len() {
            return;
        }
        let current = take_scene_snapshot(world);
        let restore = {
            let mut tabs = world.resource_mut::<Tabs>();
            tabs.tabs[active].snapshot = Some(current);
            tabs.active = index;
            tabs.dirty = true;
            tabs.tabs[index].snapshot.take()
        };
        match restore {
            Some(snapshot) => restore_scene_snapshot(world, &snapshot),
            None => despawn_scene(world),
        }
        world.resource_mut::<EditorSelection>().clear();
    });
}

fn on_new_tab(_: On<NewTab>, mut commands: Commands) {
    commands.queue(|world: &mut World| {
        let current = take_scene_snapshot(world);
        {
            let mut tabs = world.resource_mut::<Tabs>();
            let active = tabs.active;
            tabs.tabs[active].snapshot = Some(current);
            let n = tabs.tabs.len();
            tabs.tabs.push(SceneTab {
                name: format!("Scene {}", n + 1),
                snapshot: None,
            });
            tabs.active = n;
            tabs.dirty = true;
        }
        despawn_scene(world);
        world.resource_mut::<EditorSelection>().clear();
    });
}

/// Despawn every live scene entity (leaving an empty scene).
fn despawn_scene(world: &mut World) {
    let ids: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
        query.iter(world).collect()
    };
    for entity in ids {
        if let Ok(entity_mut) = world.get_entity_mut(entity) {
            entity_mut.despawn();
        }
    }
}

/// Rebuild the tab-bar buttons when the tab set changes.
fn rebuild_tab_bar(
    mut tabs: ResMut<Tabs>,
    content_q: Query<Entity, With<TabBarContent>>,
    mut commands: Commands,
) {
    if !tabs.dirty {
        return;
    }
    let Ok(content) = content_q.single() else {
        return;
    };
    tabs.dirty = false;

    let mut buttons: Vec<Box<dyn SceneList>> = tabs
        .tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            Box::new(EntityScene(tab_button(
                i,
                tab.name.clone(),
                i == tabs.active,
            ))) as Box<dyn SceneList>
        })
        .collect();
    buttons.push(Box::new(EntityScene(new_tab_button())));

    commands.entity(content).despawn_children();
    commands
        .entity(content)
        .queue_spawn_related_scenes::<Children>(buttons);
}

fn tab_button(index: usize, name: String, active: bool) -> impl Scene {
    let variant = if active {
        ButtonVariant::Primary
    } else {
        ButtonVariant::Normal
    };
    bsn! {
        (@FeathersButton { @variant: variant, @caption: bsn! { Text(name) ThemedText } }
            TabButton(index)
            Node { padding: UiRect::axes(px(8), px(2)), align_items: AlignItems::Center })
    }
}

fn new_tab_button() -> impl Scene {
    bsn! {
        (@FeathersButton { @variant: ButtonVariant::Plain, @caption: bsn! { Text("+") ThemedText } }
            on(|_: On<Activate>, mut c: Commands| { c.trigger(NewTab); }))
    }
}
