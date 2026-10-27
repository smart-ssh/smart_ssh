#!/usr/bin/env node
// Issue #980: regenerates the Android launcher icons from the desktop logo.
//
//   cd apps/smart-ssh-community/frontend && npm ci   # once (provides playwright)
//   node scripts/generate-android-icons.mjs          # from the repository root
//
// Inputs (apps/smart-ssh-community): `icons/icon.png` (full logo, used for the
// legacy icons and as the source of the prompt mark) and `icon-source.png`
// (flat background colour of the adaptive icon). Output: `icons/android/`
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

const b64 = (f) => readFileSync(join(app, f)).toString("base64");
const browser = await chromium.launch();
const page = await browser.newPage();
const out = await page.evaluate(
  async ({ logo, src, densities, safe }) => {
    const load = (b) =>
      new Promise((res, rej) => {
        const i = new Image();
        i.onload = () => res(i);
        i.onerror = rej;
        i.src = "data:image/png;base64," + b;
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
    const logoImg = await load(logo);
    const srcImg = await load(src);
    const N = logoImg.naturalWidth;
    // Background colour: centre pixel of icon-source.png.
    const [, sx] = mk(1);
    sx.drawImage(srcImg, srcImg.naturalWidth / 2, srcImg.naturalHeight / 2, 1, 1, 0, 0, 1, 1);
    const [r, g, b] = sx.getImageData(0, 0, 1, 1).data;
    const hex = "#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");
    // Prompt mark: keep the light pixels of the logo, drop the dark gradient.
    const [lc, lx] = mk(N);
    lx.drawImage(logoImg, 0, 0);
    const ld = lx.getImageData(0, 0, N, N);
    const [mc, mx] = mk(N);
    const md = mx.createImageData(N, N);
    let x0 = N, y0 = N, x1 = 0, y1 = 0;
    for (let i = 0; i < N * N; i++) {
      const p = i * 4;
      const a = Math.max(0, Math.min(1, (ld.data[p + 2] - 55) / (245 - 55)));
      if (ld.data[p + 3] === 0 || a < 0.02) continue;
      // two mark colours: plus sign (light) and chevron/underscore
      const light = ld.data[p] > 140;
      md.data[p] = light ? 165 : 108;
      md.data[p + 1] = light ? 176 : 124;
      md.data[p + 2] = light ? 255 : 245;
      md.data[p + 3] = Math.round(a * 255);
      if (a > 0.5) {
        const px = i % N, py = (i / N) | 0;
        x0 = Math.min(x0, px); x1 = Math.max(x1, px);
        y0 = Math.min(y0, py); y1 = Math.max(y1, py);
      }
    }
    mx.putImageData(md, 0, 0);
    const cx = (x0 + x1) / 2, cy = (y0 + y1) / 2;
    let R = 0;
    for (let py = y0; py <= y1; py++)
      for (let px = x0; px <= x1; px++)
        if (md.data[(py * N + px) * 4 + 3] > 128)
          R = Math.max(R, Math.hypot(px - cx, py - cy));
    const files = {};
    const png = (c) => c.toDataURL("image/png").split(",")[1];
    for (const [d, f] of Object.entries(densities)) {
      const fg = Math.round(108 * f), lg = Math.round(48 * f);
      // adaptive foreground: mark only, transparent
      const [fc, fx] = mk(fg);
      const s = ((safe / 108) * fg) / R;
      fx.drawImage(mc, fg / 2 - cx * s, fg / 2 - cy * s, N * s, N * s);
      files[`mipmap-${d}/ic_launcher_foreground.png`] = png(fc);
      // legacy: full logo
      const [c1, x1c] = mk(lg);
      x1c.drawImage(logoImg, 0, 0, lg, lg);
      files[`mipmap-${d}/ic_launcher.png`] = png(c1);
      // legacy round: logo clipped to a circle
      const [c2, x2] = mk(lg);
      x2.beginPath();
      x2.arc(lg / 2, lg / 2, lg / 2, 0, Math.PI * 2);
      x2.clip();
      x2.fillStyle = hex;
      x2.fillRect(0, 0, lg, lg);
      const k = lg * 1.15;
      x2.drawImage(logoImg, (lg - k) / 2, (lg - k) / 2, k, k);
      files[`mipmap-${d}/ic_launcher_round.png`] = png(c2);
    }
    return { hex, files };
  },
  { logo: b64("icons/icon.png"), src: b64("icon-source.png"), densities: DENSITIES, safe: SAFE_RADIUS_DP },
);
await browser.close();

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
