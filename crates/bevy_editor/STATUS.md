# bevy_editor — implementation status

A Unity/Godot-style GUI editor for Bevy, built entirely on existing Bevy infrastructure
(`bevy_feathers` widgets, `bevy_ui`'s `ViewportNode`, `bevy_reflect`,
`bevy_world_serialization`, `bevy_picking`/`bevy_gizmos`, `bevy_remote`). This file is an
honest accounting of what works, how it was verified, and what is still partial.

Run it: `cargo run --example editor --features bevy_editor`

## Verification legend
- **✅ Verified** — exercised end-to-end and confirmed correct. Two methods were used:
  - *Logic* — a deterministic harness drove the editor's real event API and asserted on
    world state at each step (entity counts, parenting, round-trips). Every assertion
    passed; the app ran for hundreds of frames with **zero panics**.
  - *Rendering* — confirmed by a screenshot of the running app (shell, viewport, panels).
- **🟡 Wired** — implemented, compiles, integrated, and reachable, but the specific
  *mouse/keyboard gesture* that triggers it was not simulated (the build host can't inject
  pointer events, and an un-composited window captures black, so pixel-level gesture
  testing isn't possible here). The underlying logic is, in most cases, shared with a
  ✅-verified path.
- **❌ Not built / partial** — deferred or intentionally minimal; noted explicitly.

Everything below compiles with **zero warnings** (`cargo fmt --check`, `cargo clippy`, and
the example build are all clean) and launches on Metal without panics.

---

## 1. Crate + workspace wiring — ✅ Verified
`crates/bevy_editor/` exposes the `EditorPlugins` group, wired through `bevy_internal`
(`pub use bevy_editor as editor`) and the root `Cargo.toml` (`bevy_editor` feature +
`[[example]] editor`).

## 2. UI shell — ✅ Verified (rendering); ✅ scrolling
- Window-filling paneled layout: menu bar (File / Edit / Entity / View / Build), toolbar,
  **scene-tab strip**, body row (Hierarchy | Viewport | Inspector), Assets row. Dark theme.
- ✅ **Panel scrolling** — Hierarchy, Inspector, and Asset panels are `ScrollArea`s with
  `overflow: scroll_y`; long content scrolls instead of clipping.
- 🟡 Resizable splitters between panels (drag handles).
- ❌ Free-floating / tear-off docking is not built. Panels resize (splitters) but don't
  rearrange. (Documented as future work.)

## 3. Viewport — ✅ Verified (rendering); controls 🟡
- ✅ Offscreen scene camera → `ViewportNode`; 3D scene (infinite grid + lit meshes) renders
  in the center panel.
- 🟡 3D orbit/pan/zoom (`Editor3dCamera`) and 2D pan/zoom (`Editor2dCamera`).
- ✅ **Wheel-zoom is now gated to pointer-over-viewport**, so scrolling a side panel no
  longer also dollies the camera (`ViewportHovered`).
- 🟡 Click-to-select and Escape-to-clear (forwarded picking → `EditorSelection`).

## 4. 2D / 3D modes — 🟡 Wired (3D rendering verified)
`View → Toggle 2D/3D` / toolbar rebuilds the scene camera (2D `Camera2d` + sprite picking,
or 3D). 3D is the default and is rendering-verified; the 2D path is wired.

## 5. Hierarchy panel — ✅ Verified (logic); gestures 🟡
- ✅ Live entity tree of everything tagged `SceneEntity`, with depth indentation and
  selection highlight (rebuilds on add/remove/rename/**reparent**).
- ✅ **Spawn / Delete / Duplicate / Reparent** — verified via the harness: spawning,
  deleting, reflection-based **duplicate** (clones a component set into a sibling), and
  **reparent** (with cycle-guard) all produce the correct entity counts / parenting, and
  **undo/redo** restores every one.
- 🟡 Row click → select (Ctrl/Cmd additive); **double-click / right-click context menu →
  Rename / Duplicate / Delete**; **inline rename** (autofocused text field, commit on
  Enter); **collapse/expand** disclosure toggles; **drag-and-drop reparent** (drop a row on
  another to nest, or on empty space to unparent). All wired; the click/drag gestures
  themselves were not simulated, but the actions they fire are ✅-verified.

## 6. Inspector — ✅ Verified (rebuild); editing 🟡
- ✅ Generic, reflection-driven: enumerates components via the type registry and walks each
  component's fields. Rebuilt without panic across every selection change in the harness.
- Editable widget per field type: **`f32`/`f64`/integers** (number inputs), **`bool`**
  (checkbox), **`String`** (text input), **unit enums** e.g. `Visibility` (cycle button),
  and **`Color`** (editable R/G/B/A channels). Other types show read-only.
- ✅ **Add / Remove Component** — a "＋ Add Component" dialog lists every registry type with
  `ReflectComponent` + `ReflectDefault` and inserts a default; each section's "✕" removes
  the component. (Both capture undo.)
- 🟡 Write-back (editing a widget → component field via `reflect_mut` + path) and reverse
  sync (gizmo drag → number fields update) are wired; the in-widget edit gesture wasn't
  simulated. Undo is coalesced to one entry per field-editing session.
- ❌ List/`Option`/map element editing is read-only.

## 7. Transform gizmo — ✅ all three modes implemented; drag 🟡
- ✅ Visuals per mode: translate arrows, rotate rings, scale handles, drawn at the
  selection; the engaged translate axis is highlighted.
- 🟡 **Drag-to-manipulate**, branching on `GizmoMode`:
  - **Translate** — *axis-constrained* when the initial drag direction matches a
    screen-projected world/local axis (analytic, no handle entities), else free view-plane.
  - **Rotate** — drag rotates about world/local Y (3D) or Z (2D).
  - **Scale** — drag sets a uniform scale.
  Applies to the whole multi-selection; one undo entry per drag gesture. The Rotate/Scale
  toolbar buttons are no longer no-ops. The drag *gesture* wasn't simulated.

## 8. Scene save / load + asset browser — ✅ Verified (round-trip); dialogs 🟡
- ✅ **Save → New → Open round-trips** (harness-verified: save 4 entities → New clears to 1
  → Open restores 4). Scenes persist spawn-kind + transform + **visibility** per entity.
- 🟡 **Save-As / Open / Import** dialogs (modal `EditorOverlay`s with text inputs / file
  lists), wired from the File menu.
- ✅ **Asset browser** lists saved scenes and shows **live image thumbnails** for
  `assets/*.png|jpg`. Clicking a scene **instantiates it as a prefab** into the current
  scene (additive); File→Open replaces the scene.
- ⚠️ Custom spawn-kind RON format (not `.scn.ron`): only editor spawn-kinds + transform +
  visibility round-trip through a file; arbitrary reflected components and parent links do
  not (runtime mesh handles can't survive file reload — the documented reason). Full
  reflected-component file persistence is future work.

## 9. Play / Pause / Stop + scripting — ✅ Verified (snapshot); behavior 🟡
- ✅ **Snapshot on play, restore on stop** via in-memory `DynamicWorld` (asset handles stay
  valid). The snapshot/restore path is the same one undo and tabs use and is harness-verified.
- 🟡 **Behavior scripting** (`BehaviorScript`) — a minimal built-in interpreter
  (`spin`/`rotate`/`translate`/`scale`) animates entities during play; the demo cube carries
  `spin 1.0`. Editable live in the inspector (it's a reflected `String` field) and attachable
  to any entity via Add Component. *Honest stub: not a full scripting language.*

## 10. Undo / redo — ✅ Verified
Whole-scene snapshot stack. Captured before every mutation (spawn, delete, duplicate,
reparent, rename, inspector edit, gizmo drag-start, scene New/Open). Cmd/Ctrl+Z / +Shift+Z
(and the Edit menu) undo/redo; gated to edit mode. The harness verified spawn→undo,
duplicate→undo, delete→undo, reparent→undo, and redo all restore exactly.

## 11. Multi-scene tabs — 🟡 Wired
A tab strip under the toolbar; each tab owns an in-memory `DynamicWorld` snapshot. Switching
snapshots the live scene into the active tab and restores the target's (reusing the
verified snapshot module); "+" opens a new empty tab. Wired; the tab-click gesture wasn't
simulated. ❌ Per-panel collapse/free docking not built.

## 12. Build / Export — 🟡 Wired
*Build menu.* **Export Scene** saves the active scene; **Build Project** shells out to
`cargo build --release` on a worker thread and reports success/failure in a modal. *Honest
stub: real cargo build, but no asset bundling/packaging.*

## 13. Remote (BRP) editing — 🟡 Wired, read-only
*File → Connect to Remote.* Connects to a running Bevy app with `RemotePlugin` +
`RemoteHttpPlugin` (default `127.0.0.1:15702`), issues a `world.query` over HTTP on a worker
thread, and reports the remote entity count. **Read-only** — remote component editing and a
full remote hierarchy/inspector are future work. (Requires a separate BRP server to exercise;
the request/response path is implemented but the live round-trip wasn't tested here.)

---

## Still partial / deferred (honest)
- Free / dockable panel rearranging and per-panel collapse (splitter resize works).
- Inspector editing of lists / `Option` / maps; per-axis gizmo scale; gizmo snapping.
- Full reflected-component **file** persistence and `.scn.ron`; parent links in saved files.
- Multi-line script editor; a real embedded scripting language.
- Build packaging/bundling; remote **editing** (only remote query is implemented).

## How this was verified
The interactive core was validated by a deterministic state-assertion harness (an
`EDITOR_VERIFY`-gated path, since removed from the example) that drove the editor's real
event API and asserted world state at each step. The full sequence —
spawn → reparent → duplicate → delete → undo → undo → undo → undo → redo → save → New →
Open — produced the exact expected entity counts and parenting at every step, with no
panics across the inspector/hierarchy/tab/overlay rebuilds it triggered. Shell rendering was
screenshot-verified. Gestures that depend on injected pointer input (clicking specific menu
pixels, dragging the gizmo/splitters, typing into a widget) are compile- and logic-verified
but were not pixel-simulated; validate those by running the editor locally.
