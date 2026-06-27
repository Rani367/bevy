//! The scene viewport: an offscreen camera (2D or 3D) whose render target is shown
//! inside the center panel via `bevy_ui`'s `ViewportNode`, plus the editor camera
//! controllers, the reference grid, and click-to-select picking.

mod camera;
mod gizmos;
mod grid;
mod picking;

pub use camera::{Editor2dCamera, Editor3dCamera};

use bevy_app::{App, Plugin, Startup, Update};
use bevy_asset::{Assets, Handle};
use bevy_camera::visibility::Visibility;
use bevy_camera::{Camera, Camera2d, Camera3d, RenderTarget};
use bevy_dev_tools::infinite_grid::InfiniteGrid;
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use bevy_image::Image;
use bevy_light::DirectionalLight;
use bevy_math::Vec3;
use bevy_render::render_resource::TextureFormat;
use bevy_transform::components::Transform;
use bevy_ui::widget::ViewportNode;

use crate::actions::SpawnKind;
use crate::markers::{EditorEntity, GameCamera, SceneEntity};
use crate::spawning::SpawnedAs;
use crate::state::ViewportMode;
use crate::ui::ViewportSlot;

/// Tracks the offscreen scene camera, its render-target image, the panel node hosting
/// the viewport, and the mode the camera was built for.
#[derive(Resource)]
pub struct EditorViewport {
    /// The live scene camera (rebuilt when switching 2D/3D).
    pub camera: Entity,
    /// The render-target image the scene camera draws into and the `ViewportNode` shows.
    pub image: Handle<Image>,
    /// The `ViewportSlot` node the `ViewportNode` was bound onto, once available.
    pub slot: Option<Entity>,
    /// The mode the current camera was created for.
    pub current_mode: ViewportMode,
}

/// Installs the viewport camera, grid, mode-switching, and picking.
pub struct ViewportPlugin;

impl Plugin for ViewportPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_viewport)
            .add_systems(
                Update,
                (
                    bind_viewport_node,
                    switch_viewport_mode,
                    camera::orbit_camera,
                    camera::pan_camera,
                    picking::sync_selected_marker,
                    picking::clear_on_escape,
                    gizmos::draw_gizmos,
                ),
            )
            .add_observer(picking::select_on_click)
            .add_observer(gizmos::select_on_drag_start)
            .add_observer(gizmos::drag_to_translate);
    }
}

/// Spawn a 3D scene camera that renders into `image`.
fn spawn_camera_3d(commands: &mut Commands, image: Handle<Image>) -> Entity {
    commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: -1,
                ..Default::default()
            },
            RenderTarget::Image(image.into()),
            Editor3dCamera::default(),
            GameCamera,
            EditorEntity,
        ))
        .id()
}

/// Spawn a 2D scene camera that renders into `image`.
fn spawn_camera_2d(commands: &mut Commands, image: Handle<Image>) -> Entity {
    commands
        .spawn((
            Camera2d,
            Camera {
                order: -1,
                ..Default::default()
            },
            RenderTarget::Image(image.into()),
            Editor2dCamera::default(),
            GameCamera,
            EditorEntity,
        ))
        .id()
}

/// Startup: create the render-target image, spawn the initial scene camera + grid, add a
/// default light, and stash the [`EditorViewport`] resource.
fn setup_viewport(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mode: Res<ViewportMode>,
) {
    // Size is irrelevant: `update_viewport_render_target_size` resizes it to the panel.
    let image = Image::new_target_texture(1, 1, TextureFormat::Bgra8UnormSrgb, None);
    let handle = images.add(image);

    let camera = match *mode {
        ViewportMode::ThreeD => spawn_camera_3d(&mut commands, handle.clone()),
        ViewportMode::TwoD => spawn_camera_2d(&mut commands, handle.clone()),
    };

    grid::spawn_grid(&mut commands);

    // A default light so spawned 3D meshes are visible. It is part of the scene and is
    // tagged so it serializes like any other spawned entity.
    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.0,
            ..Default::default()
        },
        Transform::from_xyz(6.0, 10.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
        SceneEntity,
        SpawnedAs(SpawnKind::DirectionalLight),
        Name::new("Directional Light"),
    ));

    commands.insert_resource(EditorViewport {
        camera,
        image: handle,
        slot: None,
        current_mode: *mode,
    });
}

/// Once the shell's `ViewportSlot` exists, attach a `ViewportNode` bound to the scene
/// camera. Runs every frame but no-ops after the first successful bind.
fn bind_viewport_node(
    mut commands: Commands,
    mut viewport: ResMut<EditorViewport>,
    slot_q: Query<Entity, (With<ViewportSlot>, Without<ViewportNode>)>,
) {
    if viewport.slot.is_some() {
        return;
    }
    if let Ok(slot) = slot_q.single() {
        commands
            .entity(slot)
            .insert(ViewportNode::new(viewport.camera));
        viewport.slot = Some(slot);
    }
}

/// When [`ViewportMode`] is toggled, rebuild the scene camera for the new dimension,
/// rebind the `ViewportNode`, and show/hide the 3D grid.
fn switch_viewport_mode(
    mode: Res<ViewportMode>,
    mut commands: Commands,
    mut viewport: ResMut<EditorViewport>,
    mut node_q: Query<&mut ViewportNode>,
    mut grid_q: Query<&mut Visibility, With<InfiniteGrid>>,
) {
    if *mode == viewport.current_mode {
        return;
    }

    commands.entity(viewport.camera).despawn();
    let new_camera = match *mode {
        ViewportMode::ThreeD => spawn_camera_3d(&mut commands, viewport.image.clone()),
        ViewportMode::TwoD => spawn_camera_2d(&mut commands, viewport.image.clone()),
    };
    viewport.camera = new_camera;
    viewport.current_mode = *mode;

    if let Some(slot) = viewport.slot
        && let Ok(mut node) = node_q.get_mut(slot)
    {
        node.camera = Some(new_camera);
    }

    let grid_visibility = match *mode {
        ViewportMode::ThreeD => Visibility::Visible,
        ViewportMode::TwoD => Visibility::Hidden,
    };
    for mut visibility in grid_q.iter_mut() {
        *visibility = grid_visibility;
    }
}
