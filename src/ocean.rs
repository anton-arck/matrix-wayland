//! Escena "ocean": bajo el mar, en caracteres. Unos pocos peces grandes y detallados (payaso, cirujano
//! y angel) entre rayos de luz, medusas, algas, corales y burbujas. Los peces se dibujan a nivel de
//! pixel (silueta suave que ondula) con las letras encima, para que no salten de celda en celda.
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
    x: f32, // columna del centro
    y: f32, // fila del centro
    lane: f32,
    cruise: f32, // celdas por segundo
    speed: f32, // la de ahora: da un impulso y planea
    dir: f32, // hacia donde quiere ir: 1 o -1
    facing: f32, // hacia donde mira; pasa de un lado a otro al girarse
    pitch: f32, // inclinacion del cuerpo (positivo: morro abajo)
    beat: f32, // fase de la cola
    phase: f32,
    turn_in: f32, // segundos hasta que se plantea girarse
}

/// Cuanto se desvia el eje del cuerpo en `u`: la cabeza casi quieta y la cola amplia.
fn wave(u: f32, beat: f32) -> f32 {
    let back = ((1.0 - u) * 0.5).clamp(0.0, 1.0);
    (0.02 + 0.26 * back * back) * (beat + u * 2.6).sin()
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

/// Fila de la arena (en alturas de pantalla) en `x`.
fn floor(x: f32) -> f32 { 0.93 - noise(x * 2.6, 4.0) * 0.07 - noise(x * 9.0, 2.0) * 0.015 }

fn rgb_pixel(c: Rgb) -> u32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    0xFF00_0000 | b(c[0]) << 16 | b(c[1]) << 8 | b(c[2])
}

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
        let dir = if right { 1.0 } else { -1.0 };
        let x = if inside {
            cols * (0.18 + 0.16 * ((i * 3) % 5) as f32) + self.rng.rf(-4.0, 4.0)
        } else if right {
            -len * 0.5 - self.rng.rf(0.0, cols * 0.08)
        } else {
            cols + len * 0.5 + self.rng.rf(0.0, cols * 0.08)
        };
        let lane = rows * (0.17 + 0.135 * i as f32);
        let cruise = self.rng.rf(2.5, 5.0);
        Fish {
            species, len, tall, x, y: lane, lane, cruise, speed: cruise, dir, facing: dir, pitch: 0.0,
            beat: self.rng.rf(0.0, 6.28), phase: self.rng.rf(0.0, 6.28), turn_in: self.rng.rf(8.0, 30.0),
        }
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

    fn draw_fish(grid: &mut Grid, p: &mut Painter, f: &Fish, t: f32) {
        let (cw, ch) = (grid.cw as f32, grid.ch as f32);
        // medio largo y medio alto en pixeles; al girarse el pez se estrecha
        let (a, b) = (f.len * cw * 0.5, f.tall * ch * 0.5);
        let side = if f.facing < 0.0 { -1.0 } else { 1.0 };
        let wide = a * f.facing.abs().max(0.12);
        let (cx, cy) = (f.x * cw, f.y * ch);
        let (sin, cos) = (f.pitch.sin(), f.pitch.cos());
        // la onda del cuerpo y el brillo del lomo solo dependen de u: se tabulan una vez por pez
        let table: [(f32, f32); 128] = std::array::from_fn(|i| {
            let u = i as f32 / 127.0 * 2.4 - 1.2;
            (wave(u, f.beat), 0.16 * (0.6 + 0.4 * (u * 5.0 - t * 2.0).sin()))
        });
        let look = |u: f32| table[(((u + 1.2) * (127.0 / 2.4)) as usize).min(127)];
        let mouth_open = 0.025 + 0.025 * (t * 2.3 + f.phase).sin().max(0.0);
        let flap = (t * 5.5 + f.phase).sin(); // la aleta del costado, que se abre y se cierra
        let (fin_sin, fin_cos) = (0.25 + 0.45 * flap).sin_cos();
        let (per_wide, per_tall) = (1.0 / wide, 1.0 / b);
        // de la pantalla a las coordenadas del pez (u hacia el morro, v hacia abajo)
        let local = |x: f32, y: f32| {
            let (dx, dy) = ((x - cx) * side, y - cy);
            let u = (dx * cos + dy * sin) * per_wide;
            (u, (dy * cos - dx * sin) * per_tall - look(u).0)
        };
        let at = |x: f32, y: f32| {
            let (u, v) = local(x, y);
            f.species.part(u, v, 0.0).map(|part| (part, u, v))
        };
        let shade = |part: Part, u: f32, v: f32| {
            let mut color = scale(f.species.color(part, u, v), 1.0 - 0.28 * v.clamp(-0.3, 1.0)); // la luz viene de arriba
            // un brillo que recorre el lomo
            let back = (1.0 - ((v + 0.36) * 5.0).powi(2)).max(0.0);
            if back > 0.0 { color = add(color, [0.5, 0.7, 0.8], back * look(u).1); }
            if part != Part::Body { return color; }
            let gill = (u - 0.36 + 0.14 * v * v).abs() < 0.022 && v.abs() < 0.45;
            let mouth = u > 0.84 && (v - 0.16).abs() < mouth_open;
            if gill || mouth { scale(color, 0.35) } else { color }
        };

        let reach = a * sin.abs() + b * 1.45;
        let (x0, x1) = (((cx - a * 1.05) / cw).floor() as i32, ((cx + a * 1.05) / cw).ceil() as i32);
        let (y0, y1) = (((cy - reach) / ch).floor() as i32, ((cy + reach) / ch).ceil() as i32);
        for row in y0.max(0)..y1.min(grid.rows as i32) {
            for col in x0.max(0)..x1.min(grid.cols as i32) {
                let (left, top) = (col as f32 * cw, row as f32 * ch);
                let probes = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.5), (0.5, 0.0), (0.5, 1.0)];
                let hits = probes.iter().filter(|&&(i, j)| at(left + i * cw, top + j * ch).is_some()).count();
                if hits == 0 { continue; }
                let solid = hits == probes.len();
                // la letra de la celda: escamas, radios de las aletas y de la cola
                let letter = at(left + cw * 0.5, top + ch * 0.5).map(|(part, u, v)| {
                    let glyph = match part {
                        Part::Tail => EQUALS,
                        Part::Fin => if (v < 0.0) == (side > 0.0) { SLASH } else { BACKSLASH },
                        Part::Body if (col + row) % 2 == 0 => if side > 0.0 { CLOSE } else { OPEN },
                        Part::Body => 2,
                    };
                    (glyph, mix(shade(part, u, v), [1.0; 3], 0.38))
                });
                let cell = row as usize * grid.cols + col as usize;
                grid.invalidate(cell);
                for dy in 0..grid.ch {
                    let py = top as usize + dy;
                    if py >= p.h { break; }
                    for dx in 0..grid.cw {
                        let px = left as usize + dx;
                        if px >= p.w { break; }
                        let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
                        // por dentro basta una muestra; en el borde, cuatro para suavizarlo
                        let offsets: &[(f32, f32)] = if solid { &[(0.0, 0.0)] } else { &[(-0.25, -0.25), (0.25, -0.25), (-0.25, 0.25), (0.25, 0.25)] };
                        let mut sum = [0.0; 3];
                        let mut inside = 0;
                        for &(ox, oy) in offsets {
                            if let Some((part, u, v)) = at(x + ox, y + oy) {
                                sum = add(sum, shade(part, u, v), if part == Part::Body { 0.78 } else { 0.62 });
                                inside += 1;
                            }
                        }
                        if inside == 0 { continue; }
                        let mut color = scale(sum, 1.0 / inside as f32);
                        if let Some((glyph, ink)) = letter {
                            color = mix(color, ink, grid.cover(glyph, dx, dy) as f32 / 255.0 * 0.85);
                        }
                        let (u, v) = local(x, y);
                        // aleta del costado
                        let (fu, fv) = (u - 0.12, v - 0.22);
                        let (along, across) = (-fu * fin_cos + fv * fin_sin, fu * fin_sin + fv * fin_cos);
                        if (0.0..0.34).contains(&along) && across.abs() < 0.11 * (1.0 - (along / 0.34 - 0.5).powi(2) * 3.0) {
                            color = mix(color, add(color, [1.0; 3], 0.35), 0.7);
                        }
                        let alpha = inside as f32 / offsets.len() as f32;
                        let under = p.fb[py * p.w + px];
                        let under = [(under >> 16) & 255, (under >> 8) & 255, under & 255].map(|c| c as f32 / 255.0);
                        p.fb[py * p.w + px] = rgb_pixel(mix(under, color, alpha));
                    }
                }
            }
        }

        // el ojo, redondo y mirando hacia delante
        let (eu, ev) = (0.64, -0.14 + wave(0.64, f.beat));
        let (ex, ey) = (cx + side * (eu * wide * cos - ev * b * sin), cy + eu * wide * sin + ev * b * cos);
        let radius = (b * 0.15).max(2.5);
        let blink = if (t * 0.45 + f.phase).sin() > 0.995 { 0.2 } else { 1.0 };
        for py in (ey - radius - 1.0).max(0.0) as usize..((ey + radius + 2.0).max(0.0) as usize).min(p.h) {
            for px in (ex - radius - 1.0).max(0.0) as usize..((ex + radius + 2.0).max(0.0) as usize).min(p.w) {
                let (dx, dy) = (px as f32 + 0.5 - ex, (py as f32 + 0.5 - ey) / blink);
                let white = (radius - dx.hypot(dy) + 0.5).clamp(0.0, 1.0);
                if white == 0.0 { continue; }
                let pupil = (radius * 0.58 - (dx - side * radius * 0.22).hypot(dy) + 0.5).clamp(0.0, 1.0);
                let glint = (radius * 0.18 - (dx - side * radius * 0.4).hypot(dy + radius * 0.25) + 0.5).clamp(0.0, 1.0);
                let eye = mix(mix([0.95, 0.97, 1.0], [0.01, 0.01, 0.04], pupil), [1.0; 3], glint);
                let under = p.fb[py * p.w + px];
                let under = [(under >> 16) & 255, (under >> 8) & 255, under & 255].map(|c| c as f32 / 255.0);
                p.fb[py * p.w + px] = rgb_pixel(mix(under, eye, white));
                grid.invalidate((py / grid.ch) * grid.cols + px / grid.cw);
            }
        }
    }

    fn swim(&mut self, dt: f32) {
        let cols = self.grid.cols as f32;
        let rows = self.grid.rows as f32;
        let t = self.t;
        for i in 0..self.fish.len() {
            let f = &mut self.fish[i];
            // impulso y planeo: la cola bate mas deprisa cuando empuja
            let push = (t * 0.7 + f.phase).sin().max(0.0).powi(2);
            let goal = f.cruise * (0.6 + 1.1 * push);
            f.speed += (goal - f.speed) * (dt * 1.8).min(1.0);
            f.beat = (f.beat + dt * (2.2 + 1.3 * f.speed * (0.4 + push))) % std::f32::consts::TAU;
            // girarse tarda cerca de un segundo
            f.facing += (f.dir - f.facing).clamp(-2.4 * dt, 2.4 * dt);
            f.x += f.speed * f.facing * dt;
            // sube y baja sin prisa alrededor de su franja, inclinando el morro
            let target = f.lane + (t * 0.23 + f.phase).sin() * rows * 0.07 + (t * 0.57 + f.phase * 2.0).sin() * rows * 0.025;
            let rise = (target - f.y) * 0.8;
            f.y += rise * dt;
            let slope = (rise * 10.0 / 9.0 / f.speed.max(1.0)).atan().clamp(-0.32, 0.32);
            f.pitch += (slope - f.pitch) * (dt * 2.0).min(1.0);
            // a veces da media vuelta, si esta bien dentro de la pantalla
            f.turn_in -= dt;
            if f.turn_in <= 0.0 {
                f.turn_in = self.rng.rf(10.0, 35.0);
                if f.x > cols * 0.2 && f.x < cols * 0.8 { f.dir = -f.dir; }
            }
            let f = &self.fish[i];
            if (f.dir > 0.0 && f.x - f.len * 0.5 > cols + 4.0) || (f.dir < 0.0 && f.x + f.len * 0.5 < -4.0) {
                self.fish[i] = self.swimmer(i, false);
            }
        }
        // de vez en cuando alguno suelta una burbuja
        if self.rng.f() < (dt * 0.9) as f64 {
            let f = &self.fish[(self.rng.next() % self.fish.len() as u64) as usize];
            let height = (self.grid.rows * self.grid.ch) as f32;
            let x = (f.x + f.facing * f.len * 0.52) * self.grid.cw as f32 / height;
            let y = f.y / self.grid.rows as f32;
            let (speed, phase) = (self.rng.rf(0.07, 0.14), self.rng.rf(0.0, 6.28));
            self.bubbles.push(Bubble { x, y, speed, phase });
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
        self.grid.present(p);
        for f in &self.fish { Ocean::draw_fish(&mut self.grid, p, f, self.t); }
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
        // el cuerpo ondula: la cola se mueve mucho mas que la cabeza
        let sway = |u: f32| (0..60).map(|k| wave(u, k as f32 * 0.1).abs()).fold(0.0, f32::max);
        assert!(sway(-1.0) > 5.0 * sway(0.9));
    }

    #[test]
    fn the_same_few_fish_keep_coming_back_and_turn() {
        let mut s = Ocean::new();
        let mut p = Painter::plain();
        s.resize(&mut p, 640, 360);
        let (mut returns, mut turns) = (0, 0);
        for _ in 0..30 * 600 {
            let before: Vec<(f32, f32)> = s.fish.iter().map(|f| (f.x, f.dir)).collect();
            s.t += 1.0 / 30.0;
            s.swim(1.0 / 30.0);
            for (f, (x, dir)) in s.fish.iter().zip(before) {
                if (f.x - x).abs() > 5.0 { returns += 1; } else if f.dir != dir { turns += 1; }
                assert!(f.x.is_finite() && f.y.is_finite() && f.facing.abs() <= 1.0);
            }
            assert_eq!(s.fish.len(), SHOAL.len());
        }
        assert!(returns > 5 && turns > 5, "vuelven {returns}, giran {turns}");
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
