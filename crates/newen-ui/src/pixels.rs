//! The Ultracode pixel field: lilac cells that light up and drift along the effort slider's
//! track while it rests on its top level.
//!
//! Ported from claude-model-selector (github.com/zanwei/claude-model-selector,
//! `JavaScript/claude-model-selector.js`, `_drawPixelField`), under its license:
//!
//! Copyright (c) 2026 Zanwei Guo
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy of this
//! software and associated documentation files (the "Software"), to deal in the Software
//! without restriction, including without limitation the rights to use, copy, modify, merge,
//! publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons
//! to whom the Software is furnished to do so, subject to the following conditions: The above
//! copyright notice and this permission notice shall be included in all copies or substantial
//! portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
//! EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
//! FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
//! HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
//! CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
//! USE OR OTHER DEALINGS IN THE SOFTWARE.

use limn::Color;

/// A cell's side and the space left around its square, in logical pixels.
pub const CELL: f32 = 5.0;
const GAP: f32 = 1.1;

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

fn mix3(from: [f32; 3], to: [f32; 3], amount: f32) -> [f32; 3] {
    [
        mix(from[0], to[0], amount),
        mix(from[1], to[1], amount),
        mix(from[2], to[2], amount),
    ]
}

/// The fractional part of `|sin(x)| * scale`, the reference's cheap hash.
fn hash(x: f32, scale: f32) -> f32 {
    (x.sin() * scale).abs().fract()
}

/// Each cell of a `width` × `height` field, row by row, `elapsed` milliseconds after the
/// slider reached its top level: its color, or `None` where nothing shows. `reveal` runs
/// from 0 to 1 as the field sweeps in from the right. `left` is the color the field starts
/// from at the track's left end: the track's own.
pub fn field(
    width: f32,
    height: f32,
    elapsed: f32,
    reveal: f32,
    left: [f32; 3],
) -> (usize, Vec<Option<Color>>) {
    let frontier = 1.0 - reveal;
    let columns = (width / CELL).ceil() as usize;
    let rows = (height / CELL).ceil() as usize;
    let deep_violet = [156.0, 120.0, 192.0];
    let deep_mid = [156.0, 132.0, 192.0];
    let mid_purple = [168.0, 144.0, 204.0];
    let soft_mid = [168.0, 156.0, 204.0];
    let soft_lilac = [180.0, 168.0, 204.0];
    let pale_cool = [192.0, 180.0, 204.0];
    let highlight = [216.0, 204.0, 228.0];
    let peak = [232.0, 224.0, 242.0];
    let tones = [
        deep_violet,
        deep_violet,
        deep_mid,
        deep_mid,
        mid_purple,
        mid_purple,
        mid_purple,
        soft_mid,
        soft_mid,
        soft_lilac,
        pale_cool,
    ];
    let raw_flow = elapsed / 4000.0;
    let eased_flow = raw_flow.floor() + smoothstep(0.0, 1.0, raw_flow.fract());
    let mut cells = Vec::with_capacity(columns * rows);
    for row in 0..rows {
        for column in 0..columns {
            let (c, r) = (column as f32, row as f32);
            let x = (c * CELL + CELL * 0.5) / width;
            let reveal_alpha = smoothstep(frontier - 0.1, frontier + 0.07, x);
            if reveal_alpha <= 0.002 {
                cells.push(None);
                continue;
            }
            let purple = smoothstep(0.1, 0.88, x);
            let intensity = smoothstep(0.04, 0.38, x);
            let depth = smoothstep(0.35, 0.95, x);
            let base = hash(c * 12.9898 + r * 78.233, 43758.545);
            let tempo = hash(c * 7.13 + r * 19.41, 19341.73);
            let phase = hash(c * 31.17 + r * 11.93, 28437.123);
            let chroma = hash(c * 9.47 + r * 67.13, 15823.917);
            let period = 500.0 + tempo * 1500.0;
            let local = elapsed + phase * period;
            let cycle = (local / period).floor();
            let progress = (local % period) / period;
            let cycle_hash = hash(c * 17.17 + r * 41.73 + cycle * 13.11, 24634.635);
            let width_hash = hash(c * 5.37 + r * 29.11 + cycle * 7.43, 17391.443);
            let pulse_center = 0.2 + cycle_hash * 0.55;
            let pulse_width = 0.09 + width_hash * 0.08;
            let distance = (progress - pulse_center) / pulse_width;
            let envelope = (-distance * distance * 1.45).exp();
            let flicker = envelope * if cycle_hash > 0.12 { 1.0 } else { 0.26 };
            let coordinate = (x + eased_flow) * 9.0;
            let index = coordinate.floor();
            let flow_progress = smoothstep(0.0, 1.0, coordinate - index);
            let flow_a = hash(index * 18.31 + r * 37.17, 19283.174);
            let flow_b = hash((index + 1.0) * 18.31 + r * 37.17, 19283.174);
            let cluster = smoothstep(0.46, 0.84, mix(flow_a, flow_b, flow_progress));
            let wave_phase = (x + eased_flow + r * 0.06 + base * 0.02) * std::f32::consts::TAU;
            let wave = (0.5 + 0.5 * wave_phase.cos()).powi(5);
            let directional = cluster.max(wave * 0.62);
            let flowing =
                (flicker * (0.48 + directional * 0.58)).max(directional * (0.38 + base * 0.28));
            let glow = if reveal < 0.995 {
                (-((x - frontier).powi(2)) / 0.012).exp() * (1.0 - smoothstep(0.7, 1.0, reveal))
            } else {
                0.0
            };
            let light = flowing.max(glow * (0.4 + base * 0.4));
            let peak_on = light > 0.4 && flicker > 0.16 && cycle_hash > 0.26 && cluster > 0.04;
            let hottest = light > 0.68 && flicker > 0.3 && cycle_hash > 0.48 && cluster > 0.12;
            let highlight_amount = if peak_on {
                0.97
            } else {
                (light * (0.44 + cycle_hash * 0.3)).clamp(0.0, 0.64)
            };
            let drift = base * 0.28
                + depth * 0.28
                + progress * 0.38
                + eased_flow * 0.18
                + cycle_hash * 0.2
                + (elapsed * 0.00135 + phase * std::f32::consts::TAU).sin() * 0.14;
            let position = drift.rem_euclid(1.0) * tones.len() as f32;
            let tone_index = position.floor() as usize % tones.len();
            let tone = mix3(
                tones[tone_index],
                tones[(tone_index + 1) % tones.len()],
                position.fract(),
            );
            let nudge = (chroma - 0.5) * 10.0 + depth * 12.0;
            let varied = [
                (tone[0] + nudge * 0.35 - depth * 8.0).clamp(140.0, 196.0),
                (tone[1] - depth * 16.0 + (base - 0.5) * 8.0).clamp(104.0, 168.0),
                (tone[2] + depth * 6.0 + (cycle_hash - 0.5) * 6.0).clamp(182.0, 216.0),
            ];
            let base_color = mix3(left, varied, purple);
            let color = if hottest {
                mix3(base_color, peak, 0.95)
            } else {
                mix3(base_color, highlight, highlight_amount)
            };
            let opacity = if peak_on || hottest {
                reveal_alpha * intensity
            } else {
                reveal_alpha * intensity * (0.7 + base * 0.2 + flowing * 0.12).clamp(0.0, 1.0)
            };
            cells.push(Some(Color::rgba(
                color[0].round() as u8,
                color[1].round() as u8,
                color[2].round() as u8,
                (opacity * 255.0).round() as u8,
            )));
        }
    }
    (columns, cells)
}

/// The space inside a cell its colored square leaves on each side.
pub const INSET: f32 = GAP * 0.5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_field_sweeps_in_from_the_right_and_stays_dark_at_the_left() {
        let left = [210.0, 206.0, 214.0];
        let (columns, early) = field(190.0, 24.0, 100.0, 0.2, left);
        assert_eq!(columns, 38);
        // Early on only the right end has come in.
        assert!(early[0].is_none());
        assert!(early[columns - 1].is_some());
        let (_, settled) = field(190.0, 24.0, 5000.0, 1.0, left);
        // At the left end the field is all but clear; to the right it is violet.
        assert!(settled[0].is_none_or(|c| c.a < 10));
        let right = settled[columns - 1].unwrap();
        assert!(right.b > right.g && right.a > 100);
    }
}
