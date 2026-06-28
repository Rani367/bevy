# bevy_editor — remaining work

## Status: all tracked items complete ✅

Every item that used to be listed here is now done (see `STATUS.md` §26 for the per-item summary
and file map). At a glance:

- Inspector: write-back failure **toasts**, interactive **color-picker popup**, generic per-`TypeId`
  **`PropertyEditorRegistry`** (Vec3 grouping was already done).
- **Hierarchy search/filter** (keeps matches + ancestors) and a 1–2 frame rebuild **debounce**.
- **Menu keyboard-accelerator hints** (+ ⌘Z/⇧⌘Z undo/redo shortcuts).
- **Remote (BRP) hardening** (timeouts, response cap, toasts) + unit tests.
- **Viewport HUD** overlay (gizmo mode / snap / space + camera hints).
- Inline Rust **syntax highlighting** (hand-rolled lexer → colored `TextSpan` layer behind a
  glyph-transparent `EditableText`, scroll-synced).
- Game-UI **theme-token editor** + **localization string-table editor** (two new bottom-dock tabs,
  persisted as RON).
- 2D **tilemap painting** (serialized `tiles`, click-to-paint, palette hotkeys `0`–`8`).
- Asset-browser **caching**, and the hidden **`editor_verify`** behavioral example.

Verification: `cargo clippy -p bevy_editor --all-targets` is zero-warning, `cargo test -p
bevy_editor` is green (**59 tests**), and `cargo run --example editor_verify --features bevy_editor`
exits cleanly with "all invariants held".

If you pick up new work, append it below and keep the reference material that follows.

---

## Working in this crate

```sh
cargo clippy -p bevy_editor --all-targets   # must stay zero-warning
cargo test  -p bevy_editor                  # 59 tests, must stay green
cargo fmt   -p bevy_editor

# Behavioral smoke test: boots EditorPlugins, asserts shell/hierarchy/inspector invariants, exits.
cargo run --example editor_verify --features bevy_editor

# Headless screenshot: renders the UI to an offscreen image, writes a PNG, then exits.
EDITOR_SCREENSHOT=/tmp/shot.png cargo run --example editor --features bevy_editor
# Capture a specific surface first (see examples/editor/editor.rs's take_screenshot):
EDITOR_SCREENSHOT=/tmp/shot.png EDITOR_SHOT_OPEN=palette cargo run --example editor --features bevy_editor
#   EDITOR_SHOT_OPEN ∈ {
#     save, import, palette, console, theme, toast,   # original surfaces
#     newproject, openproject, settings,              # project flows
#     code, codehl,                                   # Rust code editor (codehl opens a file;
#                                                     # EDITOR_SHOT_FILE overrides which file)
#     stats, material, animation, audio,              # bottom-dock tabs
#     themeeditor, localization,                      # game-theme + localization tabs
#     uinode, physics,                                # spawn UI node / physics cube
#   }
# This env-driven capture path is the verification harness for future agents: add a new
# `match` arm in `take_screenshot` (examples/editor/editor.rs) for any new surface, then diff
# the PNG. Logic-heavy pieces are covered by the unit tests above.
```

### `bsn!` macro gotchas (you will hit these)
- Component args can't be method calls: `ThemeBorderColor(token.clone())` fails — precompute
  `let t = token.clone();` then `ThemeBorderColor(t)`.
- Enum/tuple component *values* (e.g. `MyEnum::Variant(x)`) must be wrapped as
  `template_value(MyEnum::Variant(x))`, and the type must `#[derive(Default)]`.
- `TextFont` can't go through `template_value` (its `FontSourceTemplate` field needs the bsn
  template form) — write it as an inline `TextFont { font: FontSourceTemplate::Handle(...), ... }`
  struct literal (see `code_editor_area`/`mono` in `src/code.rs`).
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
  `z::`/`etokens::` constants (incl. `etokens::HUD_*` and `etokens::SYNTAX_*`).
- `crate::ui::ShowToast::{info,success,warning,error}(text)` — trigger via `commands.trigger(...)`
  or `world.trigger(...)`.
- A raw `EditableText` is fully editable on its own (click-to-focus + cursor handled by
  `bevy_ui_widgets`); style it with `TextFont`/`TextColor`/`TextCursorStyle`.
- Inspector field editors are pluggable per-`TypeId` via `PropertyEditorRegistry` (`src/inspector`).
- New bottom-dock tab: add a `BottomTab` variant + content node (see `src/ui/bottom_dock.rs` and
  `src/theme_editor.rs` / `src/localization.rs` as templates).
- Icons: `crate::ui::icons::*` constants; embedded in `lib.rs::embed_editor_icons`; PNGs in
  `src/assets/icons/`; regenerate with `tools/gen_icons.mjs`.
