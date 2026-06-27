# bevy_editor — implementation status

A Unity/Godot-style GUI editor for Bevy, built entirely on existing Bevy infrastructure
(`bevy_feathers` widgets, `bevy_ui`'s `ViewportNode`, `bevy_reflect`,
`bevy_world_serialization`, `bevy_picking`/`bevy_gizmos`, `bevy_remote`). This file is an
honest accounting of what works and how it is verified — **everything below is ✅**.

Run it: `cargo run --example editor --features bevy_editor`

## Verification

- **Unit tests** (`cargo test -p bevy_editor`, 21 tests) cover the logic-heavy pieces with no
  GPU/UI needed: the scripting language (lexer/parser/evaluator + error cases), the `.scn.ron`
  serialize→deserialize round-trip (parent links + arbitrary components), the inspector
  list/option/map reflection ops, the gizmo axis-pick + snapping math, and the build packaging.
- **Interactive paths** (pointer/keyboard gestures) are wired to the same action/observer code
  the unit-tested logic uses; exercise them by running the `editor` example. Each gesture is
  noted below with the action it drives.

Everything compiles **zero-warning** (`cargo fmt --check`, `cargo clippy --all-targets`, and the
`editor` example) and launches on Metal without panics.

---

## 1. Crate + workspace wiring — ✅
`crates/bevy_editor/` exposes the `EditorPlugins` group, wired through `bevy_internal`
(`pub use bevy_editor as editor`) and the root `Cargo.toml` (`bevy_editor` feature + the
`editor` example).

## 2. UI shell + dockable panels — ✅
- Window-filling paneled layout: menu bar (File / Edit / Entity / View / Build), toolbar,
  scene-tab strip, body row (Hierarchy | Viewport | Inspector), Assets row. Dark theme.
- **Panel scrolling** — Hierarchy / Inspector / Asset panels are `ScrollArea`s.
- **Resizable splitters** — drag handles resize neighboring panels, clamped to `[120, 900]`px.
- **In-window dockable panels** (`ui/docking.rs`) — each side panel can be **collapsed** (body
  hidden) and **torn off to float** (dragged by its header, then re-docked) via header buttons;
  layout is data-driven through `DockState` (`apply_dock_layout` sets the content `Display` and
  the root `position_type`/offset/z-index).

## 3. Viewport — ✅
- Offscreen scene camera → `ViewportNode`; 3D scene (infinite grid + lit meshes) renders in the
  center panel; wheel-zoom is gated to pointer-over-viewport (`ViewportHovered`).
- **3D orbit/pan/zoom** (`Editor3dCamera`) and **2D pan/zoom** (`Editor2dCamera`): right-drag
  orbits, middle-drag pans, wheel zooms.
- **Click-to-select** (forwarded picking → `EditorSelection` + `EditorSelected`) and
  **Escape-to-clear**.

## 4. 2D / 3D modes — ✅
`View → Toggle 2D/3D` / the toolbar rebuilds the scene camera (2D `Camera2d` + sprite picking,
or 3D `Camera3d` + mesh picking) via `switch_viewport_mode` and shows/hides the grid.

## 5. Hierarchy panel — ✅
- Live entity tree of every `SceneEntity` with depth indentation + selection highlight,
  rebuilding on add/remove/rename/reparent.
- **Spawn / Delete / Duplicate / Reparent** — reflection-based; all undoable.
- **Gestures**: row click → select (Ctrl/Cmd additive); right-click → context menu (Rename /
  Duplicate / Delete), dismissed by clicking the backdrop; double-click + inline rename
  (autofocused field, commit on Enter); disclosure collapse/expand; **drag-and-drop reparent**
  (drop a row on another to nest, on empty space to unparent — propagation is stopped at the
  row so the drop doesn't bubble to the panel and immediately unparent).

## 6. Inspector — ✅
- Generic, reflection-driven: enumerates components and walks fields.
- **Editable per field type**: `f32`/`f64`/integers (number inputs), `bool` (checkbox),
  `String` (text), unit enums e.g. `Visibility` (cycle button), `Color` (R/G/B/A). Write-back
  via `reflect_mut` + path; gizmo/script changes sync back into the number fields.
- **List / `Option` / Map editing** — list elements edit in place (`[i]` paths) plus add/remove;
  `Option` has a Some/None toggle + payload editor; maps add/remove entries and edit values
  (`apply_structural` / `apply_element_patch`, unit-tested).
- **Add / Remove Component** dialog (registry types with `ReflectComponent` + `ReflectDefault`).
  All edits capture undo.

## 7. Transform gizmo — ✅
- Visuals per mode (translate arrows, rotate rings, scale handles), engaged axis highlighted.
- **Drag-to-manipulate** per `GizmoMode`: translate (axis-constrained or free view-plane),
  rotate, scale (uniform **or per-axis**); applies to the whole multi-selection, one undo entry
  per gesture. **Snapping** (toolbar toggle or held Ctrl) snaps translate/scale to a grid and
  rotate to angle steps (axis-pick + snap math unit-tested).

## 8. Scene save / load + asset browser — ✅
- **Full `.scn.ron` persistence** via `DynamicWorld`: every `SceneEntity`'s reflected components
  **and parent links** round-trip to disk; only the runtime-built mesh/material/sprite and the
  computed transform/visibility are excluded and rebuilt from each entity's `SpawnedAs` on load
  (round-trip unit-tested).
- **Save-As / Open / Import** modal dialogs, wired from the File menu.
- **Asset browser** lists saved scenes + live image thumbnails; clicking a scene instantiates it
  as a prefab; File→Open replaces the scene.

## 9. Play / Pause / Stop + scripting — ✅
- **Snapshot on play, restore on stop** via in-memory `DynamicWorld` (asset handles stay valid).
- **Behavior scripting** (`scripting/lang.rs`) — a complete built-in mini-language (lexer +
  recursive-descent parser + tree-walking evaluator, no external deps): `let`, arithmetic /
  comparison expressions, `if/else`, `self.position|rotation|scale` channels, `time`/`dt`/`pi`,
  and `sin cos tan abs sqrt floor sign min max`; legacy `spin/rotate/translate/scale` one-liners
  still work. Parse/runtime errors surface (not panic) via a `ScriptError` component shown in the
  inspector; a **multi-line script editor** opens from the inspector's "Edit Script" button.

## 10. Undo / redo — ✅
Whole-scene snapshot stack, captured before every mutation. Cmd/Ctrl+Z / +Shift+Z (and the Edit
menu).

## 11. Multi-scene tabs — ✅
A tab strip; each tab owns a `DynamicWorld` snapshot. Switching snapshots the live scene and
restores the target's; "+" opens an empty tab.

## 12. Build / Export — ✅
*Build menu.* **Export Scene** saves the active scene. **Build Project** shells out to
`cargo build --release` on a worker thread and then **packages** the built binary + the
`assets/` directory into a shippable `dist/<binary>/` folder, reporting the path (artifact
discovery + bundle copy unit-tested).

## 13. Remote (BRP) editing — ✅
*File → Connect to Remote.* Connects to a running Bevy app with `RemotePlugin` +
`RemoteHttpPlugin` and both **queries and edits** it over BRP — spawn, despawn, and mutate a
component field (`world.spawn_entity` / `world.despawn_entity` / `world.mutate_components`), from
a remote-actions overlay. The low-level `brp_request` helper (and the typed `brp_spawn` /
`brp_despawn` / `brp_mutate` / `brp_query_entities`) are public for tooling.

---

## How to re-verify
```sh
cargo fmt --check
cargo clippy -p bevy_editor --all-targets
cargo test -p bevy_editor
cargo run --example editor --features bevy_editor   # exercise the interactive paths
```
