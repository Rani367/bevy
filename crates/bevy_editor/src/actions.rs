//! Editor "command" events emitted by the menu bar, toolbar, and context menus, and
//! handled by the relevant feature plugins. Routing through events keeps the UI shell
//! decoupled from the hierarchy / inspector / scene-IO implementations: the shell only
//! needs to know an event *type* exists, not who handles it.

use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use serde::{Deserialize, Serialize};

/// The kind of entity the user asked to create. Shared by the *Entity* menu, the
/// hierarchy context menu, and the editor scene file format.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Reflect, Serialize, Deserialize)]
pub enum SpawnKind {
    /// A 3D cube mesh with a default material.
    Cube,
    /// A 3D UV sphere mesh with a default material.
    Sphere,
    /// A 3D ground plane mesh with a default material.
    Plane,
    /// A point light.
    PointLight,
    /// A directional (sun) light.
    DirectionalLight,
    /// A 2D sprite (colored square).
    Sprite,
    /// An empty entity (just a `Transform` + `Name`), useful as a grouping node.
    Empty,
    /// A UI panel node (a colored `Node`), previewed in the viewport.
    UiNode,
    /// A UI text node.
    UiText,
}

/// Request to spawn a new scene entity. Handled by the hierarchy plugin, which owns
/// the mesh/material/asset resources needed to build it.
#[derive(Event, Clone, Copy)]
pub struct SpawnRequest(pub SpawnKind);

/// Request to delete the current selection. Handled by the hierarchy plugin.
#[derive(Event, Clone, Copy)]
pub struct DeleteSelectedRequest;

/// Request to duplicate the current selection. Handled by the hierarchy plugin.
#[derive(Event, Clone, Copy)]
pub struct DuplicateRequest;

/// Request to reparent a scene entity. `new_parent` of `None` moves it to the scene root.
/// Handled by the hierarchy plugin.
#[derive(Event, Clone, Copy)]
pub struct ReparentRequest {
    /// The entity being moved.
    pub child: Entity,
    /// The new parent, or `None` to unparent to the scene root.
    pub new_parent: Option<Entity>,
}

/// Request to begin renaming a scene entity inline in the hierarchy. Handled by the
/// hierarchy plugin.
#[derive(Event, Clone, Copy)]
pub struct RenameRequest(pub Entity);

/// Scene-file operations, handled by the scene-IO plugin. Paths are relative to the
/// asset root's `scenes/` directory.
#[derive(Event, Clone, Debug)]
pub enum SceneIoRequest {
    /// Clear the scene and start fresh.
    New,
    /// Save to the current scene path (falls back to the default name if unset).
    Save,
    /// Save to an explicit file name.
    SaveAs(String),
    /// Load a scene file, replacing the current scene.
    Open(String),
    /// Instantiate a scene file's entities *into* the current scene (prefab drop), without
    /// clearing what's already there.
    Instantiate(String),
}

/// Open the "Import Asset" dialog (a source-path prompt). Handled by the scene-IO plugin.
#[derive(Event, Clone, Copy)]
pub struct OpenImportDialog;

/// Open the "Save Scene As" dialog (a name prompt). Handled by the scene-IO plugin.
#[derive(Event, Clone, Copy)]
pub struct OpenSaveDialog;

/// Open the "Open Scene" dialog (a list of saved scenes). Handled by the scene-IO plugin.
#[derive(Event, Clone, Copy)]
pub struct OpenOpenDialog;
