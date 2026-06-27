# bevy_editor — implementation status

A Phase-1 MVP of a Unity/Godot-style GUI editor for Bevy. This file is an exact,
honest accounting of what is done, what is wired-but-untested, and what is not built.

Run it: `cargo run --example editor --features bevy_editor`

## Verification legend
- **✅ Verified** — confirmed working by a screenshot of the running app (rendering /
  startup state). See the demo scene the example spawns (a cube + sphere, cube selected).
- **🟡 Wired (untested)** — implemented, compiles, integrated into the running app, but
  **not** behavior-verified, because interactive input (mouse clicks, drags, typing)
  could not be simulated in the headless environment used to build this. The code path
  exists and is reachable; it has not been exercised end-to-end.
- **❌ Not built** — not implemented; deferred to a later phase.

Everything below compiles cleanly: `cargo fmt --check`, `cargo clippy`, and the example
build all pass with zero warnings. The app launches, initializes rendering (Metal), and
runs without panics.

---

## 1. Crate + workspace wiring — ✅ Verified
- New `crates/bevy_editor/` crate; `EditorPlugins` plugin group.
- Feature wired through `crates/bevy_internal` (dep + `bevy_editor` feature + `pub use
  bevy_editor as editor`) and the root `Cargo.toml` (`bevy_editor` feature + `[[example]]
  editor`). Builds via `--features bevy_editor`.

## 2. UI shell — ✅ Verified (rendering)
- ✅ Window-filling paneled layout: menu bar (File / Edit / Entity / View), toolbar,
  body row (Hierarchy | Viewport | Inspector), Assets row. Dark Feathers theme. All
  panels, text, and toolbar buttons render correctly.
- 🟡 **Resizable splitters** between panels — drag observer mutates the neighbor panel's
  width (clamped 120–900 px). Not drag-tested.
- 🟡 Menu items / toolbar buttons — they render and are wired to actions/observers, but
  no click was simulated, so the *click → action* path is unverified.
- ❌ **Panel scrolling** — no overflow scroll. Long inspectors / hierarchies clip at the
  panel edge (visible: the inspector's lower components are cut off).
- ❌ Free/dockable window rearranging — panels are fixed-position (only width-resizable).

## 3. Viewport — ✅ Verified (rendering); controls 🟡
- ✅ Offscreen scene camera → `ViewportNode` in the center panel; the 3D scene (infinite
  grid + lit cube + sphere) renders inside the panel.
- ✅ Infinite reference grid (3D), on the scene render layer.
- 🟡 **3D orbit/pan/zoom camera** (`Editor3dCamera`): RMB-orbit, MMB-pan, wheel-zoom.
  Implemented; not input-tested.
- 🟡 **Click-to-select** in the viewport (forwarded picking → `EditorSelection`). The
  forwarding mechanism is the same one Bevy's own `viewport_node` example uses; the
  selection observer is wired but no click was simulated.
- 🟡 **Escape clears selection.** Wired, untested.
- ⚠️ Camera input is **not** gated to "pointer over viewport" — RMB/MMB/wheel are read
  globally. Fine in practice (panels have no wheel-scroll yet) but not polished.

## 4. 2D / 3D modes — 🟡 Wired (3D verified, 2D not)
- ✅ 3D mode is the default and is fully rendered.
- 🟡 **2D mode**: `Camera2d` + `Editor2dCamera` (pan + zoom-by-scale) + sprite spawning +
  sprite picking. The `View → Toggle 2D/3D` and toolbar `2D/3D` actions rebuild the scene
  camera. Implemented but **not** verified — the screenshots are all 3D.
- ⚠️ Known unknown: the gizmo/grid use 3D gizmo drawing, which may not render under a 2D
  camera. 2D drag-to-move and inspector still work regardless.

## 5. Hierarchy panel — ✅ Verified (rendering); editing 🟡
- ✅ Live entity tree of everything tagged `SceneEntity` (shows Directional Light, Cube,
  Sphere). Depth-based indentation.
- ✅ Selection highlight (the selected Cube row is highlighted).
- 🟡 Row click → select (Ctrl/Cmd = additive). Wired, not click-tested.
- 🟡 **Spawn** Cube / Sphere / Plane / Point Light / Directional Light / Sprite / Empty
  via the *Entity* menu. The underlying `spawn_kind` is ✅ verified (the demo uses it to
  create the cube + sphere); the *menu → spawn* trigger path is 🟡 untested.
- 🟡 **Delete Selected** via the *View* menu. Wired, untested.
- ❌ **No right-click context menu.** ❌ No **rename**, ❌ **duplicate**, ❌ **reparent**,
  ❌ drag-drop. (The tree is effectively a flat indented list — no collapse/expand
  disclosure toggles, and spawned entities are not parented.)

## 6. Inspector — ✅ Verified (rendering); editing partial
- ✅ Generic, reflection-driven: enumerates the selected entity's components via the type
  registry and walks each component's fields. Renders sections (`GlobalTransform`,
  `Transform`, `Visibility`, `Mesh3d`, …) with the right values.
- ✅ **Numeric (`f32`) fields are editable widgets** — Feathers number inputs, including
  nested struct fields (`Transform.translation.x/y/z`, rotation quat, scale). The screenshot
  shows the cube's live values (-1.2, 0.5, 0).
- 🟡 **Write-back**: editing a number writes to the component via `ReflectComponent::
  reflect_mut` + a reflect path. Wired; not typed-in-tested.
- 🟡 **Reverse sync** (entity changes elsewhere → number updates, skipping the focused
  field). Wired; not tested.
- ❌ **Only `f32`/`f64` are editable.** `bool`, `String`, `Color`, enums, lists, `Option`,
  etc. are shown **read-only** (as a debug string), not as checkboxes/text-inputs/swatches/
  dropdowns.
- ❌ **No "Add Component" / "Remove Component" UI.**

## 7. Transform gizmo — partial
- ✅ Visual translate gizmo: X/Y/Z axis arrows drawn at the selection (visible on the cube).
- 🟡 **Drag-to-translate**: left-drag a scene entity to move it in the camera's view plane
  (3D) or screen plane (2D). Wired; not drag-tested.
- ❌ **Not axis-constrained** — dragging moves freely in the plane, not along a single
  picked axis handle. (Use the inspector's per-axis number fields for precise/constrained
  edits.)
- ❌ **No rotate or scale gizmo.** The toolbar Rotate/Scale buttons set `GizmoMode` but
  only Translate is implemented; Rotate/Scale are currently no-ops.

## 8. Scene save / load + asset browser — 🟡 Wired; browser ✅ renders
- ✅ Asset panel renders ("No saved scenes" when empty).
- 🟡 **New / Save / Save As / Open** via the File menu. Implemented; not click-tested.
- 🟡 Asset browser lists `assets/scenes/*.ron` and opens on click. The listing/refresh
  logic is wired; not tested with real files.
- ⚠️ **Custom file format, not `.scn.ron`/`DynamicWorld`.** Scenes save as a small
  editor-controlled RON (one node per entity: `SpawnKind` + transform), and primitives are
  rebuilt fresh on load. This is deliberate: runtime-generated mesh/material handles don't
  survive serialization across a despawn, so a full `DynamicWorld` file wouldn't restore
  geometry. Consequence: only entities created through the editor's spawn kinds round-trip;
  arbitrary user components are **not** saved to file.
- ❌ **No file picker dialog** — Save/Open use a fixed default name (`scene.ron`).

## 9. Play / Pause / Stop — 🟡 Wired (not tested)
- 🟡 **Snapshot on play, restore on stop** via `DynamicWorld` (in-memory, so asset handles
  stay valid — unlike file save). Entering play snapshots all `SceneEntity`s; Stop despawns
  and restores them.
- 🟡 **Demo behavior**: scene meshes spin while `Playing`; Stop reverts them via the
  snapshot. Gated with `run_if(in_state(Playing))`, so Pause freezes it.
- Not verified — Play/Pause/Stop were not clicked. The toolbar buttons set the state.
- ⚠️ Components without `ReflectComponent` registration are dropped from the snapshot
  (they won't restore). Core components (Transform, Name, mesh/material handles, lights)
  are registered and should restore.

---

## Not built at all (explicitly deferred)
- Undo / redo (no command history).
- Hierarchy: context menu, rename, duplicate, reparent, drag-drop.
- Inspector: non-numeric field editing, add/remove component, enum/Option/list handling.
- Gizmo: rotate, scale, axis-constrained handles, snapping.
- Panel scrolling; free docking; multi-scene tabs.
- Asset import / thumbnails; prefabs / `bsn!` authoring.
- File picker dialog; project management; build/export; scripting.
- Out-of-process / remote (BRP) editing mode.

## Honest summary of verification
Everything that **renders at startup** is screenshot-verified: the full shell, the 3D
viewport with grid + meshes, the live hierarchy with selection highlight, the
reflection-driven inspector with editable numeric Transform fields, the translate gizmo on
the selection, and the asset panel. Every **interactive gesture** (clicking menus/toolbar,
dragging splitters/gizmo/camera, typing into the inspector, Save/Open/Play) is implemented,
compiled, and integrated, but was **not** exercised, because the build environment could
not inject input events. Those paths should be validated by running the editor locally.
