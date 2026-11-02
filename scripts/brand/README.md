# Newen's icon and mark

`generate.mjs` draws Newen's app icon and in-app mark and writes them to `apps/newen/assets/icons/`.

The initial N in Shippori Mincho ExtraBold stands on a beni brush horizon. The horizon enters from beyond the tile's left edge, is cut by the tile's rounded corner and ends in a dry-brush tail before the right edge. The N's feet stand on the arc's crest, and the rise shows inside the N's open foot. The tile is 1024 with `rx=230`. Sumi N on a washi tile (light); washi N on a sumi tile (dark). The arc is beni in both, and it is the only red.

Two cuts:

- **Ink cut** (above 48 px, and the SVGs): the N's outline bleeds into the paper with scalloped, grainy edges. The arc is a brush stroke: heavier where it enters, then a dry tail of seven bristles.
- **Clean cut** (48 px and below): the font's own N with a 14-unit stroke in its own colour, so the hairlines hold at 16 px. The horizon is a level band from the left edge to a round end, narrowing to the right. At every size the N's top and bottom and the band's edges are snapped to whole pixels, with at least one clear pixel between them. At 16 px the band is exactly one crisp row.

The dark tile has a slightly lighter rim (`#2c2722`) so it stays visible on a dark desktop.

Palette: sumi `#141413`, washi `#f2ebde`, beni `#c0605c`, dark tile `#1a1714`, rim `#2c2722`.

## Files

| File in `apps/newen/assets/icons/` | What it is | Used by |
|---|---|---|
| `newen.svg` | Light icon, ink cut, viewBox 1024 | `scripts/install-desktop.sh` and the migration from quill, `crates/newen-engine/src/migrate.rs` (hicolor `scalable/apps`) |
| `newen-dark.svg` | Dark icon, ink cut | nothing in code; kept beside the light one |
| `newen-{16,24,32,48,64,128,256,512}.png` | Light icon PNGs, RGBA. Clean cut at 16–48, ink cut at 64–512 | `scripts/install-desktop.sh` (hicolor PNGs), `scripts/package.sh` (the macOS iconset: 16, 32, 128, 256, 512 and their @2x), `apps/newen/src/main.rs` (`newen-256.png`, the window icon), the migration from quill (`newen-256.png`, hicolor `256x256/apps`) |
| `newen-dark-{same sizes}.png` | Dark icon PNGs | `apps/newen/src/main.rs` (`newen-dark-256.png`, the window icon in the dark theme) |
| `newen.ico` | Windows icon, light, with layers 16, 24, 32, 48, 64, 128, 256 | `apps/newen/build.rs` (`winresource` `set_icon`) |
| `newen-mark.svg` | The bare mark (N and horizon) without the tile, all in `currentColor`, viewBox 1024 in the tile's own frame | `crates/newen-ui/src/account.rs` and `appearance.rs` (`icons::of(include_str!(...))`, tinted with the text's colour, 56 px) |
| `newen-mark-letter.svg`, `newen-mark-horizon.svg` | The mark's two layers, the N and the horizon, each in `currentColor` in the same frame | `crates/newen-ui/src/welcome.rs`: the N in the text's colour, the horizon in beni, drawn left to right under it |

limn swaps `currentColor` for white, rasterises the SVG with resvg into a coverage mask, contains the viewBox in the icon's square and tints the mask. So the marks have no fixed colours. A limn icon is at most 128 px, which is the welcome's size. The horizon is cut by the same rounded-tile clip as on the icon, so its left end matches the icon.

## Regenerating

```sh
node scripts/brand/generate.mjs            # writes every file above into apps/newen/assets/icons
node scripts/brand/generate.mjs <out dir>  # or into another folder
```

It needs Node (no npm packages) and ImageMagick 7 (`magick`) with its librsvg SVG delegate. It reads `mincho-extrabold-glyphs.json` from beside itself. The output is deterministic: the ink and the brush use seeded random numbers, and the PNGs are written with `-strip`. A run with no change to the script leaves `git status` clean.

The layout is set by the constants at the top of each section of `generate.mjs`:

- `L`: the ink cut's N size and baseline, the arc's radius, crest, ink weight, bleed, rim and seed.
- `S`: the clean cut's N, bold stroke and band.

The script stops with an error if any of these checks fail:

- Both of the N's flat feet stand on the arc's top edge: none floats more than 6 units above it or sinks more than 24 into it.
- At every clean-cut size, the N and the band have at least one clear pixel between them and the band has a full row of its own.
- Every PNG has the requested size.
- `newen.ico` has exactly the layers 16, 24, 32, 48, 64, 128 and 256.

There is no `.icns` here: `scripts/package.sh` builds it on macOS with `iconutil` from the PNGs. `newen.ico` is the light icon only.

## Font licence

The letter is drawn from the outlines of **Shippori Mincho ExtraBold**, Copyright 2021 The Shippori Mincho Project Authors (https://github.com/fontdasu/ShipporiMincho), licensed under the **SIL Open Font License 1.1** (`OFL-ShipporiMincho.txt`, next to this file). The icon is artwork made with the font's outlines and is not a font. The OFL allows that use. Its conditions (the Reserved Font Name, and no sale of the font by itself) apply to the font software and its derivatives, not to artwork made with it. The licence text travels with `mincho-extrabold-glyphs.json`, which holds outlines taken from the font.

Everything else (the ink and brush treatment, the composition, the colours) is original work for Newen. It takes nothing from Anthropic or any other vendor, and it uses no Mapuche sacred symbols.
