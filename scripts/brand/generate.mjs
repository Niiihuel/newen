// Newen's app icon and in-app mark: the initial N in Shippori Mincho ExtraBold (SIL OFL) standing on
// a beni brush horizon that enters from beyond the tile's left edge and ends in a dry tail.
//   node scripts/brand/generate.mjs [out dir]   (default: apps/newen/assets/icons; needs ImageMagick 7
//                                                `magick` with librsvg)
// Writes newen.svg and newen-dark.svg (ink cut), newen-mark.svg (the bare mark in currentColor) and its
// two layers newen-mark-letter.svg and newen-mark-horizon.svg (the welcome screen's),
// newen[-dark]-{16,24,32,48,64,128,256,512}.png (clean cut at <= 48 px, ink cut above) and newen.ico.
// newen-ide's icon is the same with a hanada horizon: newen-ide[-dark].svg, its PNGs and newen-ide.ico,
// written to
// apps/newen-ide/assets/icons (or beside Newen's when an out dir is given).
// The ink and brush code is copied from the website's scripts/brand (newen.mjs, brush.mjs); the changes
// are marked "change:".
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const out = process.argv[2] ?? join(here, '../../apps/newen/assets/icons');
const ideOut = process.argv[2] ?? join(here, '../../apps/newen-ide/assets/icons');
const font = JSON.parse(readFileSync(join(here, 'mincho-extrabold-glyphs.json'), 'utf8'));
const CAP = 740;
const f = (n) => (Math.round(n * 100) / 100).toString();

const SUMI = '#141413';
const WASHI = '#f2ebde';
const BENI = '#c0605c';
const HANADA = '#5c8bc0'; // newen-ide's horizon: a light indigo with beni's lightness and saturation
const NIGHT = '#1a1714';
const RIM = '#2c2722'; // the dark tile's edge, so it does not vanish on a dark desktop
const THEMES = { light: { tile: WASHI, mark: SUMI }, dark: { tile: NIGHT, mark: WASHI, rim: RIM } };
const SIZES = [16, 24, 32, 48, 64, 128, 256, 512];
const SMALL = 48; // the clean cut up to this size, the ink cut above

// ---------- brush (from brush.mjs) ----------
function rng(seed) {
  let a = (seed * 0x9e3779b9) >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// A smooth wobble: a few sines with random phases, roughly in -1..1.
function wobble(r, freqs) {
  const waves = freqs.map((fq) => [fq, r() * Math.PI * 2, 0.5 + r() * 0.5]);
  const norm = waves.reduce((s, w) => s + w[2], 0);
  return (s) => waves.reduce((v, [fq, p, a]) => v + a * Math.sin(fq * s + p), 0) / norm;
}

// Catmull-Rom through the points, resampled to `m` points evenly spaced along the curve.
function centerline(pts, m) {
  const dense = [];
  const P = [pts[0], ...pts, pts[pts.length - 1]];
  for (let i = 1; i < P.length - 2; i++) {
    for (let k = 0; k < 32; k++) {
      const t = k / 32;
      const t2 = t * t;
      const t3 = t2 * t;
      dense.push(
        [0, 1, 2].map((c) => 0.5 * (2 * P[i][c] + (-P[i - 1][c] + P[i + 1][c]) * t + (2 * P[i - 1][c] - 5 * P[i][c] + 4 * P[i + 1][c] - P[i + 2][c]) * t2 + (-P[i - 1][c] + 3 * P[i][c] - 3 * P[i + 1][c] + P[i + 2][c]) * t3)),
      );
    }
  }
  dense.push(pts[pts.length - 1]);
  const len = [0];
  for (let i = 1; i < dense.length; i++) len.push(len[i - 1] + Math.hypot(dense[i][0] - dense[i - 1][0], dense[i][1] - dense[i - 1][1]));
  const total = len[len.length - 1];
  const res = [];
  let j = 0;
  for (let i = 0; i < m; i++) {
    const s = (total * i) / (m - 1);
    while (j < dense.length - 2 && len[j + 1] < s) j++;
    const fr = (s - len[j]) / (len[j + 1] - len[j] || 1);
    res.push([0, 1, 2].map((c) => dense[j][c] + (dense[j + 1][c] - dense[j][c]) * fr));
  }
  return res.map((p, i) => {
    const a = res[Math.max(0, i - 1)];
    const b = res[Math.min(m - 1, i + 1)];
    const d = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1;
    return { x: p[0], y: p[1], w: p[2], tx: (b[0] - a[0]) / d, ty: (b[1] - a[1]) / d, s: (total * i) / (m - 1) };
  });
}

// A shape as compact path data, relative moves, rounded to `digits` decimals (whole units by default).
function encode(ps, digits = 0) {
  const k = 10 ** digits;
  const q = ps.map(([x, y]) => [Math.round(x * k), Math.round(y * k)]).filter((p, i, a) => i === 0 || p[0] !== a[i - 1][0] || p[1] !== a[i - 1][1]);
  const n = (v) => +(v / k).toFixed(digits);
  return `M${n(q[0][0])} ${n(q[0][1])}l` + q.slice(1).map((p, i) => `${n(p[0] - q[i][0])} ${n(p[1] - q[i][1])}`).join(' ').replace(/ -/g, '-') + 'z';
}

// A brush stroke along pts ([x, y, width]): a body whose edges bleed, an optional pressed head and
// a dry tail of bristles. Returns one polygon per piece.
function stroke({ pts, head = 1, dry, spread = 0.4, seed }) {
  const r = rng(seed);
  const M = 120;
  const c = centerline(pts, M);
  const end = dry ? Math.round(dry.at * (M - 1)) : M - 1;
  // change: the body's last JOIN samples lose their bleed and narrow a little, and the bristles start
  // there, so the body no longer ends in a hard vertical step where the bristles take over
  const JOIN = dry ? 12 : 0;
  const paths = [];

  const wl = wobble(r, [0.05, 0.11, 0.23, 0.41]);
  const wr = wobble(r, [0.05, 0.13, 0.21, 0.37]);
  const L = [];
  const R = [];
  for (let i = 0; i <= end; i++) {
    const p = c[i];
    const t = i / (M - 1);
    const fade = JOIN ? Math.min(1, (end - i) / JOIN) : 1;
    const bleed = (0.16 * (1 - t) ** 1.6 + 0.035) * fade;
    const narrow = 0.84 + 0.16 * fade;
    const hl = (p.w / 2) * narrow * (1 + bleed * wl(p.s));
    const hr = (p.w / 2) * narrow * (1 + bleed * wr(p.s));
    L.push([p.x - p.ty * hl, p.y + p.tx * hl]);
    R.push([p.x + p.ty * hr, p.y - p.tx * hr]);
  }
  paths.push([...L, ...R.reverse()]);

  // The head: where the brush pressed down, a round knot with scalloped edges.
  if (head) {
    const p = c[0];
    const rad = (p.w / 2) * 1.12 * head;
    const lobes = wobble(r, [3, 5, 7, 11]);
    const ang = Math.atan2(p.ty, p.tx);
    const cx = p.x + p.tx * rad * 0.35;
    const cy = p.y + p.ty * rad * 0.35;
    const knot = [];
    for (let k = 0; k < 60; k++) {
      const th = (k / 60) * Math.PI * 2;
      const rr = rad * (1 + 0.13 * lobes(th)) * (Math.cos(th) < 0 ? 1.08 : 0.94);
      const x = Math.cos(th) * rr * 1.08;
      const y = Math.sin(th) * rr;
      knot.push([cx + x * Math.cos(ang) - y * Math.sin(ang), cy + x * Math.sin(ang) + y * Math.cos(ang)]);
    }
    paths.push(knot);
  }

  // The dry tail: bristles that start shoulder to shoulder, part, and stop one by one.
  if (dry) {
    const ws = c[end].w;
    const n = dry.n;
    const span = M - 1 - end;
    const u0 = -JOIN / span;
    for (let b = 0; b < n; b++) {
      const lat = -0.5 + (b + 0.5) / n + (r() - 0.5) * (0.4 / n);
      const outer = Math.abs(lat) * 2;
      const stop = Math.min(1, Math.max(0.3, 1 - r() * (0.25 + outer * 0.5)));
      const thick = (ws / n) * (0.75 + r() * 0.6);
      const drift = wobble(r, [0.04, 0.09]);
      // a dry skip: the bristle lifts off the paper for a moment
      const gap = r() < 0.4 ? [0.2 + r() * (stop - 0.35), 0.04 + r() * 0.08] : null;
      const pieces = gap ? [[u0, gap[0]], [gap[0] + gap[1], stop]] : [[u0, stop]];
      for (const [a0, a1] of pieces) {
        if (a1 - a0 < 0.03) continue;
        const BL = [];
        const BR = [];
        const steps = Math.max(6, Math.round((a1 - a0) * span * 0.9));
        for (let k = 0; k <= steps; k++) {
          const u = a0 + ((a1 - a0) * k) / steps;
          const i = Math.min(M - 1, Math.max(0, Math.round(end + u * span)));
          const p = c[i];
          const env = Math.max(p.w, ws * 0.35) * (1 + spread * Math.max(0, u));
          const off = lat * env + drift(p.s) * thick * 0.5 * Math.max(0, u);
          // pointed at the bristle's own end, and at both ends of a skip
          const toEnd = Math.min(1, (a1 - u) / 0.12, a0 > 0 ? (u - a0) / 0.04 : 1);
          const h = (thick / 2) * Math.max(0, toEnd) ** 0.7 * (1 - 0.35 * Math.max(0, u));
          const x = p.x - p.ty * off;
          const y = p.y + p.tx * off;
          BL.push([x - p.ty * h, y + p.tx * h]);
          BR.push([x + p.ty * h, y - p.tx * h]);
        }
        paths.push([...BL, ...BR.reverse()]);
      }
    }
  }
  return paths;
}

// ---------- outlines → contours → ink (from newen.mjs) ----------
function contours(d) {
  const tok = d.match(/[MLHVQZ]|-?\d*\.?\d+(?:e-?\d+)?/gi);
  const all = [];
  let cur = null;
  let p = [0, 0];
  let i = 0;
  const num = () => +tok[i++];
  while (i < tok.length) {
    const c = tok[i++];
    if (c === 'M') (cur = [(p = [num(), num()])]), all.push(cur);
    else if (c === 'L') cur.push((p = [num(), num()]));
    else if (c === 'H') cur.push((p = [num(), p[1]]));
    else if (c === 'V') cur.push((p = [p[0], num()]));
    else if (c === 'Q') {
      const [cx, cy, x, y] = [num(), num(), num(), num()];
      for (let k = 1; k <= 8; k++) {
        const t = k / 8;
        const u = 1 - t;
        cur.push([u * u * p[0] + 2 * u * t * cx + t * t * x, u * u * p[1] + 2 * u * t * cy + t * t * y]);
      }
      p = [x, y];
    } else if (c === 'Z') p = cur[0];
    else throw new Error(`path command ${c}`);
  }
  return all;
}

function resample(poly, step) {
  const pts = [...poly, poly[0]];
  const res = [];
  let carry = 0;
  for (let i = 0; i < pts.length - 1; i++) {
    const [a, b] = [pts[i], pts[i + 1]];
    const len = Math.hypot(b[0] - a[0], b[1] - a[1]);
    for (let s = carry; s < len; s += step) res.push([a[0] + ((b[0] - a[0]) * s) / len, a[1] + ((b[1] - a[1]) * s) / len]);
    carry = (((carry - len) % step) + step) % step;
  }
  return res;
}

// Ink pressed into washi: every edge moves outwards (the bleed), wanders in long scallops and fine
// grain, and is then softened. change: `amount` scales the whole effect; the icon shows its N far
// larger than the hero shows a letter, and full bleed swells the serif ends into bulbs there.
function inked(ch, seed, amount) {
  const r = rng(seed);
  return contours(font.glyphs[ch].d).map((poly) => {
    const pts = resample(poly, 5);
    const n = pts.length;
    const area = pts.reduce((a, p, i) => a + p[0] * pts[(i + 1) % n][1] - pts[(i + 1) % n][0] * p[1], 0);
    const side = area > 0 ? 1 : -1;
    const scallop = wobble(r, [0.025, 0.05, 0.09]);
    const grain = wobble(r, [0.2, 0.33, 0.55]);
    const moved = pts.map((p, i) => {
      const [a, b] = [pts[(i - 1 + n) % n], pts[(i + 1) % n]];
      const len = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1;
      const [nx, ny] = [((b[1] - a[1]) / len) * side, (-(b[0] - a[0]) / len) * side];
      const s = i * 5;
      const d = amount * (7 + 5 * scallop(s) + 1.8 * grain(s) + (r() - 0.5) * 1.4);
      return [p[0] + nx * d, p[1] + ny * d];
    });
    return moved.map((_, i) => {
      let [x, y] = [0, 0];
      for (let k = -2; k <= 2; k++) (x += moved[(i + k + n) % n][0]), (y += moved[(i + k + n) % n][1]);
      return [x / 5, y / 5];
    });
  });
}

// The N with its box centred on x = 0 and its baseline on y = 0 (y down), scaled by k.
const box = font.glyphs.N.bounds;
const MID = (box[0] + box[2]) / 2;
function letterN(k, cut) {
  const t = `transform="scale(${+k.toFixed(6)} ${-k.toFixed(6)}) translate(${f(-MID)} 0)"`;
  if (cut === 'clean') return `<path ${t} d="${font.glyphs.N.d}"/>`;
  return `<path ${t} d="${inked('N', 3, L.bleed).map((c) => encode(c)).join('')}"/>`;
}

// ---------- the tile ----------
const svg = (body) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">${body}</svg>\n`;
// The horizon is cut by the tile's own outline, as the hero's is cut by the page's edge.
const CLIP = '<defs><clipPath id="newen-tile"><rect width="1024" height="1024" rx="230"/></clipPath></defs>';
// The rim is a stroke inside the tile's outline, `w` units wide.
function tile(theme, w) {
  const t = THEMES[theme];
  if (!t.rim) return `<rect width="1024" height="1024" rx="230" fill="${t.tile}"/>`;
  return `<rect x="${f(w / 2)}" y="${f(w / 2)}" width="${f(1024 - w)}" height="${f(1024 - w)}" rx="${f(230 - w / 2)}" fill="${t.tile}" stroke="${t.rim}" stroke-width="${f(w)}"/>`;
}

// ---------- the ink cut (> 48 px) ----------
// The N (cap height `cap`, baseline `base`, centred) stands on the crest of a circle of radius R: the
// arc's top edge rises `crest` units above the baseline at x = 512, so the feet stand on it and the
// rise shows in the N's open foot. The stroke runs from beyond the left edge (x0) to a dry end at x1.
const L = { cap: 470, base: 716, R: 1600, crest: 10, x0: -40, x1: 900, ink: 50, bleed: 0.7, dryAt: 0.72, rim: 16, seed: 3 };
const width = (i) => L.ink * (1.1 - 0.02 * i) * (0.94 + 0.12 * Math.sin(i * 1.3) ** 2); // pressed at the edge, thinning
const yAt = (x) => L.base - L.crest + L.ink / 2 + L.R - Math.sqrt(L.R * L.R - (x - 512) ** 2);
const ARC = (() => {
  const pts = Array.from({ length: 11 }, (_, i) => {
    const x = L.x0 + ((L.x1 - L.x0) * i) / 10;
    return [x, yAt(x), width(i)];
  });
  return stroke({ pts, head: 0, dry: { at: L.dryAt, n: 7 }, spread: 0.55, seed: L.seed })
    .map((p) => `<path d="${encode(p, 1)}"/>`)
    .join('');
})();

// Self-check: both feet of the N (the flat bottoms of its outline, y = 0) touch the arc's top edge,
// none floats more than 6 units above it and none sinks more than 24 into it.
{
  const k = L.cap / CAP;
  const feet = [[37, 267], [582, 659]]; // the outline's two flat bottoms, from "M37 0" to "267 0Z" and "V0H582"
  if (!font.glyphs.N.d.startsWith('M37 0') || !font.glyphs.N.d.includes('267 0Z') || !font.glyphs.N.d.includes('659 624V0H582')) throw new Error("the N's outline changed: update its feet");
  for (const [a, b] of feet) {
    for (let fx = a; fx <= b; fx += 4) {
      const x = 512 + (fx - MID) * k;
      const i = ((x - L.x0) / (L.x1 - L.x0)) * 10;
      const gap = yAt(x) - width(i) / 2 - L.base; // > 0: the foot floats over the arc
      if (gap > 6 || gap < -24) throw new Error(`the N's foot at x=${f(x)} is ${f(gap)} units off the arc`);
    }
  }
}

function large(theme, horizon = BENI) {
  const t = THEMES[theme];
  const n = `<g fill="${t.mark}" transform="translate(512 ${L.base})">${letterN(L.cap / CAP, 'ink')}</g>`;
  return svg(`${CLIP}${tile(theme, L.rim)}<g fill="${horizon}" clip-path="url(#newen-tile)">${ARC}</g>${n}`);
}

// The bare mark for the UI: the same N and horizon in currentColor, no tile (limn tints it).
const mark = () => svg(`${CLIP}<g fill="currentColor"><g clip-path="url(#newen-tile)">${ARC}</g><g transform="translate(512 ${L.base})">${letterN(L.cap / CAP, 'ink')}</g></g>`);
// Its two layers in the same frame, for the welcome screen, which tints the horizon beni and draws it
// left to right under the N.
const letter = () => svg(`<g fill="currentColor" transform="translate(512 ${L.base})">${letterN(L.cap / CAP, 'ink')}</g>`);
const horizon = () => svg(`${CLIP}<g fill="currentColor" clip-path="url(#newen-tile)">${ARC}</g>`);

// ---------- the clean cut (<= 48 px) ----------
// The font's own N, a little larger, with a `bold` stroke in its colour so the hairlines hold (an
// optical small size; the shapes stay the font's). The horizon is a level band from beyond the left
// edge to a round end, narrowing to the right as the brush does but never leaving its own row, so at
// 16 px it is one crisp row. At each size the N's top and bottom and the band's edges are snapped
// to whole pixels, with at least one clear pixel between the N and the band.
const S = { cap: 498, base: 697, bold: 14, y: 800, w: 64, x0: -20, x1: 800, taper: 520, w1: 40 };
function small(theme, px, horizon = BENI) {
  const t = THEMES[theme];
  const u = 1024 / px;
  const snap = (y) => Math.round(y / u) * u;
  const top = snap(S.base - S.cap - S.bold / 2);
  const bottom = snap(S.base + S.bold / 2);
  const base = bottom - S.bold / 2;
  const k = (bottom - top - S.bold) / CAP;
  const a = snap(S.y - S.w / 2);
  const b = snap(S.y + S.w / 2);
  if (a - bottom < u || b - a < u) throw new Error(`${px} px: the band needs a clear pixel above it and one row of its own`);
  const y = (a + b) / 2;
  const h0 = (b - a) / 2;
  const h1 = Math.min(h0, S.w1 / 2);
  const end = S.x1 - h1; // centre of the round end
  const half = (x) => (x <= S.taper ? h0 : h0 + ((h1 - h0) * (x - S.taper)) / (end - S.taper));
  const xs = Array.from({ length: 41 }, (_, i) => S.x0 + ((end - S.x0) * i) / 40);
  const tip = Array.from({ length: 11 }, (_, j) => {
    const th = -Math.PI / 2 + (Math.PI * (j + 1)) / 12;
    return [end + h1 * Math.cos(th), y + h1 * Math.sin(th)];
  });
  const band = [...xs.map((x) => [x, y - half(x)]), ...tip, ...xs.reverse().map((x) => [x, y + half(x)])];
  const n = `<g fill="${t.mark}" stroke="${t.mark}" stroke-width="${f(S.bold / k)}" stroke-linejoin="round" transform="translate(512 ${f(base)})">${letterN(k, 'clean')}</g>`;
  return svg(`${CLIP}${tile(theme, u)}<path fill="${horizon}" clip-path="url(#newen-tile)" d="${encode(band, 2)}"/>${n}`);
}

// ---------- write and render ----------
const tmp = mkdtempSync(join(tmpdir(), 'newen-icon-'));
const magick = (args) => execFileSync('magick', args, { encoding: 'utf8' });
function png(source, px, file) {
  const src = join(tmp, `${px}.svg`);
  writeFileSync(src, source);
  magick(['-background', 'none', '-density', String((96 * px) / 1024), src, '-resize', `${px}x${px}!`, '-strip', `PNG32:${file}`]);
  const size = magick(['identify', '-format', '%wx%h', file]);
  if (size !== `${px}x${px}`) throw new Error(`${file} is ${size}, not ${px}x${px}`);
}
try {
  writeFileSync(join(out, 'newen.svg'), large('light'));
  writeFileSync(join(out, 'newen-dark.svg'), large('dark'));
  writeFileSync(join(out, 'newen-mark.svg'), mark());
  writeFileSync(join(out, 'newen-mark-letter.svg'), letter());
  writeFileSync(join(out, 'newen-mark-horizon.svg'), horizon());
  for (const theme of ['light', 'dark']) {
    const name = theme === 'light' ? 'newen' : 'newen-dark';
    for (const px of SIZES) png(px <= SMALL ? small(theme, px) : large(theme), px, join(out, `${name}-${px}.png`));
  }
  const layers = [16, 24, 32, 48, 64, 128, 256];
  magick([...layers.map((px) => join(out, `newen-${px}.png`)), join(out, 'newen.ico')]);
  const found = magick(['identify', '-format', '%w ', join(out, 'newen.ico')]).trim();
  if (found !== layers.join(' ')) throw new Error(`newen.ico has layers ${found}`);
  // newen-ide: the same icon on a hanada horizon.
  writeFileSync(join(ideOut, 'newen-ide.svg'), large('light', HANADA));
  writeFileSync(join(ideOut, 'newen-ide-dark.svg'), large('dark', HANADA));
  for (const theme of ['light', 'dark']) {
    const name = theme === 'light' ? 'newen-ide' : 'newen-ide-dark';
    for (const px of SIZES) {
      png(px <= SMALL ? small(theme, px, HANADA) : large(theme, HANADA), px, join(ideOut, `${name}-${px}.png`));
    }
  }
  magick([...layers.map((px) => join(ideOut, `newen-ide-${px}.png`)), join(ideOut, 'newen-ide.ico')]);
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
console.log(`wrote newen.svg, newen-dark.svg, newen-mark.svg and its two layers, ${SIZES.length * 2} PNGs and newen.ico in ${out}`);
console.log(`wrote newen-ide.svg, newen-ide-dark.svg, ${SIZES.length * 2} PNGs and newen-ide.ico in ${ideOut}`);
