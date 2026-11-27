//! Escena "sunset": el sol se esconde en el mar, en caracteres. El ciclo dura CYCLE segundos: baja,
//! queda el crepusculo con las primeras estrellas, se funde a negro y vuelve a empezar.
use super::ascii::*;
use super::sleet::smooth;
use super::{Painter, Scene};

const CYCLE: f32 = 150.0;
const HORIZON: f32 = 0.60;
const SUN_R: f32 = 0.12;

#[derive(Default)]
pub struct Sunset { grid: Grid, t: f32 }

/// Altura del centro del sol (en alturas de pantalla, hacia abajo) en el segundo `p` del ciclo.
fn sun_height(p: f32) -> f32 { 0.30 + (HORIZON + SUN_R * 1.15 - 0.30) * (p / 110.0).min(1.0) }

impl Sunset {
    pub fn new() -> Self { Sunset::default() }

    fn paint(&mut self) {
        let t = self.t;
        let p = t % CYCLE;
        let fade = smooth(0.0, 4.0, p) * (1.0 - smooth(CYCLE - 5.0, CYCLE, p));
        let dusk = smooth(55.0, 125.0, p);
        let aspect = self.grid.aspect();
        let (sx, sy) = (aspect * 0.5, sun_height(p));
        // cuanto sol asoma sobre el horizonte
        let above = smooth(HORIZON + SUN_R, HORIZON - SUN_R, sy);
        let low = smooth(0.30, HORIZON, sy);
        let sun = mix([1.0, 0.88, 0.40], [1.0, 0.36, 0.14], low);
        let top = mix([0.10, 0.13, 0.32], [0.015, 0.02, 0.08], dusk);
        let middle = mix([0.52, 0.22, 0.38], [0.13, 0.05, 0.19], dusk);
        let horizon = mix([0.98, 0.52, 0.18], [0.48, 0.12, 0.14], dusk);
        let cell_h = 1.0 / self.grid.rows as f32;

        for r in 0..self.grid.rows {
            for c in 0..self.grid.cols {
                let (x, y) = self.grid.center(c, r);
                let cell = if y < HORIZON {
                    let h = y / HORIZON;
                    let mut sky = if h < 0.6 { mix(top, middle, h / 0.6) } else { mix(middle, horizon, (h - 0.6) / 0.4) };
                    let (dx, dy) = (x - sx, y - sy);
                    let d = (dx * dx + dy * dy).sqrt();
                    sky = add(sky, sun, (-(d * d) / 0.07).exp() * 0.28 * (0.25 + 0.75 * above));
                    // nubes largas que derivan despacio
                    let cloud = noise(x * 2.2 + t * 0.018, y * 17.0) * 0.7 + noise(x * 6.0 + t * 0.03, y * 34.0) * 0.3;
                    let cloud = smooth(0.54, 0.80, cloud) * smooth(0.02, 0.25, h) * (1.0 - smooth(0.9, 1.0, h));
                    let lit = mix([0.55, 0.24, 0.36], sun, (-(d * d) / 0.12).exp() * above.max(0.3));
                    let lit = scale(lit, 1.0 - 0.5 * dusk);
                    let mut cell = Cell { glyph: DOT, fg: add(scale(sky, 1.15), [1.0; 3], 0.04), bg: scale(sky, 0.78) };
                    if cloud > 0.05 {
                        cell.glyph = if cloud > 0.7 { EQUALS } else if cloud > 0.35 { TILDE } else { DASH };
                        cell.fg = mix(cell.fg, add(lit, sky, 0.35), cloud);
                        cell.bg = mix(cell.bg, scale(lit, 0.6), cloud * 0.6);
                    }
                    // primeras estrellas
                    if dusk > 0.5 && h < 0.7 && hash(c as i32, r as i32) > 0.975 {
                        let twinkle = 0.7 + 0.3 * (t * 2.0 + hash(r as i32, c as i32) * 20.0).sin();
                        cell.glyph = if hash(c as i32, r as i32 + 7) > 0.8 { PLUS } else { DOT };
                        cell.fg = mix(cell.fg, [0.95, 0.95, 1.0], (dusk - 0.5) * 2.0 * twinkle);
                    }
                    // el disco, con el borde suavizado
                    let cover = ((SUN_R - d) / cell_h + 0.5).clamp(0.0, 1.0);
                    if cover > 0.0 {
                        let hot = mix(sun, [1.0, 0.97, 0.75], (1.0 - d / SUN_R) * 0.5);
                        cell.glyph = if cover > 0.8 { AT } else { ramp(0.35 + cover * 0.55) };
                        cell.fg = mix(cell.fg, hot, 0.4 + 0.6 * cover);
                        cell.bg = mix(cell.bg, scale(sun, 0.72), cover);
                    }
                    cell
                } else {
                    let z = (y - HORIZON) / (1.0 - HORIZON); // 0 lejos, 1 cerca
                    let water = mix(scale(horizon, 0.40), [0.015, 0.035, 0.09], smooth(0.0, 0.7, z));
                    // olas: mas juntas y finas cuanto mas lejos
                    let depth = 1.0 / (z + 0.12);
                    let swell = (depth * 9.0 + t * 1.1 + noise(x * 4.0 * depth.min(4.0), depth * 3.0) * 6.0).sin();
                    let ripple = noise(x * (9.0 + 14.0 * (1.0 - z)) - t * 0.35, depth * 7.0 + t * 0.8);
                    let wave = swell * 0.6 + (ripple - 0.5) * 1.6;
                    let crest = smooth(0.1, 0.9, wave);
                    let mut cell = Cell {
                        glyph: if crest > 0.6 { TILDE } else if crest > 0.15 { DASH } else { DOT },
                        fg: add(scale(water, 1.35), mix(horizon, middle, z), crest * 0.35),
                        bg: scale(water, 0.8 + 0.25 * crest),
                    };
                    // el camino de luz del sol sobre el agua
                    let width = SUN_R * (0.45 + 1.1 * z);
                    let wobble = (noise(y * 40.0, t * 1.4) - 0.5) * 0.09 * (0.3 + z);
                    let dx = (x - sx + wobble) / width;
                    let glint = (-dx * dx).exp() * above * smooth(0.32, 0.78, noise(x * 22.0 + t * 0.4, depth * 11.0 - t * 1.7));
                    if glint > 0.08 {
                        cell.glyph = if glint > 0.55 { EQUALS } else if glint > 0.28 { TILDE } else { DASH };
                        cell.fg = mix(cell.fg, add(sun, [1.0, 0.9, 0.6], 0.15), glint * 1.4);
                        cell.bg = add(cell.bg, sun, glint * 0.22);
                    }
                    cell
                };
                self.grid.cells[r * self.grid.cols + c] = Cell { glyph: cell.glyph, fg: scale(cell.fg, fade), bg: scale(cell.bg, fade) };
            }
        }
        // un velero cruza de lado a lado
        let boat = aspect * (1.1 - (t * 0.006) % 1.2);
        let row = HORIZON - cell_h * 0.5;
        let cw = self.grid.cw as f32 / (self.grid.rows * self.grid.ch) as f32;
        for (dx, dy, glyph) in [(0.0, 3.0, BAR), (1.0, 2.0, BACKSLASH), (1.0, 1.0, BACKSLASH), (0.0, 2.0, BAR), (0.0, 1.0, BAR),
            (-1.0, 1.0, SLASH), (-2.0, 0.0, BACKSLASH), (-1.0, 0.0, EQUALS), (0.0, 0.0, EQUALS), (1.0, 0.0, EQUALS), (2.0, 0.0, SLASH)] {
            if let Some(i) = self.grid.at(boat + dx * cw, row - dy * cell_h) {
                self.grid.cells[i].glyph = glyph;
                self.grid.cells[i].fg = scale([0.05, 0.02, 0.05], fade);
            }
        }
    }
}

impl Scene for Sunset {
    fn resize(&mut self, p: &mut Painter, w: usize, h: usize) { self.grid.resize(p, w, h); }

    fn step(&mut self, p: &mut Painter, dt: f32) {
        self.t += dt;
        self.paint();
        self.grid.present(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sun_goes_down_and_hides() {
        assert!(sun_height(0.0) < sun_height(50.0));
        assert!(sun_height(120.0) - SUN_R > HORIZON, "al final queda bajo el horizonte");
        assert_eq!(sun_height(120.0), sun_height(140.0));
    }

    #[test]
    fn runs_at_any_size() {
        let mut s = Sunset::new();
        let mut p = Painter::plain();
        for &(w, h) in &[(640usize, 360usize), (160, 100), (300, 900), (1200, 300)] {
            s.resize(&mut p, w, h);
            for _ in 0..200 { s.step(&mut p, 1.0); }
            s.t = 20.0; // lejos del fundido a negro que une un ciclo con otro
            s.step(&mut p, 1.0 / 30.0);
            assert!(p.fb.iter().any(|&c| c != 0xFF00_0000));
        }
    }
}
