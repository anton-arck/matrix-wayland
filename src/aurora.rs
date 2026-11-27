//! Escena "aurora": cortinas de aurora boreal sobre montanas nevadas, un pinar y un lago que las
//! refleja, en caracteres.
use super::ascii::*;
use super::sleet::smooth;
use super::{Painter, Scene};

const SHORE: f32 = 0.76;

#[derive(Default)]
pub struct Aurora { grid: Grid, t: f32 }

fn mountain(x: f32) -> f32 { 0.68 - noise(x * 2.1, 1.0) * 0.20 - noise(x * 6.5, 5.0) * 0.05 }

/// Altura de la copa del pinar en `x`: triangulos de distinto tamano.
fn pines(x: f32) -> f32 {
    let step = 0.045;
    let cell = (x / step).floor() as i32;
    (cell - 1..=cell + 1).fold(SHORE - 0.012, |top, i| {
        let mid = (i as f32 + 0.5) * step;
        let tall = (0.35 + 0.65 * hash(i, 77)) * 0.15 * smooth(0.25, 0.6, noise(mid * 3.0, 8.0));
        let slope = 1.0 - (x - mid).abs() / (step * 0.75);
        if slope > 0.0 { top.min(SHORE - 0.012 - tall * slope) } else { top }
    })
}

/// Borde inferior, alto y fuerza de cada una de las tres cortinas en una columna.
type Column = [(f32, f32, f32); 3];

fn column(x: f32, t: f32) -> Column {
    let shapes = [(0.46, 0.24, 0.050, 1.0), (0.32, 0.18, 0.034, 0.7), (0.57, 0.12, 0.072, 0.55)];
    std::array::from_fn(|k| {
        let (base, height, speed, power): (f32, f32, f32, f32) = shapes[k];
        let k = k as f32;
        let foot = base + (x * 2.4 + t * speed * 2.0 + k * 2.0).sin() * 0.055 + (noise(x * 1.7 - t * speed, k * 9.0) - 0.5) * 0.17;
        let rays = 0.35 + 0.65 * smooth(0.2, 0.85, noise(x * 24.0 + noise(x * 3.0, t * 0.1) * 5.0 + k * 31.0, t * 0.35));
        let presence = smooth(0.2, 0.7, noise(x * 1.3 + k * 4.0 + t * speed * 0.6, t * 0.05 + k));
        (foot, height, rays * presence * power)
    })
}

/// Color e intensidad de las cortinas a la altura `y` de una columna.
fn curtains(col: &Column, y: f32) -> (Rgb, f32) {
    let mut color = [0.0; 3];
    let mut total = 0.0;
    for &(foot, height, strength) in col {
        let up = foot - y; // distancia sobre el borde inferior
        let fall = if up >= 0.0 { (-up / height).exp() } else { (up / 0.018).exp() };
        let v = fall * strength;
        let tone = mix([0.12, 1.0, 0.52], [0.58, 0.22, 0.92], smooth(0.15, 1.1, up / height));
        color = add(color, tone, v);
        total += v;
    }
    (color, total.min(1.0))
}

impl Aurora {
    pub fn new() -> Self { Aurora::default() }

    fn sky(&self, col: &Column, c: i32, r: i32, y: f32) -> Cell {
        let t = self.t;
        let mut bg = mix([0.008, 0.012, 0.04], [0.03, 0.07, 0.12], y / SHORE);
        let (light, v) = curtains(col, y);
        bg = add(bg, light, 0.30);
        let mut cell = Cell { glyph: 0, fg: bg, bg };
        if hash(c, r) > 0.955 {
            cell.glyph = if hash(c + 3, r) > 0.9 { PLUS } else { DOT };
            let twinkle = 0.6 + 0.4 * (t * 1.7 + hash(r, c) * 20.0).sin();
            cell.fg = add(bg, [0.7, 0.75, 0.9], twinkle * (1.0 - v));
        }
        if v > 0.07 {
            cell.glyph = if v > 0.72 { 7 } else if v > 0.52 { PLUS } else if v > 0.2 { BAR } else { 2 };
            let strength = v.max(0.001);
            cell.fg = add(bg, scale(light, 1.0 / strength), 0.18 + 0.75 * v);
        }
        cell
    }

    fn paint(&mut self) {
        let t = self.t;
        let rows = self.grid.rows as f32;
        let columns: Vec<Column> = (0..self.grid.cols).map(|c| column(self.grid.center(c, 0).0, t)).collect();
        let cell_w = self.grid.cw as f32 / (self.grid.rows * self.grid.ch) as f32;
        for r in 0..self.grid.rows {
            for c in 0..self.grid.cols {
                let (x, y) = self.grid.center(c, r);
                let (c32, r32) = (c as i32, r as i32);
                let cell = if y > SHORE {
                    // el lago: el cielo boca abajo, mas oscuro y roto por las ondas
                    let depth = (y - SHORE) / (1.0 - SHORE);
                    let shift = (y * 190.0 - t * 1.9).sin() * 0.006 * (0.3 + depth) + (noise(y * 40.0, t * 0.6) - 0.5) * 0.03 * depth;
                    let from = (c as f32 + shift / cell_w).round().clamp(0.0, (self.grid.cols - 1) as f32) as usize;
                    let (light, v) = curtains(&columns[from], 0.55 - (y - SHORE) * 1.2);
                    let broken = smooth(0.2, 0.75, noise(x * 7.0 - t * 0.2, y * 75.0 + t * 0.7));
                    let bg = add([0.008, 0.018, 0.04], light, 0.26 * (1.0 - depth * 0.4));
                    let glint = v * broken;
                    let mut cell = Cell {
                        glyph: if glint > 0.45 { EQUALS } else if glint > 0.2 { TILDE } else if broken > 0.55 { DASH } else { DOT },
                        fg: add(add(bg, [0.04, 0.07, 0.12], 1.0), scale(light, 1.0 / v.max(0.001)), 0.1 + glint * 0.9),
                        bg,
                    };
                    if y - SHORE < 1.0 / rows { cell = Cell { glyph: DASH, fg: [0.10, 0.17, 0.22], bg: [0.008, 0.015, 0.025] }; }
                    cell
                } else if y > pines(x) {
                    let edge = y - pines(x) < 1.0 / rows;
                    Cell {
                        glyph: if edge { CARET } else if hash(c32, r32) > 0.45 { if hash(r32, c32) > 0.5 { SLASH } else { BACKSLASH } } else { 0 },
                        fg: if edge { [0.16, 0.38, 0.32] } else { [0.07, 0.18, 0.16] },
                        bg: [0.004, 0.012, 0.014],
                    }
                } else if y > mountain(x) {
                    let top = mountain(x);
                    let down = y - top;
                    let rock = Cell { glyph: if hash(c32, r32) > 0.7 { 2 } else { 0 }, fg: [0.13, 0.19, 0.3], bg: [0.032, 0.05, 0.09] };
                    let (light, _) = curtains(&columns[c], 0.45);
                    // la nieve de las cumbres toma el color de la aurora
                    let line = 0.035 + (noise(x * 28.0, y * 18.0) - 0.5) * 0.05;
                    if top < 0.57 && down < line {
                        Cell { glyph: if down < 1.0 / rows { CARET } else { 4 }, fg: add([0.42, 0.50, 0.64], light, 0.22), bg: add([0.07, 0.10, 0.17], light, 0.08) }
                    } else if down < 1.0 / rows {
                        Cell { glyph: CARET, fg: add([0.13, 0.18, 0.28], light, 0.15), ..rock }
                    } else {
                        rock
                    }
                } else {
                    self.sky(&columns[c], c32, r32, y)
                };
                self.grid.cells[r * self.grid.cols + c] = cell;
            }
        }
    }
}

impl Scene for Aurora {
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
    fn curtains_move() {
        let at = |t: f32| (0..200).map(|i| curtains(&column(i as f32 * 0.009, t), 0.35).1).collect::<Vec<_>>();
        assert!(at(0.0).iter().any(|&v| v > 0.2));
        assert!(at(0.0) != at(5.0));
    }

    #[test]
    fn runs_at_any_size() {
        let mut s = Aurora::new();
        let mut p = Painter::plain();
        for &(w, h) in &[(640usize, 360usize), (160, 100), (300, 900), (1200, 300)] {
            s.resize(&mut p, w, h);
            for _ in 0..60 { s.step(&mut p, 0.5); }
            assert!(p.fb.iter().any(|&c| c != 0xFF00_0000));
        }
    }
}
