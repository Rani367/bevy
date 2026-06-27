// Regenerate the editor icon set from Lucide (ISC License).
//
//   cd crates/bevy_editor/tools
//   npm install @resvg/resvg-js
//   node gen_icons.mjs
//
// Downloads each Lucide source SVG, recolors its `currentColor` stroke to white
// (so `ThemedIcon` can tint it), and rasterizes it to a 48x48 transparent PNG in
// ../src/assets/icons/. Keep the `editor name -> lucide name` map below in sync
// with the constants in src/ui/icons.rs.

import { Resvg } from '@resvg/resvg-js';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ICONS = {
  // playback
  play: 'play', pause: 'pause', stop: 'square', 'play-mode': 'circle-play',
  // gizmo
  'gizmo-move': 'move-3d', 'gizmo-rotate': 'rotate-3d', 'gizmo-scale': 'scale-3d',
  // viewport / view
  cube: 'box', square: 'square', grid: 'grid-3x3', snap: 'magnet', frame: 'focus',
  eye: 'eye', 'eye-off': 'eye-off', lock: 'lock', unlock: 'lock-open',
  // entity types
  sphere: 'circle', light: 'lightbulb', 'dir-light': 'sun', camera: 'video',
  sprite: 'image', empty: 'box-select',
  // panel / dock
  'chevron-down': 'chevron-down', 'chevron-right': 'chevron-right',
  float: 'external-link', dock: 'pin', list: 'list-tree', sliders: 'sliders-horizontal',
  'folder-tree': 'folder-tree',
  // actions
  plus: 'plus', x: 'x', close: 'x', duplicate: 'copy', trash: 'trash-2', search: 'search',
  undo: 'undo-2', redo: 'redo-2', save: 'save', folder: 'folder', 'folder-open': 'folder-open',
  file: 'file', 'file-plus': 'file-plus', image: 'image', import: 'download', code: 'code',
  // flagship
  terminal: 'terminal', command: 'command', sun: 'sun', moon: 'moon', remote: 'satellite-dish',
  // status / misc
  info: 'info', warning: 'triangle-alert', error: 'circle-alert', success: 'circle-check',
  check: 'check', settings: 'settings', build: 'hammer', export: 'package', menu: 'menu',
};

const here = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.join(here, '..', 'src', 'assets', 'icons');
fs.mkdirSync(outDir, { recursive: true });

const base = 'https://cdn.jsdelivr.net/npm/lucide-static@latest/icons/';
const fails = [];
let n = 0;
for (const [dest, src] of Object.entries(ICONS)) {
  try {
    const res = await fetch(base + src + '.svg');
    if (!res.ok) { fails.push(`${dest}<-${src} HTTP ${res.status}`); continue; }
    const svg = (await res.text()).replace(/currentColor/g, '#ffffff');
    const png = new Resvg(svg, { fitTo: { mode: 'width', value: 48 }, background: 'rgba(0,0,0,0)' })
      .render().asPng();
    fs.writeFileSync(path.join(outDir, dest + '.png'), png);
    n++;
  } catch (e) { fails.push(`${dest}<-${src} ${e.message}`); }
}
console.log(`rendered ${n} icons to ${outDir}`);
if (fails.length) { console.log('FAILURES:\n' + fails.join('\n')); process.exit(1); }
