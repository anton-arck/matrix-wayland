//! Escena "stars": cielo estrellado en caracteres sobre unas lomas, con la via lactea y estrellas
//! fugaces que aparecen cada pocos segundos en sitios al azar.
use super::ascii::*;
use super::sleet::smooth;
use super::{Painter, Rng, Scene};

const TAIL: f32 = 0.22; // largo de la estela, en alturas de pantalla

struct Meteor { x: f32, y: f32, vx: f32, vy: f32, age: f32, life: f32 }

pub struct Stars {
    grid: Grid,
    sky: Vec<Cell>, // el cielo quieto
    twinkling: Vec<(usize, Rgb, f32, f32)>, // (celda, color, fase, velocidad)
    meteors: Vec<Meteor>,
    wait: f32, // segundos hasta la siguiente fugaz
    rng: Rng,
    t: f32,
}

/// Altura de las lomas en `x` (y de pantalla: 1 es abajo).
fn ridge(x: f32) -> f32 { 0.90 - noise(x * 2.3, 3.0) * 0.14 - noise(x * 7.0, 9.0) * 0.035 }

impl Stars {
    pub fn new() -> Self {
        Stars { grid: Grid::default(), sky: vec![], twinkling: vec![], meteors: vec![], wait: 2.0, rng: Rng::from_time(), t: 0.0 }
    }

    /// Una fugaz en un punto y direccion al azar de la parte alta del cielo.
    fn spawn(&mut self) {
        let aspect = self.grid.aspect();
        let side = if self.rng.f() < 0.5 { -1.0 } else { 1.0 };
        let angle = self.rng.rf(0.25, 0.85); // radianes bajo la horizontal
        let speed = self.rng.rf(0.55, 1.0);
        self.meteors.push(Meteor {
            x: self.rng.rf(0.08, 0.92) * aspect,
            y: self.rng.rf(0.04, 0.45),
            vx: side * angle.cos() * speed,
            vy: angle.sin() * speed,
            age: 0.0,
            life: self.rng.rf(0.7, 1.3),
        });
    }

    fn build(&mut self) {
        let g = &self.grid;
        self.sky = vec![Cell::default(); g.cols * g.rows];
        self.twinkling.clear();
        let aspect = g.aspect();
        for r in 0..g.rows {
            for c in 0..g.cols {
                let i = r * g.cols + c;
                let (x, y) = g.center(c, r);
                let mut bg = mix([0.008, 0.012, 0.035], [0.035, 0.065, 0.14], y);
                // la via lactea: una banda diagonal de polvo
                let across = (y - (0.72 - 0.55 * x / aspect)) / 0.17;
                let dust = noise(x * 9.0, y * 9.0) * 0.6 + noise(x * 23.0, y * 23.0) * 0.4;
                let milky = (-across * across).exp() * smooth(0.25, 0.85, dust);
                bg = add(bg, [0.17, 0.14, 0.24], milky * 0.55);
                let mut cell = Cell { glyph: 0, fg: bg, bg };
                if milky > 0.12 {
                    cell.glyph = if milky > 0.55 { 2 } else { DOT };
                    cell.fg = add(bg, [0.45, 0.42, 0.58], milky);
                }
                let (c32, r32) = (c as i32, r as i32);
                let chance = hash(c32, r32);
                if chance > 0.93 - milky * 0.08 {
                    let size = hash(c32 + 31, r32);
                    let tone = hash(c32, r32 + 57);
                    let color = if tone < 0.15 { [0.65, 0.78, 1.0] } else if tone > 0.85 { [1.0, 0.82, 0.58] } else { [0.93, 0.95, 1.0] };
                    cell.glyph = if size > 0.985 { AT } else if size > 0.95 { STAR } else if size > 0.86 { PLUS } else { DOT };
                    let bright = 0.35 + 0.65 * hash(c32 + 5, r32 + 11);
                    cell.fg = mix(bg, color, if size > 0.95 { 1.0 } else { bright });
                    if hash(c32 + 17, r32 + 3) > 0.55 {
                        self.twinkling.push((i, cell.fg, hash(r32, c32) * 6.28, 0.8 + 3.0 * hash(c32, c32 + r32)));
                    }
                }
                // las lomas, recortadas contra el cielo
                let top = ridge(x);
                if y > top {
                    let depth = ((y - top) / 0.08).min(1.0);
                    let bg = mix([0.012, 0.02, 0.032], [0.003, 0.005, 0.008], depth);
                    cell = if y - top < 1.0 / g.rows as f32 {
                        Cell { glyph: CARET, fg: [0.26, 0.35, 0.55], bg }
                    } else {
                        Cell { glyph: if chance > 0.8 { 2 } else { 0 }, fg: [0.09, 0.13, 0.2], bg }
                    };
                    self.twinkling.retain(|s| s.0 != i);
                }
                self.sky[i] = cell;
            }
        }
    }

    fn draw_meteor(grid: &mut Grid, m: &Meteor) {
        let speed = (m.vx * m.vx + m.vy * m.vy).sqrt();
        let (ux, uy) = (m.vx / speed, m.vy / speed);
        let glow = smooth(0.0, 0.12, m.age) * (1.0 - smooth(m.life * 0.65, m.life, m.age));
        let length = TAIL.min(speed * m.age);
        let slope = uy.atan2(ux.abs());
        let body = if slope < 0.38 { DASH } else if ux > 0.0 { BACKSLASH } else { SLASH };
        // una celda por columna (o por fila si cae muy empinada): asi la estela no sale doble
        let height = (grid.rows * grid.ch) as f32;
        let (cell_w, cell_h) = (grid.cw as f32 / height, grid.ch as f32 / height);
        let step = if ux.abs() / cell_w > uy.abs() / cell_h { cell_w / ux.abs() } else { cell_h / uy.abs() };
        let mut s = length;
        while s >= 0.0 {
            let (x, y) = (m.x - ux * s, m.y - uy * s);
            if let Some(i) = grid.at(x, y) {
                if y < ridge(x) {
                    let near = 1.0 - s / TAIL; // 1 en la cabeza
                    let cell = &mut grid.cells[i];
                    cell.glyph = if s < step { AT } else if near > 0.82 { STAR } else if near > 0.2 { body } else { DOT };
                    cell.fg = mix(cell.bg, mix([0.45, 0.6, 1.0], [1.0, 0.98, 0.9], near), glow * (0.25 + 0.75 * near));
                    cell.bg = add(cell.bg, [0.35, 0.45, 0.8], glow * near * near * 0.18);
                }
            }
            s -= step;
        }
    }
}

impl Scene for Stars {
    fn resize(&mut self, p: &mut Painter, w: usize, h: usize) {
        self.grid.resize(p, w, h);
        self.meteors.clear();
        self.build();
    }

    fn step(&mut self, p: &mut Painter, dt: f32) {
        self.t += dt;
        self.wait -= dt;
        if self.wait <= 0.0 {
            self.spawn();
            // de vez en cuando vienen dos seguidas
            self.wait = if self.rng.f() < 0.15 { self.rng.rf(0.2, 0.6) } else { self.rng.rf(3.0, 9.0) };
        }
        self.grid.cells.copy_from_slice(&self.sky);
        for &(i, color, phase, speed) in &self.twinkling {
            let k = 0.65 + 0.35 * (self.t * speed + phase).sin();
            self.grid.cells[i].fg = mix(self.grid.cells[i].bg, color, k);
        }
        for m in &mut self.meteors {
            m.age += dt;
            m.x += m.vx * dt;
            m.y += m.vy * dt;
            Stars::draw_meteor(&mut self.grid, m);
        }
        self.meteors.retain(|m| m.age < m.life);
        self.grid.present(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meteors_appear_in_different_places_and_leave() {
        let mut s = Stars::new();
        let mut p = Painter::plain();
        s.resize(&mut p, 640, 360);
        let mut starts = vec![];
        for _ in 0..30 * 120 {
            s.step(&mut p, 1.0 / 30.0);
            for m in s.meteors.iter().filter(|m| m.age <= 1.0 / 30.0) { starts.push((m.x - m.vx * m.age, m.y - m.vy * m.age)); }
            assert!(s.meteors.len() <= 4);
        }
        assert!(starts.len() >= 10, "en dos minutos pasan varias: {}", starts.len());
        assert!(starts.windows(2).all(|w| (w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs() > 1e-4));
    }

    #[test]
    fn runs_at_any_size() {
        let mut s = Stars::new();
        let mut p = Painter::plain();
        for &(w, h) in &[(640usize, 360usize), (160, 100), (300, 900), (1200, 300)] {
            s.resize(&mut p, w, h);
            for _ in 0..400 { s.step(&mut p, 1.0 / 30.0); }
            assert!(p.fb.iter().any(|&c| c != 0xFF00_0000));
        }
    }
}
