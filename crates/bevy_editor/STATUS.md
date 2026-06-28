# bevy_editor — implementation status

A Unity/Godot-style GUI editor for Bevy, built entirely on existing Bevy infrastructure
(`bevy_feathers` widgets, `bevy_ui`'s `ViewportNode`, `bevy_reflect`,
`bevy_world_serialization`, `bevy_picking`/`bevy_gizmos`, `bevy_remote`). This file is an
honest accounting of what works and how it is verified — **everything below is ✅**.

Run it: `cargo run --example editor --features bevy_editor`

The UI is icon-driven and fully themed (light + dark), with a status bar, toast notifications,
a command palette, an in-editor log console, and a consistent modal-dialog system — see
section 14 for the full design-system / polish summary.

## Verification

- **Unit tests** (`cargo test -p bevy_editor`, 25 tests) cover the logic-heavy pieces with no
  GPU/UI needed: the scripting language (lexer/parser/evaluator + error cases), the `.scn.ron`
  serialize→deserialize round-trip (parent links + arbitrary components) and the scene-name
  display logic, the inspector list/option/map reflection ops + axis-sigil and add-component
  filtering, the gizmo axis-pick + snapping math, and the build packaging.
- **Interactive paths** (pointer/keyboard gestures) are wired to the same action/observer code
  the unit-tested logic uses; exercise them by running the `editor` example. Each gesture is
  noted below with the action it drives.
- **Visual capture**: `EDITOR_SCREENSHOT=<path> cargo run --example editor --features bevy_editor`
  renders the UI to an offscreen target and writes a PNG, then exits — a headless way to verify
  the look without a visible window.

Everything compiles **zero-warning** (`cargo fmt --check`, `cargo clippy --all-targets`, and the
`editor` example) and launches on Metal without panics.

---

## 1. Crate + workspace wiring — ✅
`crates/bevy_editor/` exposes the `EditorPlugins` group, wired through `bevy_internal`
(`pub use bevy_editor as editor`) and the root `Cargo.toml` (`bevy_editor` feature + the
`editor` example).

## 2. UI shell + panels — ✅
- Window-filling paneled layout: menu bar (Project / File / Edit / Entity / GameObject / View /
  Build), toolbar, scene-tab strip, the **Hierarchy / Viewport / Inspector / Assets** panels, and
  a status bar. Light + dark themes.
- **Fixed panels** (`ui/shell.rs`) — Hierarchy on the left, Inspector on the right, Assets along
  the bottom of the center column, with a plain icon+title header each. (An earlier experiment with
  draggable / tabbable / floatable panels was removed — see §28.)
- **Panel scrolling** — Hierarchy / Inspector / Asset panel bodies are `ScrollArea`s.
- **Resizable panels** — splitter handles (`ui/splitter.rs`) resize the neighboring panel:
  horizontal handles for the Hierarchy/Inspector columns, a vertical handle for the Assets row.

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

## 14. UI design system + flagship polish — ✅
A release-grade visual overhaul, all built on the existing `bevy_feathers` primitives:

- **Design system** (`ui/style.rs`, `ui/icons.rs`): centralized spacing / size / z-layer
  constants, an `editor.*` theme-token set layered onto the Feathers theme, reusable scene
  builders (`field_row`, `section_header`, `dialog_frame`, `status_segment`), and **59 embedded
  monochrome icons** (Lucide, ISC) that tint to the theme via `ThemedIcon`.
- **Icon-driven shell** (`ui/shell.rs`): the toolbar is icon buttons with **active-state
  highlighting** (the live run-state / gizmo-mode / snap button lights up), menus carry leading
  icons, panels have icon headers + borders, and the hierarchy shows a per-entity **type icon**
  (cube / sphere / light / sprite …) plus chevron disclosure.
- **Themed modals** (`style::dialog_frame`): every dialog (save / open / import / script editor /
  remote / build / add-component / command palette) is a centered, bordered, shadowed, rounded
  modal with a title bar and a dimming scrim, replacing the old flat overlays.
- **Status bar** (`ui/status_bar.rs`): viewport mode, selection count, gizmo, snap, scene name +
  dirty marker, and FPS.
- **Toast notifications** (`ui/toast.rs`): `ShowToast` cards (info / success / warning / error)
  stack bottom-right and auto-dismiss; wired to save / open / import / build feedback.
- **Command palette** (`ui/command_palette.rs`): `Cmd/Ctrl+P`, fuzzy-filtered, runs any editor
  action by name.
- **Console** (`ui/console.rs`): a toggleable bottom log panel (`` ` ``) capturing real `tracing`
  output via a `LogPlugin` custom layer (`editor_console_layer`), level-colored + monospace.
- **Light + dark themes** (`ui/theme_switch.rs`): `ToggleTheme` swaps the whole `UiTheme`; the
  light theme is derived from the dark one by lightness inversion of neutral tokens.
- **Viewport selection outline** (`viewport/outline.rs`): a wireframe box around each selected
  entity, plus `F` to frame the selection (3D orbit + 2D pan cameras).
- **Keyboard shortcuts** (`ui/shortcuts.rs`): Delete, F2, `Cmd/Ctrl+D/S/N/O/P`, W/E/R gizmo
  modes, F frame, `` ` `` console — suppressed while a text field is focused.
- **Add-component search** + **inspector axis coloring** (red/green/blue X/Y/Z inputs).

## 15. Robustness + performance — ✅
- Enum-cycle bounds guard, `is_descendant` cycle-depth cap, `duplicate_entity` half-built
  cleanup, and silent-failure feedback routed through toasts.
- `sync_number_fields` only runs during a gizmo drag or play mode (not every frame); undo/redo
  use a `VecDeque` (O(1) cap); scene open parses fully before clearing the live scene (atomic).

---

# Godot-parity expansion (in progress)

The sections below track the work to grow this from a scene editor into a full Godot-style engine
("make / run / code a game from scratch"). See the approved plan for the full roadmap.

## 16. Project model + scaffolding — ✅
`src/project.rs`. An editor **project** is a directory with a `project.bevy.ron` config (name,
default scene, recent scenes, build profile, input-action stubs). [`ActiveProject`] is the single
source of truth for where files live — scene I/O, the asset browser, cargo build, and the code
editor all resolve against its root (default: the working dir). **New / Open / Recent Project**
flows (Project menu + command palette), and **New-Project scaffolding** writes a runnable Bevy
cargo project from scratch (`Cargo.toml`, `src/main.rs`, `assets/scenes/`, `.gitignore`,
`project.bevy.ron`). Recent projects persist to `~/.bevy_editor/recent_projects.ron`. Unit-tested:
config RON round-trip, recent-scene de-dup, crate-name sanitization, scaffold output, recents cap.

## 17. Tabbed bottom dock + workspace persistence — ✅
`src/ui/bottom_dock.rs`. Replaces the single console strip with a **tabbed bottom dock** hosting
the **Console** and build **Output** tabs (extensible: add a `BottomTab` variant + a
`BottomTabContent` node). Open/active state is a serializable workspace persisted to
`~/.bevy_editor/layout.ron`. The console was migrated in as a tab.

## 18. Code the game in Rust — ✅
`src/code.rs`. The center area switches (toolbar `</>`, palette) between the scene viewport and a
**Rust code editor** ([`MainView`]) that browses the active project's `src/**.rs`, edits a file in
a multi-line area, and saves it. **Cargo integration**: `cargo check` streams **clickable
diagnostics** (file:line → opens + the editor) into the Output dock; **Run** launches the game via
`cargo run` as a child process whose stdout/stderr is captured into Output; **Stop** kills it.
Runs on the worker-thread + poll pattern, scoped to the active project's profile/root. Unit-tested:
recursive `.rs` listing, cargo-JSON diagnostic parsing (spans + level filtering), main-view toggle.
*(Follow-ups: Rust syntax highlighting, rust-analyzer LSP, dylib hot-reload — see TODO.)*

## 19. Inspector grouping + color swatch — ✅
`src/inspector/mod.rs`. `Vec2`/`Vec3`/`Vec4`/`Quat` render as **one labeled row** with colored
axis inputs side-by-side (e.g. `Translation: [X][Y][Z]`); `Color` shows a clickable **preview
swatch** above its editable R/G/B/A channels. Unit-tested vector detection. *(Interactive color
picker + `PropertyEditorRegistry` now done — see §26.)*

## 20. Project settings + input map — ✅
`src/project.rs`. A **Project Settings** dialog edits the name, default scene, build profile
(Debug/Release), and cross-compile **target triple**; an **Input Map** dialog adds/removes named
action→key bindings. Both persist to `project.bevy.ron` (Project menu + palette).

## 21. Asset import dock — ✅
`src/scene_io.rs`. The Assets panel is now a **recursive folder tree** of the project's `assets/`
(folders first, file-type icons, inline image thumbnails) with an **Import Asset** button; scene
files instantiate on click.

## 22. Debugger/Stats + Animation panels — ✅
`src/diagnostics.rs`, `src/animation.rs`. A **Stats** dock tab shows live FPS / frame time /
entity + scene-entity counts / selection / run-state with a **Step Frame** button (single-step
game logic while paused). An **Animation** tab drives the selection's `AnimationPlayer`
(pause/resume/restart all clips).

## 23. Material editor + shaders + UI editor — ✅
`src/material.rs`, `src/code.rs`, `src/ui_edit.rs`. A **Material** dock tab edits the selection's
`StandardMaterial` (base-color R/G/B/A, metallic, roughness) live via sliders. The code editor's
file list also lists **`.wgsl`** shaders for editing. The Entity menu spawns **UI nodes** (Node /
Text) that preview *inside the viewport* (bound to the scene camera) and serialize with the scene.

## 24. First-party physics / particles / tilemap — ✅
`src/gameplay.rs`. Dependency-free, in-tree engine features (mature third-party crates target
released Bevy, not this fork): a **physics** integrator (`RigidBody` velocity + gravity + ground
bounce), a **particle** emitter (`ParticleEmitter`, transient particles cleaned up on Stop), and a
**tilemap** (`Tilemap` grid of tile sprites, rebuilt on change). All are reflected components
(inspector-editable, serialized) and spawnable from the **GameObject** menu / palette. Physics +
particles run only in play mode.

## 25. Audio mixer + multi-target export — ✅
`src/audio.rs`, `src/build_export.rs`. An **Audio** dock tab scales Bevy's `GlobalVolume` with a
master-volume slider. **Build Project** honors the project's profile **and target triple**
(`cargo build [--release] [--target <triple>]`) for multi-platform export.

---

## 26. Remaining TODO items — ✅ (all complete)
The previously-deferred polish + the 9 numbered TODO items are now done:

- **Inspector write-back feedback** (`src/inspector/mod.rs`): the reflect-write paths
  (`write_numeric`/`apply_patch`/`cycle_enum`/`on_bool_changed`/string-commit) now surface a
  warning toast (`warn_set`) instead of failing silently.
- **Interactive color picker** (`src/inspector/mod.rs`): the `Color` swatch is a clickable
  `FeathersColorSwatch` button that opens a `dialog_frame` popup — a HueSaturation
  `FeathersColorPlane` + lightness/alpha `FeathersColorSlider`s + a live preview — round-tripping
  through the existing reflect-write path (`ActiveColorEdit` + `sync_picker_widgets`).
- **`PropertyEditorRegistry`** (`src/inspector/mod.rs`): a per-`TypeId` registry consulted first in
  `push_field`, with `Color` and `Vec2/3/4`/`Quat` registered as built-in editors; falls back to
  the built-in dispatch. Unit-tested (custom-editor override + fallback).
- **Hierarchy search** (`src/hierarchy/mod.rs`, `src/ui/shell.rs`): a search box in the hierarchy
  header drives `HierarchyFilter`; `rebuild_hierarchy` keeps matches **and their ancestors**.
- **Menu accelerators** (`src/ui/shell.rs`): `menu_item_accel` shows right-aligned dim shortcuts
  (⌘N/⌘O/⌘S/⇧⌘S/⌘Z/⇧⌘Z/⌘D/⌘P/Del/F/`` ` ``); Undo/Redo (⌘Z/⇧⌘Z) added to `src/ui/shortcuts.rs`.
- **BRP robustness + tests** (`src/remote.rs`): connect/read/write timeouts + a 4 MiB response cap
  in `brp_request`; hardened `parse_entity_ids`; failure/success toasts; `#[test]`s for
  `parse_entity_ids` + `normalize_addr`.
- **Viewport HUD** (`src/viewport/hud.rs`): a `Pickable::IGNORE` overlay shows gizmo mode / snap /
  space + camera hints, themed with `etokens::HUD_*`.
- **Inline Rust syntax highlighting** (`src/code_highlight.rs`, `src/code.rs`): a dependency-free
  lexer feeds a colored read-only `Text`+`TextSpan` layer rendered behind a glyph-transparent
  `EditableText` (shared monospace + `NoWrap`); the colored layer tracks the editor's `TextScroll`.
  Syntax colors are theme tokens (`etokens::SYNTAX_*`). Lexer unit-tested.
- **Game-UI theme-token editor** (`src/theme_editor.rs`) + **localization string-table editor**
  (`src/localization.rs`): two new bottom-dock tabs (`BottomTab::{Theme, Localization}`) with
  add/edit/remove rows that persist to `assets/theme.ron` / `assets/localization.ron`. RON
  round-trips unit-tested.
- **2D tilemap painting** (`src/gameplay.rs`): `Tilemap` gained a serialized `tiles: Vec<u32>`;
  clicking a tile of the selected map paints it with the `TilePaint` brush (number keys `0`–`8`).
  Index math unit-tested.
- **Perf** (`src/hierarchy/mod.rs`, `src/scene_io.rs`): `rebuild_hierarchy` debounces bursts;
  `rebuild_asset_browser` skips re-spawning when the directory listing is unchanged.
- **`editor_verify` example** (`examples/editor/editor_verify.rs`): boots `EditorPlugins`, runs
  ~110 frames, and asserts shell/hierarchy-row-count/inspector-populate/spawn invariants.

Tests: **59** green (up from 36). New `EDITOR_SHOT_OPEN` arms: `themeeditor`, `localization`,
`codehl` (+ `EDITOR_SHOT_FILE`).

---

## 28. Godot-style feature expansion — ✅

A second wave bringing the editor closer to a full Godot/Unity-style engine. All gated by change
detection / dirty flags and covered by unit tests + `editor_verify`.

- **Fixed panels** (`src/ui/shell.rs`, `src/ui/splitter.rs`): the layout is Hierarchy (left),
  Inspector (right), and Assets (bottom of the center column), each a fixed panel with a plain
  header, separated by resize splitters. (A draggable/tabbable/floatable docking experiment was
  built and then removed at the user's request — `docking.rs` is gone; only the resize splitters
  remain.)
- **Keyboard-shortcuts cheat sheet** (`src/ui/help_overlay.rs`): a data-driven `?`/`F1` overlay
  (also View menu + palette), categorized keycap rows.
- **Tilemap editor** (`src/tilemap.rs`): a **Tilemap** dock tab with a visual tile palette (click a
  swatch to set the brush, active swatch ringed) and non-destructive grid resize, on top of the
  existing in-viewport painter. Spawning a tilemap auto-opens the palette.
- **Keyframe animation editor** (`src/animation.rs`): replaced playback-only controls with a real
  in-tree **timeline** — an `EditedAnimation` reflected component with per-channel keyframe tracks
  over `Transform` (position/rotation/scale XYZ). Controls (Play/Stop/Add Key/Remove Key/Duration),
  a scrub slider playhead, and per-channel lanes with keyframe diamonds. Pose → Add Key → Play.
  Serializes with the scene.
- **UI / canvas layout editor** (`src/ui_edit.rs`): a **UI** dock tab with a 3×3 Godot-style
  anchor-preset grid (corner pins / edge stretch / auto-margin centering), a Relative/Absolute
  toggle, Fill-Parent, and width/height presets — editing the selected node's `Node` live.
- **Perf**: `bind_ui_to_viewport` reduced from a per-frame full scan to acting only on unbound
  nodes / camera rebuilds.

The bottom dock now hosts 10 tabs (added **Tilemap**, **UI**).

---

## How to re-verify
```sh
cargo fmt --check
cargo clippy -p bevy_editor --all-targets            # zero warnings
cargo test -p bevy_editor                            # 73 tests
cargo run --example editor_verify --features bevy_editor   # behavioral invariants (exits 0)
cargo run --example editor --features bevy_editor    # exercise the interactive paths
# Visual capture of any surface (see examples/editor/editor.rs's EDITOR_SHOT_OPEN arms):
EDITOR_SCREENSHOT=/tmp/shot.png EDITOR_SHOT_OPEN=codehl cargo run --example editor --features bevy_editor
```
