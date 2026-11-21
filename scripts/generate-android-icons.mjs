#!/usr/bin/env node
// Issue #980 / #1364: regenerates the Android launcher icons from the "Trace"
// logo SVGs.
//
//   cd apps/smart-ssh-community/frontend && npm ci   # once (provides playwright)
//   node scripts/generate-android-icons.mjs          # from the repository root
//
// Inputs (apps/smart-ssh-community): `icons/logo.svg` (dark tile: legacy icons,
// and its fill is the adaptive background colour) and `icons/logo-mark.svg`
// (glyph only, adaptive foreground). Output: `icons/android/`
// (the single source of truth) and a byte-identical copy in
// `gen/android/app/src/main/res/`. Rendering uses headless Chromium (canvas),
// so no extra image tooling is needed. Spec 0114.
import { createRequire } from "node:module";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const app = join(root, "apps/smart-ssh-community");
const require = createRequire(join(app, "frontend/package.json"));
const { chromium } = require("playwright");

const DENSITIES = { mdpi: 1, hdpi: 1.5, xhdpi: 2, xxhdpi: 3, xxxhdpi: 4 };
// Adaptive icon: 108 dp canvas, 66 dp safe zone (a circle). The mark's
// farthest pixel stays at SAFE_RADIUS dp from the centre.
const SAFE_RADIUS_DP = 31;

const read = (f) => readFileSync(join(app, f), "utf8");
const tileSvg = read("icons/logo.svg");
const markSvg = read("icons/logo-mark.svg");
// Background colour of the adaptive icon: the tile fill of icons/logo.svg.
const hex = tileSvg.match(/<rect[^>]*fill="(#[0-9a-fA-F]{6})"/)?.[1]?.toLowerCase();
if (!hex) throw new Error("icons/logo.svg has no <rect fill=\"#rrggbb\"> tile");
// Farthest point of the mark (outer corner of an accent square) from the
// 64-unit centre: hypot(22, 24).
const MARK_RADIUS_U = Math.hypot(22, 24);
const dataUrl = (svg) => "data:image/svg+xml;base64," + Buffer.from(svg).toString("base64");
const browser = await chromium.launch();
const page = await browser.newPage();
const out = await page.evaluate(
  async ({ tile, mark, hex, densities, safe, R }) => {
    const load = (u) =>
      new Promise((res, rej) => {
        const i = new Image();
        i.onload = () => res(i);
        i.onerror = rej;
        i.src = u;
      });
    const mk = (w, h = w) => {
      const c = document.createElement("canvas");
      c.width = w;
      c.height = h;
      const x = c.getContext("2d");
      x.imageSmoothingEnabled = true;
      x.imageSmoothingQuality = "high";
      return [c, x];
    };
    const tileImg = await load(tile);
    const markImg = await load(mark);
    const files = {};
    const png = (c) => c.toDataURL("image/png").split(",")[1];
    for (const [d, f] of Object.entries(densities)) {
      const fg = Math.round(108 * f), lg = Math.round(48 * f);
      // adaptive foreground: mark only (stroke 9 u), transparent, scaled so
      // its farthest point sits on the safe-zone circle (66 dp of 108 dp)
      const [fc, fx] = mk(fg);
      const k0 = ((safe / 108) * fg) / R; // px per 64-unit grid unit
      const side = 64 * k0;
      fx.drawImage(markImg, (fg - side) / 2, (fg - side) / 2, side, side);
      files[`mipmap-${d}/ic_launcher_foreground.png`] = png(fc);
      // legacy: full tile
      const [c1, x1c] = mk(lg);
      x1c.drawImage(tileImg, 0, 0, lg, lg);
      files[`mipmap-${d}/ic_launcher.png`] = png(c1);
      // legacy round: tile clipped to a circle
      const [c2, x2] = mk(lg);
      x2.beginPath();
      x2.arc(lg / 2, lg / 2, lg / 2, 0, Math.PI * 2);
      x2.clip();
      x2.fillStyle = hex;
      x2.fillRect(0, 0, lg, lg);
      const k = lg * 1.15;
      x2.drawImage(tileImg, (lg - k) / 2, (lg - k) / 2, k, k);
      files[`mipmap-${d}/ic_launcher_round.png`] = png(c2);
    }
    return { files };
  },
  { tile: dataUrl(tileSvg), mark: dataUrl(markSvg), hex, densities: DENSITIES, safe: SAFE_RADIUS_DP, R: MARK_RADIUS_U },
);
await browser.close();
out.hex = hex;

const adaptive = `<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
  <background android:drawable="@color/ic_launcher_background"/>
  <foreground android:drawable="@mipmap/ic_launcher_foreground"/>
</adaptive-icon>
`;
const bg = `<?xml version="1.0" encoding="utf-8"?>
<resources>
  <color name="ic_launcher_background">${out.hex}</color>
</resources>
`;
const text = {
  "mipmap-anydpi-v26/ic_launcher.xml": adaptive,
  "mipmap-anydpi-v26/ic_launcher_round.xml": adaptive,
  "values/ic_launcher_background.xml": bg,
};
for (const base of [
  join(app, "icons/android"),
  join(app, "gen/android/app/src/main/res"),
]) {
  for (const [name, data] of Object.entries(out.files)) {
    const p = join(base, name);
    mkdirSync(dirname(p), { recursive: true });
    writeFileSync(p, Buffer.from(data, "base64"));
  }
  for (const [name, data] of Object.entries(text)) {
    const p = join(base, name);
    mkdirSync(dirname(p), { recursive: true });
    writeFileSync(p, data);
  }
}
console.log(`background ${out.hex}; wrote ${Object.keys(out.files).length} PNGs`);
