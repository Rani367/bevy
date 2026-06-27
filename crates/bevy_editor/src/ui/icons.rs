//! Embedded editor icon paths.
//!
//! The PNGs live in `src/assets/icons/` and are embedded by [`crate::EditorPlugin`]
//! (see [`embed_editor_icons`]). They are monochrome (white on transparent) so the
//! Feathers `ThemedIcon` tints them to the active theme's text color — pass any of
//! these constants to [`bevy_feathers::display::icon`].
//!
//! Icons are rasterized from [Lucide](https://lucide.dev) (ISC License); regenerate
//! with `tools/gen_icons.mjs`. The matching `embedded_asset!` registrations live in
//! `lib.rs` (so `include_bytes!` resolves relative to `src/`).

/// Declares the icon path constants from a single `NAME => "file-stem"` table.
macro_rules! editor_icons {
    ($($konst:ident => $stem:literal),* $(,)?) => {
        $(
            #[doc = concat!("`", $stem, ".png`")]
            pub const $konst: &str = concat!("embedded://bevy_editor/assets/icons/", $stem, ".png");
        )*
    };
}

editor_icons! {
    // Playback
    PLAY => "play",
    PAUSE => "pause",
    STOP => "stop",
    PLAY_MODE => "play-mode",
    // Gizmo modes
    GIZMO_MOVE => "gizmo-move",
    GIZMO_ROTATE => "gizmo-rotate",
    GIZMO_SCALE => "gizmo-scale",
    // Viewport / view
    CUBE => "cube",
    SQUARE => "square",
    GRID => "grid",
    SNAP => "snap",
    FRAME => "frame",
    EYE => "eye",
    EYE_OFF => "eye-off",
    LOCK => "lock",
    UNLOCK => "unlock",
    // Entity types
    SPHERE => "sphere",
    LIGHT => "light",
    DIR_LIGHT => "dir-light",
    CAMERA => "camera",
    SPRITE => "sprite",
    EMPTY => "empty",
    // Panels / docking
    CHEVRON_DOWN => "chevron-down",
    CHEVRON_RIGHT => "chevron-right",
    FLOAT => "float",
    DOCK => "dock",
    LIST => "list",
    SLIDERS => "sliders",
    FOLDER_TREE => "folder-tree",
    // Actions
    PLUS => "plus",
    X => "x",
    CLOSE => "close",
    DUPLICATE => "duplicate",
    TRASH => "trash",
    SEARCH => "search",
    UNDO => "undo",
    REDO => "redo",
    SAVE => "save",
    FOLDER => "folder",
    FOLDER_OPEN => "folder-open",
    FILE => "file",
    FILE_PLUS => "file-plus",
    IMAGE => "image",
    IMPORT => "import",
    CODE => "code",
    // Flagship
    TERMINAL => "terminal",
    COMMAND => "command",
    SUN => "sun",
    MOON => "moon",
    REMOTE => "remote",
    // Status / misc
    INFO => "info",
    WARNING => "warning",
    ERROR => "error",
    SUCCESS => "success",
    CHECK => "check",
    SETTINGS => "settings",
    BUILD => "build",
    EXPORT => "export",
    MENU => "menu",
}
