//! The infinite reference grid shown in the 3D viewport. It renders through the scene
//! camera (shared default render layer), so it appears inside the viewport panel rather
//! than over the editor UI. It is tagged [`EditorEntity`] so it never enters the scene.

use bevy_dev_tools::infinite_grid::{InfiniteGrid, InfiniteGridSettings};
use bevy_ecs::prelude::*;

use crate::markers::EditorEntity;

/// Spawn the editor's reference grid and return its entity.
pub fn spawn_grid(commands: &mut Commands) -> Entity {
    commands
        .spawn((InfiniteGrid, InfiniteGridSettings::default(), EditorEntity))
        .id()
}
