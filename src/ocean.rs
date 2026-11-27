//! Escena "ocean": bajo el mar, en caracteres. Rayos de luz, cardumenes y peces sueltos, un pez grande
//! que pasa de vez en cuando, medusas, algas, corales y burbujas.
use super::ascii::*;
use super::sleet::smooth;
use super::{Painter, Rng, Scene};

// dibujos mirando a la derecha; hacia la izquierda se reflejan
const SMALL: [&str; 1] = ["><>"];
const MEDIUM: [&[&str]; 3] = [&["><((o>"], &["><|||o>"], &["  /\\", "><==o>", "  \\/"]];
const BIG: [&str; 5] = [
    r"       /|",
    r"\    /====\",
    r" \ /===|=== o\",
    r" / \===|===  /",
    r"/    \====/",
];
const COLORS: [Rgb; 6] = [
    [1.0, 0.55, 0.15], [1.0, 0.85, 0.25], [0.95, 0.35, 0.40], [0.55, 0.95, 0.75], [0.75, 0.55, 1.0], [0.85, 0.92, 1.0],
];

struct Fish {
    art: &'static [&'static str],
    x: f32, // columna de su extremo izquierdo
    y: f32, // fila de arriba
    speed: f32, // columnas por segundo, con signo
    phase: f32,
    color: Rgb,
}

struct Bubble { x: f32, y: f32, speed: f32, phase: f32 }
struct Jelly { x: f32, y: f32, phase: f32, color: Rgb }
struct Weed { col: usize, height: usize, phase: f32, color: Rgb }

pub struct Ocean {
    grid: Grid,
    fish: Vec<Fish>,
    big: Option<Fish>,
    big_wait: f32,
    bubbles: Vec<Bubble>,
    vents: Vec<f32>, // columnas de donde salen burbujas
    jellies: Vec<Jelly>,
    weeds: Vec<Weed>,
    rng: Rng,
    t: f32,
}

/// Glifo de un caracter del dibujo, reflejado si el pez va hacia la izquierda.
fn glyph(c: char, mirrored: bool) -> Option<u8> {
    let c = match (c, mirrored) {
        ('>', true) => '<', ('<', true) => '>', ('(', true) => ')', (')', true) => '(',
        ('/', true) => '\\', ('\\', true) => '/', _ => c,
    };
    Some(match c {
        '>' => GREATER, '<' => LESS, '(' => OPEN, ')' => CLOSE, 'o' => RING, '=' => EQUALS,
        '|' => BAR, '/' => SLASH, '\\' => BACKSLASH, '-' => DASH,
        _ => return None,
    })
}

fn width_of(art: &[&str]) -> f32 { art.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32 }

/// Fila de la arena (en alturas de pantalla) en `x`.
fn floor(x: f32) -> f32 { 0.93 - noise(x * 2.6, 4.0) * 0.07 - noise(x * 9.0, 2.0) * 0.015 }

impl Ocean {
    pub fn new() -> Self {
        Ocean { grid: Grid::default(), fish: vec![], big: None, big_wait: 12.0, bubbles: vec![], vents: vec![], jellies: vec![], weeds: vec![], rng: Rng::from_time(), t: 0.0 }
    }

    fn color(&mut self) -> Rgb { COLORS[(self.rng.next() % COLORS.len() as u64) as usize] }

    /// Un pez que entra por un lado (o, al empezar, ya esta dentro).
    fn swimmer(&mut self, art: &'static [&'static str], inside: bool) -> Fish {
        let cols = self.grid.cols as f32;
        let rows = self.grid.rows as f32;
        let right = self.rng.f() < 0.5;
        let speed = self.rng.rf(2.5, 7.5) * if right { 1.0 } else { -1.0 };
        let x = if inside { self.rng.rf(0.0, cols) } else if right { -width_of(art) - self.rng.rf(0.0, cols * 0.3) } else { cols + self.rng.rf(0.0, cols * 0.3) };
        Fish { art, x, y: self.rng.rf(rows * 0.12, rows * 0.76), speed, phase: self.rng.rf(0.0, 6.28), color: self.color() }
    }

    /// Peces pequenos que nadan juntos.
    fn school(&mut self, inside: bool) -> Vec<Fish> {
        let leader = self.swimmer(&SMALL, inside);
        let n = self.rng.range(9, 16);
        (0..n).map(|_| Fish {
            art: &SMALL,
            x: leader.x + self.rng.rf(-11.0, 11.0),
            y: leader.y + self.rng.rf(-4.0, 4.0),
            speed: leader.speed * self.rng.rf(0.96, 1.04),
            phase: self.rng.rf(0.0, 6.28),
            color: leader.color,
        }).collect()
    }

    fn populate(&mut self) {
        let aspect = self.grid.aspect();
        let cols = self.grid.cols;
        self.fish.clear();
        self.big = None;
        for _ in 0..3 {
            let s = self.school(true);
            self.fish.extend(s);
        }
        for i in 0..8 {
            let f = self.swimmer(MEDIUM[i % MEDIUM.len()], true);
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
        self.jellies = (0..3).map(|i| Jelly {
            x: (0.15 + 0.32 * i as f32 + self.rng.rf(0.0, 0.1)) * aspect,
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
        let width = width_of(f.art) as i32;
        let row = (f.y + (t * 1.3 + f.phase).sin() * 0.7).round() as i32;
        let left = f.x.round() as i32;
        let depth = (row as f32 / grid.rows.max(1) as f32).clamp(0.0, 1.0);
        let color = mix(f.color, [0.05, 0.22, 0.35], depth * 0.45); // el agua se come el color
        for (dy, line) in f.art.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            let inked = |i: usize| chars[i] != ' ';
            let first = (0..chars.len()).find(|&i| inked(i));
            let last = (0..chars.len()).rev().find(|&i| inked(i));
            let (Some(first), Some(last)) = (first, last) else { continue };
            for i in first..=last {
                let dx = if mirrored { width - 1 - i as i32 } else { i as i32 };
                let (col, row) = (left + dx, row + dy as i32);
                match glyph(chars[i], mirrored) {
                    Some(g) => Ocean::put(grid, col, row, g, color),
                    None => Ocean::put(grid, col, row, 0, color), // dentro del pez no se ve el fondo
                }
            }
        }
    }

    fn swim(&mut self, dt: f32) {
        let cols = self.grid.cols as f32;
        let gone = |f: &Fish| if f.speed > 0.0 { f.x > cols + 12.0 } else { f.x < -width_of(f.art) - 12.0 };
        for f in &mut self.fish { f.x += f.speed * dt; }
        // cada cardumen que sale entero vuelve a entrar como otro
        let schools = self.fish.iter().filter(|f| f.art == SMALL).count();
        if schools > 0 {
            let lost = self.fish.iter().filter(|f| f.art == SMALL && gone(f)).count();
            self.fish.retain(|f| !(f.art == SMALL && gone(f)));
            if lost > 0 && schools - lost < 18 {
                let s = self.school(false);
                self.fish.extend(s);
            }
        } else {
            let s = self.school(false);
            self.fish.extend(s);
        }
        for i in 0..self.fish.len() {
            if self.fish[i].art != SMALL && gone(&self.fish[i]) {
                let art = self.fish[i].art;
                self.fish[i] = self.swimmer(art, false);
            }
        }
        // el pez grande
        match &mut self.big {
            Some(f) => {
                f.x += f.speed * dt;
                if gone(f) {
                    self.big = None;
                    self.big_wait = self.rng.rf(20.0, 45.0);
                }
            }
            None => {
                self.big_wait -= dt;
                if self.big_wait <= 0.0 {
                    let mut f = self.swimmer(&BIG, false);
                    f.x = if f.speed > 0.0 { -width_of(&BIG) } else { cols };
                    f.speed = f.speed.signum() * self.rng.rf(3.0, 4.5);
                    f.y = self.rng.rf(0.25, 0.55) * self.grid.rows as f32;
                    self.big = Some(f);
                }
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
        if let Some(f) = &self.big { Ocean::draw_fish(&mut self.grid, f, self.t * 0.5); }
        for f in &self.fish { Ocean::draw_fish(&mut self.grid, f, self.t); }
        self.grid.present(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirrored_art_swaps_pairs() {
        assert_eq!(glyph('>', true), Some(LESS));
        assert_eq!(glyph('(', true), Some(CLOSE));
        assert_eq!(glyph('/', true), Some(BACKSLASH));
        for art in MEDIUM.iter().copied().chain([&SMALL[..], &BIG[..]]) {
            for c in art.iter().flat_map(|l| l.chars()).filter(|&c| c != ' ') { assert!(glyph(c, false).is_some(), "sin glifo: {c}"); }
        }
    }

    #[test]
    fn the_sea_never_empties() {
        let mut s = Ocean::new();
        let mut p = Painter::plain();
        s.resize(&mut p, 640, 360);
        let mut saw_big = false;
        for _ in 0..30 * 600 {
            s.swim(1.0 / 30.0);
            saw_big |= s.big.is_some();
            let small = s.fish.iter().filter(|f| f.art == SMALL).count();
            assert!(small > 0 && small < 80, "peces pequenos: {small}");
            assert_eq!(s.fish.len() - small, 8);
        }
        assert!(saw_big);
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
