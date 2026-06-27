# bevy_editor — remaining work

A large UI overhaul is **done** (icon toolbar with active-state highlighting, themed dialogs via a
shared `dialog_frame`, status bar, toast notifications, command palette, in-editor log console,
light/dark themes, viewport selection outline + frame-to-selection, keyboard shortcuts,
add-component search, plus robustness/perf fixes — see `STATUS.md` §14–15). The items below are
**not yet done**.

## Working in this crate

```sh
cargo clippy -p bevy_editor --all-targets   # must stay zero-warning
cargo test  -p bevy_editor                  # 25 tests, must stay green
cargo fmt   -p bevy_editor

# Headless screenshot: renders the UI to an offscreen image, writes a PNG, then exits.
EDITOR_SCREENSHOT=/tmp/shot.png cargo run --example editor --features bevy_editor
# Capture a specific surface first (see examples/editor/editor.rs):
EDITOR_SCREENSHOT=/tmp/shot.png EDITOR_SHOT_OPEN=palette cargo run --example editor --features bevy_editor
#   EDITOR_SHOT_OPEN ∈ {save, import, palette, console, theme, toast}
```

### `bsn!` macro gotchas (you will hit these)
- Component args can't be method calls: `ThemeBorderColor(token.clone())` fails — precompute
  `let t = token.clone();` then `ThemeBorderColor(t)`.
- Enum/tuple component *values* (e.g. `MyEnum::Variant(x)`) must be wrapped as
  `template_value(MyEnum::Variant(x))`, and the type must `#[derive(Default)]`.
- `bsn!{ A B }` allows only ONE root entity; for multi-element button captions wrap in a single
  `Node { ... Children [ ... ] }`.
- `(some_fn() Children [...])` and `(some_fn() ExtraComponent)` both work (patches onto a
  fn-returned scene).
- `text.value()` returns a parley `SplitString`, not `&str` — use `.value().to_string()`.

### Reusable helpers (use these; don't reinvent)
- `crate::ui::style::dialog_frame(title, width: Val, body: impl Scene)` — centered themed modal
  (dimming scrim, border, shadow, title bar). Dismissed by the shared `crate::ui::CloseOverlay` /
  Escape.
- `crate::ui::style::{field_row, section_header, status_segment}` and the `space::`/`sizes::`/
  `z::`/`etokens::` constants.
- `crate::ui::ShowToast::{info,success,warning,error}(text)` — trigger via `commands.trigger(...)`
  or `world.trigger(...)`.
- Icons: `crate::ui::icons::*` constants; embedded in `lib.rs::embed_editor_icons`; PNGs in
  `src/assets/icons/`; regenerate with `tools/gen_icons.mjs`.

---

## 1. Inspector color picker (biggest gap)
**Current:** `Color` components render as 4 raw number fields (R/G/B/A) — see
`NumTy::{ColorR,ColorG,ColorB,ColorA}` and `number_field`/`collect_components` in
`src/inspector/mod.rs` (grep `ColorR`). They have colored sigils but no swatch/picker.
**Wanted:** a `FeathersColorSwatch` row (read/write `ColorSwatchValue(Color)`) that, on click,
opens a small `dialog_frame` floating picker with a `FeathersColorPlane` (HueSaturation) + a hue
`FeathersColorSlider` + an alpha slider, round-tripping through the existing `FieldBinding`
reflect-write path.
**Read:** `crates/bevy_feathers/src/controls/{color_swatch,color_plane,color_slider}.rs`.
`ColorPlanePlugin`/`ColorSwatchPlugin` are already in `FeathersPlugins`.

## 2. Inspector Vec3 grouping (usability gap)
**Current:** axis fields are individually colored (red **X** / green **Y** / blue **Z** inputs),
but translation / rotation / scale are stacked as separate rows with no group label — you can't
tell which X/Y/Z belongs to which. See `axis_label`/`number_field` in `src/inspector/mod.rs`.
**Wanted:** detect a Vec3/Quat component field in `collect_components` and emit ONE labeled row
(`Translation: [X][Y][Z]`) instead of three unlabeled rows.

## 3. Hierarchy search / filter
**Current:** no search in the hierarchy. The Add Component dialog HAS one — copy that pattern:
`filter_add_component` + `AddComponentSearch` in `src/inspector/mod.rs`.
**Wanted:** a `FeathersTextInput` in the hierarchy panel header (`dockable_header` in
`src/ui/shell.rs`), a `HierarchyFilter(String)` resource, and filtering in `rebuild_hierarchy`
(`src/hierarchy/mod.rs`) that keeps matches **and their ancestors**. Mark `HierarchyDirty` on
filter change.

## 4. Menu keyboard-accelerator hints
**Current:** shortcuts work (`src/ui/shortcuts.rs`) but menus don't show them. `menu_item(icon,
text)` in `src/ui/shell.rs` builds each item.
**Wanted:** extend `menu_item` to take an optional accelerator string and render it right-aligned
dim (e.g. `flex_spacer()` + `label_dim("⌘S")`). Apply to File/Edit menus (Save=⌘S, Undo=⌘Z, …).

## 5. Inspector write-back failure feedback
**Current:** the reflection write paths (`write_numeric`, `apply_patch`, `cycle_enum`,
`on_bool_changed`, … in `src/inspector/mod.rs`) silently `return` on failure.
**Wanted:** on a failed `reflect_mut`/`path_mut`/`insert`, `commands.trigger(ShowToast::warning(
"Couldn't set <field>"))`. (Save/open/import/build already toast.)

## 6. Remote (BRP) robustness + tests
**Current:** `crate::brp_request` and `parse_entity_ids`/`normalize_addr` in `src/remote.rs` do
fragile manual HTTP/JSON parsing with no timeouts/size guards; the dialogs were modernized but the
parsing wasn't hardened, and there are no unit tests for `parse_entity_ids`/`normalize_addr` (both
`pub`).
**Wanted:** wrap parse points in `Result`, add a response-size cap, surface failures as toasts, and
add `#[test]`s for `parse_entity_ids`/`normalize_addr`.

## 7. Viewport HUD overlay (optional — status bar covers most of it)
**Wanted:** a new `src/viewport/hud.rs` — `Pickable::IGNORE` overlay nodes inside the
`ViewportSlot` showing gizmo mode (W/E/R) + snap + camera-control hints, using
`etokens::HUD_BG`/`HUD_TEXT` (already defined in `src/ui/style.rs`). Register in `ViewportPlugin`
(`src/viewport/mod.rs`).

## 8. Minor perf (low priority — already change-gated)
- `rebuild_hierarchy` (`src/hierarchy/mod.rs`): add a 1–2 frame debounce so a multi-entity
  reparent/undo burst collapses into one rebuild; optionally patch a single visible row's `Text`
  in place on a lone `Changed<Name>`.
- `rebuild_asset_browser` (`src/scene_io.rs`): cache the `read_dir` listing in a resource and keep
  `AssetBrowserDirty` precise (it's already only set on save/import/startup).

## 9. (Optional) dedicated `editor_verify` behavioral example
Verification currently rests on 25 unit tests + the screenshot harness. The original plan also
wanted a hidden `examples/editor/editor_verify.rs` (+ a `[[example]]` block in the root
`Cargo.toml`) that drives `EditorPlugins` for N `App::update()`s and asserts invariants (shell root
exists, hierarchy rows == `SceneEntity` count, inspector populates on selection, save→open
round-trips). Nice-to-have, not blocking.

---

**Acceptance per item:** `cargo clippy -p bevy_editor --all-targets` zero-warning,
`cargo test -p bevy_editor` green, and (for visual items) confirm with the `EDITOR_SCREENSHOT`
capture above.
