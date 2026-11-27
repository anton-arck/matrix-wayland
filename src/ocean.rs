//! Escena "ocean": bajo el mar, en caracteres. Unos pocos peces grandes y detallados (payaso, cirujano
//! y angel) entre rayos de luz, medusas, algas, corales y burbujas.
use super::ascii::*;
use super::sleet::smooth;
use super::{Painter, Rng, Scene};

#[derive(Clone, Copy, PartialEq)]
enum Species { Clown, Tang, Angel }

/// Que parte del pez cae en un punto.
#[derive(Clone, Copy, PartialEq)]
enum Part { Body, Fin, Tail }

// (especie, tamano): los peces que hay siempre, uno por franja de profundidad
const SHOAL: [(Species, f32); 5] =
    [(Species::Clown, 0.85), (Species::Tang, 1.0), (Species::Angel, 1.0), (Species::Clown, 1.15), (Species::Tang, 0.75)];

struct Fish {
    species: Species,
    len: f32, // largo y alto en celdas
    tall: f32,
    x: f32, // columna de su extremo izquierdo
    y: f32, // fila del eje del cuerpo
    speed: f32, // columnas por segundo, con signo
    phase: f32,
}

impl Species {
    fn size(self) -> (f32, f32) {
        match self { Species::Clown => (19.0, 8.0), Species::Tang => (22.0, 11.0), Species::Angel => (17.0, 16.0) }
    }

    /// Parte en (u, v): u va de -1 (cola) a 1 (hocico) y v de -1 (arriba) a 1 (abajo).
    /// `flap` mueve la cola.
    fn part(self, u: f32, v: f32, flap: f32) -> Option<Part> {
        // (grosor del cuerpo, alto de la aleta de arriba y de la de abajo)
        let (girth, top, bottom) = match self {
            Species::Clown => (0.74, 0.30, 0.22),
            Species::Tang => (0.80, 0.22, 0.18),
            Species::Angel => (0.58, 0.50, 0.50),
        };
        let along = (u - 0.18) / 0.82;
        let body = if along.abs() < 1.0 { girth * (1.0 - along * along).sqrt() } else { 0.0 };
        if u >= -0.62 && v.abs() <= body { return Some(Part::Body); }
        if u < -0.55 && u >= -1.0 {
            let t = (-0.55 - u) / 0.45; // 0 en el nacimiento, 1 en la punta
            let v = v - flap * t * 0.16;
            let fork = if self == Species::Angel { 0.0 } else { (t - 0.55).max(0.0) * 0.9 };
            return (v.abs() <= 0.16 + t * 0.62 && v.abs() >= fork).then_some(Part::Tail);
        }
        // aletas: triangulos inclinados hacia atras
        let (reach, peak) = if v < 0.0 { (top, if self == Species::Angel { -0.2 } else { 0.0 }) } else { (bottom, if self == Species::Angel { -0.2 } else { -0.1 }) };
        let spread = if u > peak { (u - peak) / 0.5 } else { (peak - u) / 0.35 };
        (body > 0.0 && v.abs() > body && v.abs() <= body + reach * (1.0 - spread)).then_some(Part::Fin)
    }

    /// Color de una parte; las rayas dependen de la posicion.
    fn color(self, part: Part, u: f32, v: f32) -> Rgb {
        match self {
            Species::Clown => {
                let stripe = [0.48, -0.02, -0.52].iter().any(|&s: &f32| (u - s + 0.06 * v * v).abs() < 0.075);
                if stripe && part != Part::Tail { [0.96, 0.97, 1.0] } else { [1.0, 0.46, 0.07] }
            }
            Species::Tang => match part {
                Part::Tail => [1.0, 0.84, 0.12],
                Part::Fin => [0.10, 0.22, 0.62],
                // la mancha oscura del lomo
                Part::Body if u > -0.45 && u < 0.35 && (v + 0.22 - 0.2 * u).abs() < 0.17 => [0.05, 0.08, 0.30],
                Part::Body => [0.13, 0.52, 1.0],
            },
            Species::Angel => {
                let stripe = [0.42, -0.08, -0.5].iter().any(|&s: &f32| (u - s).abs() < 0.085);
                if stripe { [0.16, 0.14, 0.22] } else if part == Part::Body { [0.93, 0.90, 0.72] } else { [1.0, 0.76, 0.20] }
            }
        }
    }
}

struct Bubble { x: f32, y: f32, speed: f32, phase: f32 }
struct Jelly { x: f32, y: f32, phase: f32, color: Rgb }
struct Weed { col: usize, height: usize, phase: f32, color: Rgb }

pub struct Ocean {
    grid: Grid,
    fish: Vec<Fish>,
    bubbles: Vec<Bubble>,
    vents: Vec<f32>, // columnas de donde salen burbujas
    jellies: Vec<Jelly>,
    weeds: Vec<Weed>,
    rng: Rng,
    t: f32,
}

/// El mismo caracter visto en un espejo.
fn mirror(glyph: u8) -> u8 {
    match glyph {
        SLASH => BACKSLASH, BACKSLASH => SLASH, OPEN => CLOSE, CLOSE => OPEN, LESS => GREATER, GREATER => LESS,
        other => other,
    }
}

/// Trazo del contorno segun por que lados se sale del pez (mirando a la derecha).
fn edge(up: bool, down: bool, left: bool, right: bool) -> Option<u8> {
    match (up, down, left, right) {
        (false, false, false, false) => None,
        (true, _, true, false) => Some(SLASH),
        (true, _, false, true) => Some(BACKSLASH),
        (false, true, true, false) => Some(BACKSLASH),
        (false, true, false, true) => Some(SLASH),
        (true, ..) | (_, true, ..) => Some(DASH),
        (_, _, true, _) => Some(OPEN),
        _ => Some(CLOSE),
    }
}

/// Fila de la arena (en alturas de pantalla) en `x`.
fn floor(x: f32) -> f32 { 0.93 - noise(x * 2.6, 4.0) * 0.07 - noise(x * 9.0, 2.0) * 0.015 }

impl Ocean {
    pub fn new() -> Self {
        Ocean { grid: Grid::default(), fish: vec![], bubbles: vec![], vents: vec![], jellies: vec![], weeds: vec![], rng: Rng::from_time(), t: 0.0 }
    }

    /// El pez numero `i`: entra por un lado (o, al empezar, ya esta dentro) en su franja.
    fn swimmer(&mut self, i: usize, inside: bool) -> Fish {
        let (species, size) = SHOAL[i];
        let cols = self.grid.cols as f32;
        let rows = self.grid.rows as f32;
        let scale = size * (rows / 59.0);
        let (len, tall) = species.size();
        let (len, tall) = ((len * scale).max(7.0).round(), (tall * scale).max(4.0).round());
        let right = self.rng.f() < 0.5;
        let speed = self.rng.rf(2.5, 5.5) * if right { 1.0 } else { -1.0 };
        let x = if inside {
            cols * (0.1 + 0.16 * ((i * 3) % 5) as f32) + self.rng.rf(-4.0, 4.0)
        } else if right {
            -len - self.rng.rf(0.0, cols * 0.08)
        } else {
            cols + self.rng.rf(0.0, cols * 0.08)
        };
        let lane = rows * (0.17 + 0.135 * i as f32);
        Fish { species, len, tall, x, y: lane + self.rng.rf(-2.0, 2.0), speed, phase: self.rng.rf(0.0, 6.28) }
    }

    fn populate(&mut self) {
        let aspect = self.grid.aspect();
        let cols = self.grid.cols;
        self.fish.clear();
        for i in 0..SHOAL.len() {
            let f = self.swimmer(i, true);
            self.fish.push(f);
        }
        self.weeds = (0..cols / 5).map(|_| {
            let green = self.rng.rf(0.35, 0.75);
            Weed {
                col: self.rng.range(0, cols as i32 - 1) as usize,
                height: self.rng.range(3, (self.grid.rows as i32 / 4).max(4)) as usize,
                phase: self.rng.rf(0.0, 6.28),
                color: [0.08, green, green * 0.45],
            }
        }).collect();
        self.vents = (0..4).map(|_| self.rng.rf(0.05, 0.95) * aspect).collect();
        self.jellies = (0..2).map(|i| Jelly {
            x: (0.2 + 0.55 * i as f32 + self.rng.rf(0.0, 0.1)) * aspect,
            y: self.rng.rf(0.2, 0.8),
            phase: self.rng.rf(0.0, 6.28),
            color: if i % 2 == 0 { [1.0, 0.55, 0.85] } else { [0.65, 0.65, 1.0] },
        }).collect();
        self.bubbles.clear();
    }

    /// El agua, la superficie, los rayos de luz y el fondo.
    fn water(&mut self) {
        let t = self.t;
        let rows = self.grid.rows as f32;
        for r in 0..self.grid.rows {
            for c in 0..self.grid.cols {
                let (x, y) = self.grid.center(c, r);
                let (c32, r32) = (c as i32, r as i32);
                let mut bg = mix([0.03, 0.27, 0.42], [0.004, 0.035, 0.10], smooth(0.0, 0.85, y));
                // rayos inclinados que bajan desde la superficie y se mecen
                let slant = x - y * 0.35 + (t * 0.13).sin() * 0.05;
                let ray = smooth(0.52, 0.85, noise(slant * 7.0, t * 0.12) * 0.75 + noise(slant * 19.0, t * 0.2) * 0.25) * (1.0 - smooth(0.1, 0.75, y));
                bg = add(bg, [0.10, 0.30, 0.34], ray * 0.55);
                let mut cell = Cell { glyph: 0, fg: bg, bg };
                if ray > 0.25 {
                    cell.glyph = if ray > 0.65 { SLASH } else { DOT };
                    cell.fg = add(bg, [0.3, 0.6, 0.65], ray * 0.55);
                } else if hash(c32, r32) > 0.975 {
                    cell.glyph = DOT; // particulas suspendidas
                    cell.fg = add(bg, [0.2, 0.35, 0.4], 0.35 + 0.25 * (t * 0.8 + hash(r32, c32) * 20.0).sin());
                }
                if r < 4 { // la superficie, vista desde abajo
                    let wave = noise(x * 11.0 + t * 0.5, r as f32 * 1.7 - t * 0.9) * (1.0 - r as f32 / 4.5);
                    if wave > 0.22 {
                        cell.glyph = if wave > 0.55 { EQUALS } else { TILDE };
                        cell.fg = mix([0.25, 0.6, 0.7], [0.85, 1.0, 1.0], wave);
                        cell.bg = add(cell.bg, [0.2, 0.4, 0.45], wave * 0.35);
                    }
                }
                let ground = floor(x);
                if y > ground {
                    let down = (y - ground) * rows;
                    let light = noise(x * 14.0 + t * 0.4, down * 0.6 - t * 0.25); // causticas en la arena
                    let sand = scale([0.30, 0.25, 0.16], 1.0 - (down / 12.0).min(0.7));
                    let grain = hash(c32, r32);
                    cell = Cell {
                        glyph: if down < 1.0 { TILDE } else if grain > 0.82 { 2 } else if grain > 0.45 { DOT } else { 0 },
                        fg: add(scale(sand, 1.7), [0.1, 0.2, 0.18], smooth(0.55, 0.9, light)),
                        bg: scale(sand, 0.55),
                    };
                    if down < 2.0 && noise(x * 17.0, 12.0) > 0.72 { // corales
                        cell.glyph = if grain > 0.5 { STAR } else { CARET };
                        cell.fg = if noise(x * 3.0, 20.0) > 0.5 { [0.95, 0.42, 0.45] } else { [1.0, 0.62, 0.25] };
                    }
                }
                self.grid.cells[r * self.grid.cols + c] = cell;
            }
        }
    }

    /// Pone un glifo en una celda (columna, fila), si cae dentro, sin tocar su fondo.
    fn put(grid: &mut Grid, col: i32, row: i32, glyph: u8, color: Rgb) {
        if col < 0 || row < 0 || col >= grid.cols as i32 || row >= grid.rows as i32 { return; }
        let cell = &mut grid.cells[row as usize * grid.cols + col as usize];
        cell.glyph = glyph;
        cell.fg = color;
    }

    fn draw_fish(grid: &mut Grid, f: &Fish, t: f32) {
        let mirrored = f.speed < 0.0;
        let (len, tall) = (f.len as i32, f.tall as i32);
        let left = f.x.round() as i32;
        let row = (f.y + (t * 0.9 + f.phase).sin() * 0.8).round() as i32;
        let flap = (t * 4.0 + f.phase).sin();
        // el pez mirando a la derecha: columna i, fila j de su caja
        let uv = |i: i32, j: i32| ((i as f32 + 0.5) / len as f32 * 2.0 - 1.0, (j as f32 + 0.5) / tall as f32 * 2.0 - 1.0);
        let part = |i: i32, j: i32| {
            let (u, v) = uv(i, j);
            f.species.part(u, v, flap)
        };
        let eye = (((0.64 + 1.0) * 0.5 * len as f32) as i32, ((-0.16 + 1.0) * 0.5 * tall as f32) as i32);
        for j in -1..=tall {
            for i in 0..len {
                let Some(kind) = part(i, j) else { continue };
                let (u, v) = uv(i, j);
                let color = f.species.color(kind, u, v);
                let belly = 1.0 - 0.22 * v.max(0.0); // la luz viene de arriba
                let color = scale(color, belly);
                let outline = edge(part(i, j - 1).is_none(), part(i, j + 1).is_none(), part(i - 1, j).is_none(), part(i + 1, j).is_none());
                let (mut glyph, mut ink, back) = match (outline, kind) {
                    (Some(g), _) => (g, mix(color, [1.0; 3], 0.45), 0.42),
                    (None, Part::Tail) => (EQUALS, mix(color, [1.0; 3], 0.25), 0.55),
                    (None, Part::Fin) => (if v < 0.0 { SLASH } else { BACKSLASH }, mix(color, [1.0; 3], 0.2), 0.5),
                    // escamas, y la branquia detras de la cabeza
                    (None, Part::Body) if (u - 0.36 + 0.12 * v * v).abs() < 1.0 / len as f32 && v.abs() < 0.45 => (CLOSE, scale(color, 0.25), 0.8),
                    (None, Part::Body) => (if (i + j) % 2 == 0 { CLOSE } else { 2 }, mix(color, [1.0; 3], 0.15), 0.7),
                };
                let mut bg = scale(color, back);
                if (i, j) == eye {
                    (glyph, ink, bg) = (AT, [0.02, 0.02, 0.05], [0.98, 0.98, 1.0]);
                }
                let (col, row) = (left + if mirrored { len - 1 - i } else { i }, row - tall / 2 + j);
                if col < 0 || row < 0 || col >= grid.cols as i32 || row >= grid.rows as i32 { continue; }
                let glyph = if mirrored && (i, j) != eye { mirror(glyph) } else { glyph };
                grid.cells[row as usize * grid.cols + col as usize] = Cell { glyph, fg: ink, bg };
            }
        }
    }

    fn swim(&mut self, dt: f32) {
        let cols = self.grid.cols as f32;
        for i in 0..self.fish.len() {
            let f = &mut self.fish[i];
            f.x += f.speed * dt;
            if (f.speed > 0.0 && f.x > cols + 4.0) || (f.speed < 0.0 && f.x < -f.len - 4.0) {
                self.fish[i] = self.swimmer(i, false);
            }
        }
    }

    fn life(&mut self, dt: f32) {
        let t = self.t;
        let grid = &mut self.grid;
        let height = (grid.rows * grid.ch) as f32;
        let (cell_w, rows) = (grid.cw as f32 / height, grid.rows as i32);
        for w in &self.weeds {
            let x = (w.col as f32 + 0.5) * cell_w;
            let base = (floor(x) * rows as f32) as i32;
            for k in 0..w.height {
                let sway = (t * 0.9 + w.phase + k as f32 * 0.45).sin();
                let col = w.col as i32 + (sway * k as f32 / w.height as f32 * 2.0).round() as i32;
                Ocean::put(grid, col, base - k as i32, if sway > 0.0 { CLOSE } else { OPEN }, scale(w.color, 0.65 + 0.35 * k as f32 / w.height as f32));
            }
        }
        for j in &mut self.jellies {
            let pulse = (t * 1.6 + j.phase).sin();
            j.y -= dt * (0.012 + 0.022 * pulse.max(0.0));
            if j.y < 0.02 { j.y = 1.05; }
            let col = (j.x / cell_w + (t * 0.25 + j.phase).sin() * 3.0).round() as i32;
            let row = (j.y * rows as f32).round() as i32;
            let wide = if pulse > 0.0 { 2 } else { 3 };
            for dx in -wide..=wide {
                let edge = if dx == -wide { SLASH } else if dx == wide { BACKSLASH } else { EQUALS };
                Ocean::put(grid, col + dx, row, edge, j.color);
                if dx.abs() < wide - 1 { Ocean::put(grid, col + dx, row - 1, DASH, j.color); }
                if dx.abs() < wide && dx % 2 == 0 {
                    for k in 1..=4 {
                        let wave = (t * 2.2 + k as f32 + dx as f32 + j.phase).sin();
                        Ocean::put(grid, col + dx, row + k, if wave > 0.0 { CLOSE } else { OPEN }, scale(j.color, 1.0 - 0.17 * k as f32));
                    }
                }
            }
        }
        // burbujas que suben en rachas desde unas grietas
        for &x in &self.vents {
            if self.rng.f() < (dt * (if (t * 0.4 + x * 5.0).sin() > 0.5 { 5.0 } else { 0.3 })) as f64 {
                self.bubbles.push(Bubble { x, y: floor(x), speed: self.rng.rf(0.07, 0.14), phase: self.rng.rf(0.0, 6.28) });
            }
        }
        for b in &mut self.bubbles {
            b.y -= b.speed * dt;
            if let Some(i) = grid.at(b.x + (t * 2.0 + b.phase).sin() * 0.007, b.y) {
                grid.cells[i].glyph = if b.speed > 0.105 { RING } else { DOT };
                grid.cells[i].fg = [0.65, 0.88, 0.95];
            }
        }
        self.bubbles.retain(|b| b.y > 0.03);
    }
}

impl Scene for Ocean {
    fn resize(&mut self, p: &mut Painter, w: usize, h: usize) {
        self.grid.resize(p, w, h);
        self.populate();
    }

    fn step(&mut self, p: &mut Painter, dt: f32) {
        self.t += dt;
        self.swim(dt);
        self.water();
        self.life(dt);
        for f in &self.fish { Ocean::draw_fish(&mut self.grid, f, self.t); }
        self.grid.present(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fish_have_body_tail_and_fins() {
        for species in [Species::Clown, Species::Tang, Species::Angel] {
            let mut seen = [false; 3];
            for i in 0..40 {
                for j in 0..40 {
                    let (u, v) = (i as f32 / 20.0 - 0.975, j as f32 / 20.0 - 0.975);
                    if let Some(part) = species.part(u, v, 0.0) { seen[part as usize] = true; }
                }
            }
            assert_eq!(seen, [true; 3]);
            assert!(species.part(0.2, 0.0, 0.0) == Some(Part::Body));
        }
        assert_eq!(mirror(mirror(SLASH)), SLASH);
    }

    #[test]
    fn the_same_few_fish_keep_coming_back() {
        let mut s = Ocean::new();
        let mut p = Painter::plain();
        s.resize(&mut p, 640, 360);
        let mut returns = 0;
        for _ in 0..30 * 300 {
            let before: Vec<f32> = s.fish.iter().map(|f| f.speed).collect();
            s.swim(1.0 / 30.0);
            returns += s.fish.iter().zip(before).filter(|(f, speed)| f.speed != *speed).count();
            assert_eq!(s.fish.len(), SHOAL.len());
        }
        assert!(returns > 5);
    }

    #[test]
    fn runs_at_any_size() {
        let mut s = Ocean::new();
        let mut p = Painter::plain();
        for &(w, h) in &[(640usize, 360usize), (160, 100), (300, 900), (1200, 300)] {
            s.resize(&mut p, w, h);
            for _ in 0..600 { s.step(&mut p, 1.0 / 15.0); }
            assert!(p.fb.iter().any(|&c| c != 0xFF00_0000));
        }
    }
}
