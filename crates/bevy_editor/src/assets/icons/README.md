# Editor icons

These monochrome PNG icons are rasterized from [Lucide](https://lucide.dev)
(`lucide-static`, **ISC License**) at 48×48 with a white stroke on a transparent
background, so they tint to the active theme text color via `bevy_feathers`'
`ThemedIcon`.

They are embedded by `EditorPlugin` (`embedded_asset!`) and referenced through the
`embedded://bevy_editor/assets/icons/<name>.png` constants in `src/ui/icons.rs`.

To regenerate, see `tools/gen_icons.mjs` (downloads the Lucide source SVGs and
renders them with `@resvg/resvg-js`). The `name -> lucide source` mapping lives there.

Lucide is distributed under the ISC license; see https://lucide.dev/license.
