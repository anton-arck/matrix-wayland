//! Escena "sleet": una noche con aguanieve sobre un puente de piedra con ocho farolas, en ASCII.
//! Portada de un script de Python (sleet.py). Las capas quietas (cielo, puente, halos) se calculan una
//! vez en `resize`; cada fotograma rehace solo las celdas que cambian (el rio y, cuando pasa, la zona
//! del coche) y encima pinta el parpadeo de las farolas, la nieve y la lluvia.
use super::{Painter, Pal, Rng, Scene};

// El mundo se mide en las coordenadas del original; se ve la franja VIEW_W x VIEW_H centrada en CENTER_Y.
const VIEW_W: f32 = 1080.0;
const VIEW_H: f32 = 608.0;
const CENTER_Y: f32 = 684.0;
const WORLD_H: f32 = 1100.0;
const FOCAL: f32 = 1180.0;
const CX: f32 = 540.0;
const HORIZON: f32 = 610.0;
const EYE: f32 = 5.4;
const NEAR_END: (f32, f32) = (-9.5, 17.0); // (x, z) del puente, junto a nosotros
const FAR_END: (f32, f32) = (30.0, 85.0);
const WIDTH: f32 = 6.0;
const DECK: f32 = 3.6;
const PARAPET: f32 = 4.45;
const LAMP_TOP: f32 = 8.0;
const LAMPS: usize = 8;
const BG: f32 = 5.0 / 255.0;
const SEED: u64 = 0x9E37_79B9_7F4A_7C15 ^ 1107; // la misma semilla, la misma noche
const CYCLE: f64 = 45.0; // cada cuanto pasan el coche y, detras, la rafaga
const LIGHT_STEP: usize = 4;
const CROSSING: f32 = 3.0; // segundos que tarda el coche en cruzar
const WATER_EVERY: f32 = 0.095; // el agua es lenta: ~10 veces por segundo bastan
const CAR_EVERY: f32 = 0.06; // el coche, 15 veces por segundo como mucho

type P = (f32, f32);

pub(crate) fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let u = ((x - a) / (b - a)).clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// Proyeccion a coordenadas del mundo 2D (no de pantalla).
fn project(x: f32, y: f32, z: f32) -> P { (CX + FOCAL * x / z, HORIZON + FOCAL * (EYE - y) / z) }

/// (x, z) del punto a una fraccion `s` del puente, `across` metros hacia el otro lado.
fn bridge_xz(s: f32, across: f32) -> P {
    let (dx, dz) = (FAR_END.0 - NEAR_END.0, FAR_END.1 - NEAR_END.1);
    let span = (dx * dx + dz * dz).sqrt();
    (NEAR_END.0 + dx * s - dz / span * across, NEAR_END.1 + dz * s + dx / span * across)
}

fn on_bridge(s: f32, y: f32, across: f32) -> P {
    let (x, z) = bridge_xz(s, across);
    project(x, y, z)
}

fn interp(x: f32, xs: &[f32], ys: &[f32]) -> f32 {
    let i = xs.partition_point(|&v| v < x);
    if i == 0 { return ys[0]; }
    if i == xs.len() { return ys[i - 1]; }
    ys[i - 1] + (ys[i] - ys[i - 1]) * (x - xs[i - 1]) / (xs[i] - xs[i - 1])
}

// ---------- lienzo en escala de grises ----------
#[derive(Clone, Copy)]
struct View { w: usize, h: usize, k: f32, ox: f32, oy: f32 }

impl View {
    fn new(w: usize, h: usize) -> Self {
        let k = (w as f32 / VIEW_W).max(h as f32 / VIEW_H);
        View { w, h, k, ox: w as f32 / 2.0 - CX * k, oy: h as f32 / 2.0 - CENTER_Y * k }
    }
    fn screen(&self, p: P) -> P { (p.0 * self.k + self.ox, p.1 * self.k + self.oy) }
    /// Coordenadas del mundo del centro de un pixel.
    fn world(&self, x: usize, y: usize) -> P { ((x as f32 + 0.5 - self.ox) / self.k, (y as f32 + 0.5 - self.oy) / self.k) }
    fn buf(&self) -> Vec<f32> { vec![0.0; self.w * self.h] }

    /// Rellena un poligono dado en coordenadas del mundo (regla par-impar).
    fn poly(&self, buf: &mut [f32], pts: &[P], v: f32) {
        let s: Vec<P> = pts.iter().map(|&p| self.screen(p)).collect();
        let (lo, hi) = s.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
        let (y0, y1) = ((lo - 0.5).ceil().max(0.0) as usize, (hi - 0.5).floor().min(self.h as f32 - 1.0));
        if y1 < 0.0 { return; }
        let mut xs = Vec::new();
        for y in y0..=y1 as usize {
            let cy = y as f32 + 0.5;
            xs.clear();
            for i in 0..s.len() {
                let (a, b) = (s[i], s[(i + 1) % s.len()]);
                if (a.1 <= cy) != (b.1 <= cy) { xs.push(a.0 + (cy - a.1) / (b.1 - a.1) * (b.0 - a.0)); }
            }
            xs.sort_by(f32::total_cmp);
            for pair in xs.chunks_exact(2) {
                let a = (pair[0] - 0.5).ceil().clamp(0.0, self.w as f32) as usize;
                let b = (pair[1] - 0.5).ceil().clamp(0.0, self.w as f32) as usize;
                if a < b { buf[y * self.w + a..y * self.w + b].fill(v); }
            }
        }
    }

    fn line(&self, buf: &mut [f32], a: P, b: P, width: f32, v: f32) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt().max(1e-3);
        let half = (width * self.k).max(1.0) / 2.0 / self.k;
        let (nx, ny) = (-dy / len * half, dx / len * half);
        self.poly(buf, &[(a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny), (b.0 - nx, b.1 - ny), (a.0 - nx, a.1 - ny)], v);
    }
}

/// Desenfoque (tres pasadas de caja por eje, casi gaussiano).
fn blur(src: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    fn pass(src: &[f32], dst: &mut [f32], n: usize, lines: usize, step: usize, stride: usize, r: usize) {
        let norm = 1.0 / (2 * r + 1) as f32;
        for l in 0..lines {
            let at = |i: isize| src[l * stride + i.clamp(0, n as isize - 1) as usize * step];
            let r = r as isize;
            let mut sum: f32 = (-r..=r).map(at).sum();
            for i in 0..n as isize {
                dst[l * stride + i as usize * step] = sum * norm;
                sum += at(i + r + 1) - at(i - r);
            }
        }
    }
    let (mut a, mut b) = (src.to_vec(), vec![0.0; src.len()]);
    for _ in 0..3 {
        pass(&a, &mut b, w, h, 1, w, radius);
        pass(&b, &mut a, h, w, w, 1, radius);
    }
    a
}

/// Ruido de baja frecuencia: rejilla aleatoria cols x rows estirada sobre el mundo.
struct Noise { cols: usize, rows: usize, v: Vec<f32> }
impl Noise {
    fn new(rng: &mut Rng, cols: usize, rows: usize) -> Self {
        Noise { cols, rows, v: (0..cols * rows).map(|_| rng.f() as f32).collect() }
    }
    fn at(&self, p: P) -> f32 {
        let fx = (p.0 / VIEW_W * self.cols as f32 - 0.5).clamp(0.0, (self.cols - 1) as f32);
        let fy = (p.1 / WORLD_H * self.rows as f32 - 0.5).clamp(0.0, (self.rows - 1) as f32);
        let (x, y) = (fx as usize, fy as usize);
        let (x1, y1) = ((x + 1).min(self.cols - 1), (y + 1).min(self.rows - 1));
        let (u, v) = (smooth(0.0, 1.0, fx.fract()), smooth(0.0, 1.0, fy.fract()));
        let row = |y: usize| self.v[y * self.cols + x] * (1.0 - u) + self.v[y * self.cols + x1] * u;
        row(y) * (1.0 - v) + row(y1) * v
    }
}

// ---------- formas pequenas: tiempo y letras ----------
#[derive(Clone, Copy)]
pub(crate) enum Stroke { Seg(P, P), Circle(P, f32) }

pub(crate) fn distance(s: Stroke, p: P) -> f32 {
    let len = |x: f32, y: f32| (x * x + y * y).sqrt();
    match s {
        Stroke::Seg(a, b) => {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
            len(p.0 - a.0 - dx * t, p.1 - a.1 - dy * t)
        }
        Stroke::Circle(c, r) => (len(p.0 - c.0, p.1 - c.1) - r).abs(),
    }
}

fn coverage(strokes: &[Stroke], half: f32, p: P) -> f32 {
    strokes.iter().fold(0.0f32, |m, &s| m.max((half + 0.5 - distance(s, p)).clamp(0.0, 1.0)))
}

/// Pixeles (dx, dy, alfa) de una forma centrada en el origen; medidas del mundo, escaladas por `k`.
struct Sprite(Vec<(i32, i32, f32)>);

fn sprite(strokes: &[Stroke], half: f32, k: f32) -> Sprite {
    let sc = |p: P| (p.0 * k, p.1 * k);
    let s: Vec<Stroke> = strokes.iter().map(|&s| match s {
        Stroke::Seg(a, b) => Stroke::Seg(sc(a), sc(b)),
        Stroke::Circle(c, r) => Stroke::Circle(sc(c), r * k),
    }).collect();
    let n = (10.0 * k).ceil() as i32;
    let mut px = Vec::new();
    for y in -n..=n {
        for x in -n..=n {
            let a = coverage(&s, half * k, (x as f32, y as f32));
            if a > 0.06 { px.push((x, y, a)); }
        }
    }
    Sprite(px)
}

fn dot(r: f32, k: f32) -> Sprite { sprite(&[Stroke::Seg((0.0, 0.0), (0.0, 0.0))], r, k) }
fn plus(a: f32, k: f32) -> Sprite { sprite(&[Stroke::Seg((-a, 0.0), (a, 0.0)), Stroke::Seg((0.0, -a), (0.0, a))], 0.6, k) }
fn star(a: f32, k: f32) -> Sprite {
    let (x, y) = (a * 0.87, a * 0.5);
    sprite(&[Stroke::Seg((0.0, -a), (0.0, a)), Stroke::Seg((-x, -y), (x, y)), Stroke::Seg((-x, y), (x, -y))], 0.6, k)
}
fn ring(r: f32, k: f32) -> Sprite { sprite(&[Stroke::Circle((0.0, 0.0), r)], 0.6, k) }

/// Trazos de cada caracter de la rampa " .:-=+*#%@" en una celda de 9 x 10.
pub(crate) fn ramp_strokes() -> [Vec<Stroke>; 10] {
    use Stroke::{Circle, Seg};
    let d = |x: f32, y: f32| Seg((x, y), (x, y));
    [
        vec![],
        vec![d(4.5, 7.5)],
        vec![d(4.5, 3.5), d(4.5, 7.5)],
        vec![Seg((2.0, 5.0), (7.0, 5.0))],
        vec![Seg((2.0, 3.8), (7.0, 3.8)), Seg((2.0, 6.2), (7.0, 6.2))],
        vec![Seg((2.0, 5.0), (7.0, 5.0)), Seg((4.5, 2.5), (4.5, 7.5))],
        vec![Seg((4.5, 2.2), (4.5, 6.8)), Seg((2.5, 3.2), (6.5, 5.8)), Seg((2.5, 5.8), (6.5, 3.2))],
        vec![Seg((3.8, 1.5), (2.8, 8.5)), Seg((6.4, 1.5), (5.4, 8.5)), Seg((1.5, 3.7), (7.8, 3.7)), Seg((1.2, 6.4), (7.5, 6.4))],
        vec![Seg((7.2, 1.8), (1.8, 8.2)), Circle((2.8, 3.0), 1.3), Circle((6.2, 7.0), 1.3)],
        vec![Circle((4.5, 5.0), 3.6), Circle((4.8, 5.0), 1.5), Seg((6.3, 3.5), (6.3, 6.5))],
    ]
}

// ---------- la escena ----------
struct Lamp { s: f32, head: P, z: f32, size: f32, color: [f32; 3] }
struct Flake { x: f32, y: f32, vy: f32, sway: f32, rank: f32, kind: usize, v: f32 }
struct Drop { x: f32, land: f32, river: bool, vy: f32, off: f32, rank: f32, v: f32 }
#[derive(Clone, Copy)]
struct Light { glow: f32, ink: [f32; 3], tint: [f32; 3] } // luz que recibe el aguanieve y color con que la refleja

/// Capas quietas de un pixel, comprimidas: u8 para 0..1, u16 para las luces que pasan de 1.
#[derive(Clone, Copy, Default)]
struct Px {
    g: u8,       // piedra, cielo y acera
    l: u16,      // luz de color
    bloom: u16,  // luz que se sale de sus bordes
    glow: u16,   // la parte que viene de las farolas
    mirror: u8,  // puente boca abajo
    streak: u8,  // columna de luz en el agua
    water: u8,   // cuanto rio se ve
    vis: u8,     // cuanto se ve lo que pasa por el puente
    front: u8,   // piedra del puente cercano
    front_a: u8,
    gb: u8,      // lo que hay detras del puente (lo tapa el coche)
    lb: u16,
    posts: u8,
    dot: u8,     // farolillos
    vig: u8,
    tint: [u8; 3],
}

fn q8(v: f32) -> u8 { (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8 }
fn f8(v: u8) -> f32 { v as f32 * (1.0 / 255.0) }
fn q16(v: f32) -> u16 { (v.clamp(0.0, 15.9) * 4096.0 + 0.5) as u16 }
fn f16(v: u16) -> f32 { v as f32 * (1.0 / 4096.0) }

struct Car {
    lights: [P; 2], // faros, en el mundo
    nose: P,
    ahead: P,
    z: f32,
    fade: f32,
    faces: Vec<(Vec<P>, f32, Option<f32>)>, // (poligono, tono, borde), de atras hacia delante
}

/// Un coche oscuro cruza el puente con nieve en el techo.
fn vehicle(t: f64) -> Option<Car> {
    let s = (t % CYCLE) as f32;
    if !(s > 4.9 && s < 5.2 + CROSSING) { return None; }
    let u = ((s - 5.0) / CROSSING).clamp(0.0, 1.0);
    let along_bridge = 1.05 - 1.13 * u.powf(0.86);
    let span = (FAR_END.0 - NEAR_END.0).hypot(FAR_END.1 - NEAR_END.1);
    let point = |along: f32, up: f32, across: f32| on_bridge(along_bridge - along / span, DECK + up, across);
    let (near, far) = (1.0, 3.2);
    let body = [(3.0, 0.35), (3.0, 1.45), (1.8, 1.7), (0.9, 2.6), (-1.6, 2.6), (-2.5, 1.75), (-3.0, 1.65), (-3.0, 0.35)];
    let side = |pts: &[P], across: f32| -> Vec<P> { pts.iter().map(|&(a, h)| point(a, h, across)).collect() };
    let mut faces = vec![(side(&body, far), 18.0, None)];
    for pair in body.windows(2) { // techo y capo
        let ((a0, h0), (a1, h1)) = (pair[0], pair[1]);
        let glass = h1 - h0 > 0.7; // el parabrisas atrapa un poco de luz
        faces.push((vec![point(a0, h0, near), point(a1, h1, near), point(a1, h1, far), point(a0, h0, far)], if glass { 62.0 } else { 38.0 }, Some(78.0)));
    }
    faces.push((side(&body, near), 17.0, Some(165.0)));
    faces.push((side(&[(1.55, 1.75), (0.8, 2.4), (-0.2, 2.4), (-0.2, 1.75)], near), 56.0, Some(120.0)));
    faces.push((side(&[(-0.4, 1.75), (-0.4, 2.4), (-1.5, 2.4), (-2.1, 1.75)], near), 48.0, Some(120.0)));
    faces.push((vec![point(3.0, 0.5, near), point(3.0, 1.45, near), point(3.0, 1.45, far), point(3.0, 0.5, far)], 34.0, Some(105.0)));
    let mut snow = side(&[(0.95, 2.64), (0.4, 2.86), (-0.7, 2.8), (-1.65, 2.64)], near);
    snow.push(point(-1.6, 2.64, far));
    snow.push(point(0.9, 2.64, far));
    faces.push((snow, 205.0, None));
    let lights = [point(3.02, 1.2, near + 0.3), point(3.02, 1.2, far - 0.3)];
    let nose = ((lights[0].0 + lights[1].0) / 2.0, (lights[0].1 + lights[1].1) / 2.0);
    let to = point(9.0, 0.6, 2.1);
    let (dx, dy) = (to.0 - nose.0, to.1 - nose.1);
    let len = dx.hypot(dy) + 1e-6;
    let z = bridge_xz(along_bridge - 3.0 / span, near).1;
    Some(Car { lights, nose, ahead: (dx / len, dy / len), z, fade: smooth(4.9, 5.5, s), faces })
}

pub struct Sleet {
    tints: Vec<[f32; 3]>,
    view: View,
    base: Vec<u32>,
    under: Vec<u32>, // el fotograma sin aguanieve
    px: Vec<Px>,
    cell: (usize, usize),
    atlas: Vec<Vec<f32>>,
    water_cells: Vec<usize>,
    car_cells: Vec<usize>, // las que ocupo el coche en el fotograma anterior
    rows: Vec<(isize, f32)>, // oleaje: desplazamiento y brillo de cada fila
    dash: Vec<u8>,
    car_tone: Vec<f32>,
    car_alpha: Vec<f32>,
    beam: Vec<[f32; 2]>, // (haz, halo) de los faros, en la rejilla gruesa
    bulbs: Vec<f32>, // los faros mismos, pixel a pixel
    car_on: bool,
    stale: f32, // segundos desde que se rehizo el agua
    last: Option<std::time::Instant>, // momento del fotograma anterior
    lamps: Vec<Lamp>,
    breath: Vec<Vec<(usize, f32)>>, // pixeles de cada farola que parpadean, con su peso
    phase: [(f32, f32); LAMPS],
    light: Vec<Light>,
    light_w: usize,
    flakes: [Vec<Flake>; 3],
    drops: Vec<Drop>,
    sprites: Vec<Sprite>,
    t: f64,
    path: f64, // distancia que ha empujado el viento
}

fn rgb_of(c: u32) -> [f32; 3] { [((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0] }
fn pack(c: [f32; 3]) -> u32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    0xFF00_0000 | b(c[0]) << 16 | b(c[1]) << 8 | b(c[2])
}

fn gust(t: f64) -> f32 {
    let s = (t % CYCLE) as f32;
    smooth(9.0, 10.6, s) * (1.0 - smooth(13.8, 15.8, s)) * (0.88 + 0.12 * (s * 5.3).sin())
}

impl Sleet {
    pub fn new(pals: Vec<Pal>) -> Self {
        let tints = pals.iter().map(|p| rgb_of(p.bands[0])).collect();
        let mut rng = Rng(SEED);
        let mut phase = [(0.0, 0.0); LAMPS];
        for p in &mut phase { *p = (rng.rf(0.0, 6.28), rng.rf(2.1, 4.7)); }
        let mut pool = |n: usize, vy: P, bright: P| -> Vec<Flake> {
            (0..n).map(|_| Flake {
                x: rng.rf(0.0, 1380.0), y: rng.rf(0.0, 1.0), vy: rng.rf(vy.0, vy.1), sway: rng.rf(0.0, 6.28),
                rank: rng.rf(0.0, 1.0), kind: rng.range(0, 1) as usize, v: rng.rf(bright.0, bright.1),
            }).collect()
        };
        // tres profundidades de nieve: lejana y tenue, cercana y rapida
        let flakes = [pool(2300, (28.0, 52.0), (0.20, 0.42)), pool(1150, (60.0, 100.0), (0.30, 0.62)), pool(170, (125.0, 195.0), (0.50, 0.90))];
        let stone: Vec<P> = (0..80).map(|i| on_bridge(-0.05 + 2.05 * i as f32 / 79.0, 0.0, 0.0)).collect();
        let (sx, sy): (Vec<f32>, Vec<f32>) = stone.into_iter().unzip();
        let drops = (0..650).map(|_| {
            let x = rng.rf(20.0, VIEW_W - 20.0);
            let near = 1032.0 - 127.0 * x / VIEW_W; // el borde de la acera
            let line = interp(x, &sx, &sy);
            let river = rng.f() < 0.12; // estas encuentran el agua
            let land = if river { rng.rf(line + 6.0, (line + 8.0).max(near - 8.0)) } else { WORLD_H + 40.0 };
            Drop { x, land, river, vy: rng.rf(950.0, 1350.0), off: rng.rf(0.0, 3.0), rank: rng.rf(0.0, 1.0), v: rng.rf(0.22, 0.55) }
        }).collect();
        Sleet { tints, view: View::new(1, 1), base: vec![], lamps: vec![], breath: vec![], phase, light: vec![], light_w: 0, flakes, drops, sprites: vec![], t: 0.0, path: 0.0,
            under: vec![], px: vec![], cell: (9, 10), atlas: vec![], water_cells: vec![], car_cells: vec![], rows: vec![],
            dash: vec![], car_tone: vec![], car_alpha: vec![], beam: vec![], bulbs: vec![], car_on: false, stale: 0.0, last: None }
    }

    fn place_lamps(&mut self) {
        // espaciadas a medio camino entre como estan y como las vemos
        let fine: Vec<f32> = (0..400).map(|i| 0.06 + 1.06 * i as f32 / 399.0).collect();
        let seen: Vec<f32> = fine.iter().map(|&s| on_bridge(s, LAMP_TOP, 0.0).0).collect();
        self.lamps = (0..LAMPS).map(|i| {
            let f = i as f32 / (LAMPS - 1) as f32;
            let by_eye = interp(seen[0] + (seen[399] - seen[0]) * f, &seen, &fine);
            let s = 0.5 * by_eye + 0.5 * (0.06 + 1.06 * f);
            let z = bridge_xz(s, 0.0).1;
            Lamp { s, head: on_bridge(s, LAMP_TOP, 0.0), z, size: 0.45 + 0.55 * (24.0 / z).min(1.0), color: self.tints[i % self.tints.len()] }
        }).collect();
    }

    /// El puente boca abajo y una columna de luz bajo cada farola, sin oleaje (se aplica en cada fotograma).
    fn reflect(&self) -> (Vec<f32>, Vec<f32>, Vec<[f32; 3]>) {
        let v = self.view;
        let mut mirror = v.buf();
        for i in 0..169 {
            let (s0, s1) = (-0.12 + 2.12 * i as f32 / 169.0, -0.12 + 2.12 * (i + 1) as f32 / 169.0);
            for (y0, y1, tone) in [(0.0, DECK, 58.0), (DECK - 0.22, DECK + 0.08, 112.0), (DECK + 0.08, PARAPET, 70.0)] {
                v.poly(&mut mirror, &[on_bridge(s0, -y0, 0.0), on_bridge(s1, -y0, 0.0), on_bridge(s1, -y1, 0.0), on_bridge(s0, -y1, 0.0)], tone / 255.0);
            }
        }
        for k in 0..13 {
            let mid = -0.04 + k as f32 * 0.158;
            let mut pts: Vec<P> = (0..24).map(|i| {
                let u = -1.0 + 2.0 * i as f32 / 23.0;
                on_bridge(mid + u * 0.058, -(0.5 + 2.45 * (1.0f32 - u * u).max(0.0).sqrt()), 0.0)
            }).collect();
            pts.push(on_bridge(mid + 0.058, 0.0, 0.0));
            pts.push(on_bridge(mid - 0.058, 0.0, 0.0));
            v.poly(&mut mirror, &pts, 6.0 / 255.0);
        }
        let mut streak = v.buf();
        let mut color = vec![[0.0f32; 3]; v.w * v.h];
        for lamp in &self.lamps {
            let foot = on_bridge(lamp.s, 0.0, 0.0).1;
            for y in 0..v.h {
                let row = v.world(0, y).1;
                if row <= foot { continue; }
                let fall = (-(row - foot) / (340.0 * lamp.size)).exp() * 0.34;
                for x in 0..v.w {
                    let dx = (v.world(x, y).0 - lamp.head.0) / (17.0 * lamp.size);
                    if dx.abs() > 3.0 { continue; }
                    let i = y * v.w + x;
                    let l = (-dx * dx).exp() * fall;
                    streak[i] += l;
                    for c in 0..3 { color[i][c] += lamp.color[c] * l; }
                }
            }
        }
        (mirror, streak, color)
    }

    /// Calcula todo lo quieto y lo deja en `self.base`.
    fn build(&mut self) {
        let v = self.view;
        let (w, h, n) = (v.w, v.h, v.w * v.h);
        let mut rng = Rng(SEED.rotate_left(17));
        // cielo y rio
        let cloud = Noise::new(&mut rng, 10, 14);
        let streaks = Noise::new(&mut rng, 22, 190);
        let mut g = v.buf();
        for y in 0..h {
            for x in 0..w {
                let p = v.world(x, y);
                g[y * w + x] = if p.1 < HORIZON {
                    let d = (p.1 - HORIZON) / 230.0;
                    BG + 0.075 * (-d * d).exp() + (cloud.at(p) - 0.45) * 0.07 * smooth(HORIZON, 100.0, p.1)
                } else {
                    BG + 0.085 * (-(p.1 - HORIZON) / 210.0).exp() + (streaks.at(p) - 0.5) * 0.07
                };
            }
        }
        // la otra orilla: tejados y unas ventanas todavia despiertas
        let mut x = -20.0;
        while x < VIEW_W {
            let (wide, tall) = (rng.rf(18.0, 70.0), rng.rf(8.0, 46.0));
            let top = HORIZON - tall;
            v.poly(&mut g, &[(x, HORIZON + 5.0), (x, top), (x + wide / 2.0, top - rng.rf(0.0, 16.0)), (x + wide, top), (x + wide, HORIZON + 5.0)], 0.045);
            x += wide - 4.0;
        }
        let mut lum = v.buf(); // luz de color
        for _ in 0..34 {
            let (x, y) = (rng.rf(8.0, VIEW_W - 8.0), HORIZON - rng.rf(3.0, 30.0));
            v.poly(&mut lum, &[(x, y), (x + 3.0, y), (x + 3.0, y + 4.0), (x, y + 4.0)], rng.rf(0.2, 0.55));
        }

        let first = self.tints[0];
        let mut col: Vec<[f32; 3]> = lum.iter().map(|&l| first.map(|c| c * l)).collect(); // color * intensidad
        let (mirror, streak, streak_col) = self.reflect();

        // el puente
        let quad = |s0: f32, s1: f32, y0: f32, y1: f32, across: f32| {
            [on_bridge(s0, y0, across), on_bridge(s1, y0, across), on_bridge(s1, y1, across), on_bridge(s0, y1, across)]
        };
        let steps: Vec<f32> = (0..170).map(|i| -0.12 + 2.12 * i as f32 / 169.0).collect();
        let (mut front, mut mask) = (v.buf(), v.buf());
        let stone = |front: &mut [f32], mask: &mut [f32], pts: [P; 4], tone: f32| {
            v.poly(front, &pts, tone / 255.0);
            v.poly(mask, &pts, 1.0);
        };
        for s in steps.windows(2) { stone(&mut front, &mut mask, quad(s[0], s[1], 0.0, DECK, 0.0), 42.0); }
        // hiladas de piedra, cada bloque un gris distinto
        for row in 0..8 {
            let y = row as f32 * 0.45;
            let mut s = -0.12 + (row % 2) as f32 * 0.008;
            while s < 2.0 {
                let ds = rng.rf(0.012, 0.022);
                stone(&mut front, &mut mask, quad(s + 0.0012, s + ds - 0.0012, y + 0.04, y + 0.41, 0.0), rng.rf(34.0, 62.0));
                s += ds;
            }
        }
        // arcos, y el hueco que se ve a traves de cada uno
        let mut hole = v.buf();
        let (period, half) = (0.158, 0.058);
        for k in 0..13 {
            let mid = -0.04 + k as f32 * period;
            let u: Vec<f32> = (0..36).map(|i| -1.0 + 2.0 * i as f32 / 35.0).collect();
            let rise = |u: f32| (1.0 - u * u).max(0.0).sqrt();
            let arc = |across: f32| -> Vec<P> {
                let mut pts: Vec<P> = u.iter().map(|&u| on_bridge(mid + u * half, 0.5 + 2.45 * rise(u), across)).collect();
                pts.push(on_bridge(mid + half, 0.0, across));
                pts.push(on_bridge(mid - half, 0.0, across));
                pts
            };
            let (near, far) = (arc(0.0), arc(WIDTH));
            let outer: Vec<P> = u.iter().map(|&u| on_bridge(mid + u * half * 1.14, 0.55 + 2.85 * rise(u), 0.0)).collect();
            for i in 0..u.len() - 1 { // dovelas, una a una
                v.poly(&mut front, &[near[i], outer[i], outer[i + 1], near[i + 1]], if i % 2 == 1 { 78.0 } else { 58.0 } / 255.0);
            }
            v.poly(&mut front, &near, 28.0 / 255.0);
            let (mut a, mut b) = (v.buf(), v.buf());
            v.poly(&mut a, &near, 1.0);
            v.poly(&mut b, &far, 1.0);
            for i in 0..n { hole[i] = hole[i].max(a[i].min(b[i])); }
        }
        // cornisa y pretil
        for s in steps.windows(2) {
            stone(&mut front, &mut mask, quad(s[0], s[1], DECK - 0.22, DECK + 0.08, 0.0), 82.0);
            stone(&mut front, &mut mask, quad(s[0], s[1], DECK + 0.08, PARAPET - 0.14, 0.0), 52.0);
            stone(&mut front, &mut mask, quad(s[0], s[1], PARAPET - 0.14, PARAPET, 0.0), 100.0);
        }
        let mut s = -0.12;
        while s < 2.0 {
            stone(&mut front, &mut mask, quad(s, s + 0.0025, DECK + 0.08, PARAPET - 0.14, 0.0), 30.0);
            s += 0.021;
        }
        let front_a: Vec<f32> = (0..n).map(|i| mask[i] * (1.0 - hole[i])).collect();
        // el pretil del fondo, visto por encima del cercano
        let mut back = v.buf();
        for s in steps.windows(2) {
            v.poly(&mut back, &quad(s[0], s[1], DECK, PARAPET, WIDTH), 34.0 / 255.0);
            v.poly(&mut back, &quad(s[0], s[1], PARAPET - 0.12, PARAPET, WIDTH), 78.0 / 255.0);
        }

        // la acera donde estamos de pie
        let lip = |x: f32| 1032.0 + (905.0 - 1032.0) * x / VIEW_W;
        let mut walk = vec![24.0 / 255.0; n];
        for i in 0..7 {
            let y = 905.0 + 34.0 * i as f32;
            v.line(&mut walk, (0.0, y + 66.0), (VIEW_W, y - 61.0), 2.0, 52.0 / 255.0);
        }
        for i in 0..28 {
            let x = -14.0 + 1.1 * i as f32;
            v.line(&mut walk, project(x, 2.0, 20.0), project(x * 0.8 - 1.5, 2.0, 5.5), 2.0, 46.0 / 255.0);
        }
        v.line(&mut walk, (0.0, lip(0.0)), (VIEW_W, lip(VIEW_W)), 7.0, 120.0 / 255.0);
        v.line(&mut walk, (0.0, lip(0.0) + 13.0), (VIEW_W, lip(VIEW_W) + 13.0), 3.0, 60.0 / 255.0);
        let mut walk_a = v.buf();
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let p = v.world(x, y);
                walk_a[i] = ((p.1 - lip(p.0)) * v.k).clamp(0.0, 1.0);
                if rng.f() > 0.985 { walk[i] += 0.18; }
            }
        }

        // de atras hacia delante
        let (mut gb, mut lb) = (vec![], vec![]);
        for (layer, (tone, alpha)) in [(&back, None), (&front, Some(&front_a)), (&walk, Some(&walk_a))].into_iter().enumerate() {
            for i in 0..n {
                let a = alpha.map_or(if tone[i] > 0.0 { 1.0 } else { 0.0 }, |a| a[i]);
                g[i] = g[i] * (1.0 - a) + tone[i] * a;
                lum[i] *= 1.0 - a;
                col[i] = col[i].map(|c| c * (1.0 - a));
            }
            if layer == 0 { (gb, lb) = (g.clone(), lum.clone()); } // lo que el coche puede tapar
        }

        // la luz de las farolas: halo y mancha sobre la piedra
        let mut glow = v.buf();
        for lamp in &self.lamps {
            let base = on_bridge(lamp.s, PARAPET, 0.0);
            let s = lamp.size;
            let gauss = |r2: f32, sigma: f32| (-r2 / (2.0 * sigma * sigma)).exp();
            for y in 0..h {
                for x in 0..w {
                    let i = y * w + x;
                    let p = v.world(x, y);
                    let r2 = (p.0 - lamp.head.0).powi(2) + (p.1 - lamp.head.1).powi(2);
                    let mut l = gauss(r2, 7.0 * s) * 1.15 + gauss(r2, 24.0 * s) * 0.24 + gauss(r2, 92.0 * s) * 0.075;
                    if front_a[i] > 0.0 {
                        let d = ((p.0 - base.0) / (130.0 * s)).powi(2) + ((p.1 - base.1 - 55.0 * s) / (95.0 * s)).powi(2);
                        l += (-d).exp() * front[i] * 1.15 * front_a[i];
                    }
                    l *= 0.93;
                    glow[i] += l;
                    for c in 0..3 { col[i][c] += lamp.color[c] * l; }
                }
            }
        }
        for i in 0..n { lum[i] += glow[i]; }
        let tint: Vec<[f32; 3]> = (0..n).map(|i| {
            let total = lum[i] + streak[i];
            if total > 1e-4 { std::array::from_fn(|c| (col[i][c] + streak_col[i][c]) / total) } else { first }
        }).collect();
        drop((col, streak_col));
        // luz derramada sobre la acera
        let clipped: Vec<f32> = glow.iter().map(|&x| x.min(1.0)).collect();
        let spill = blur(&clipped, w, h, ((90.0 * v.k) as usize).max(1));
        for i in 0..n { lum[i] += walk[i] * walk_a[i] * spill[i] * 1.8; }

        // postes de hierro y farolillos, que van sobre las letras
        let mut posts = v.buf();
        for lamp in &self.lamps {
            let (hx, hy) = lamp.head;
            let thick = (130.0 / lamp.z).floor().max(2.0);
            let k = 26.0 / lamp.z;
            v.line(&mut posts, on_bridge(lamp.s, PARAPET, 0.0), lamp.head, thick, 1.0);
            v.line(&mut posts, (hx - 9.0 * k, hy + 7.0 * k), (hx + 9.0 * k, hy + 7.0 * k), thick, 1.0);
        }
        let mut dots = v.buf();
        self.breath.clear();
        for lamp in &self.lamps {
            let (hx, hy) = v.screen(lamp.head);
            let r = (120.0 / lamp.z).max(2.0) * v.k;
            let reach = 28.0 * lamp.size * v.k;
            let mut pulse = Vec::new();
            for y in (hy - reach).max(0.0) as usize..((hy + reach).max(0.0) as usize + 1).min(h) {
                for x in (hx - reach).max(0.0) as usize..((hx + reach).max(0.0) as usize + 1).min(w) {
                    let (dx, dy) = (x as f32 + 0.5 - hx, y as f32 + 0.5 - hy);
                    dots[y * w + x] += (r - dx.hypot(dy * 0.85) + 0.5).clamp(0.0, 1.0);
                    let weight = (-(dx * dx + dy * dy) / (2.0 * (reach / 2.5).powi(2))).exp();
                    if weight > 0.02 { pulse.push((y * w + x, weight)); }
                }
            }
            self.breath.push(pulse);
        }

        self.px = (0..n).map(|i| {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            let r = ((x - w as f32 / 2.0) / (w as f32 * 0.62)).powi(2) + ((y - h as f32 / 2.0) / (h as f32 * 0.78)).powi(2);
            let vis = (1.0 - front_a[i]) * (1.0 - walk_a[i]);
            let row = v.world(i % w, i / w).1;
            Px {
                g: q8(g[i]), l: q16(lum[i]), bloom: 0, glow: q16(glow[i]),
                mirror: q8(mirror[i]), streak: q8(streak[i]), water: q8(if row > HORIZON + 6.0 { vis } else { 0.0 }),
                vis: q8(vis), front: q8(front[i]), front_a: q8(front_a[i]), gb: q8(gb[i]), lb: q16(lb[i]),
                posts: q8(posts[i]), dot: q8(dots[i]), vig: q8(1.0 - 0.32 * (r - 0.15).clamp(0.0, 1.0)),
                tint: tint[i].map(q8),
            }
        }).collect();

        // las celdas de caracteres
        let (cw, ch) = (((9.0 * v.k).round() as usize).max(4), ((10.0 * v.k).round() as usize).max(5));
        let (sx, sy) = (cw as f32 / 9.0, ch as f32 / 10.0);
        self.cell = (cw, ch);
        self.atlas = ramp_strokes().iter().map(|strokes| {
            (0..cw * ch).map(|i| {
                // se mide en unidades de celda y se pasa a pixeles para el borde suave
                let p = (((i % cw) as f32 + 0.5) / sx, ((i / cw) as f32 + 0.5) / sy);
                let d = strokes.iter().fold(f32::MAX, |m, &s| m.min(distance(s, p)));
                ((0.55 - d) * sx + 0.5).clamp(0.0, 1.0)
            }).collect()
        }).collect();
        let cells = w.div_ceil(cw) * h.div_ceil(ch);
        self.water_cells = (0..cells).filter(|&c| self.pixels(c).any(|(x, y)| self.px[y * w + x].water > 0)).collect();
        self.dash = vec![0; n];
        self.car_tone = v.buf();
        self.car_alpha = v.buf();
        self.light_w = w.div_ceil(LIGHT_STEP);
        let coarse = self.light_w * h.div_ceil(LIGHT_STEP);
        self.beam = vec![[0.0; 2]; coarse];
        self.bulbs = v.buf();
        self.car_cells.clear();
        self.car_on = false;
        self.rows = self.swell(0.0);

        // la luz se sale de sus bordes: se mide sobre una primera pasada de letras
        let mut scratch = (vec![0.0f32; cw * ch], vec![0.0f32; cw * ch]);
        let mut over = v.buf();
        for c in 0..cells {
            self.ascii_cell(c, false, &mut scratch.0, &mut scratch.1);
            for (j, (x, y)) in self.pixels(c).enumerate() { over[y * w + x] = (scratch.1[j] - 0.42).clamp(0.0, 1.0); }
        }
        let bloom = blur(&over, w, h, ((18.0 * v.k) as usize).max(1));
        for i in 0..n { self.px[i].bloom = q16(bloom[i] * 0.75); }
        let mut base = vec![0; n];
        for c in 0..cells { self.paint_cell(c, false, &mut scratch, &mut base); }
        self.under = base.clone();
        self.base = base;

        // mapa grueso de luz para el aguanieve
        self.light = (0..coarse).map(|j| {
            let i = (j / self.light_w * LIGHT_STEP) * w + j % self.light_w * LIGHT_STEP;
            let share = (glow[i] * 1.6).clamp(0.0, 1.0);
            Light { glow: glow[i].min(1.0), ink: tint[i].map(|c| (1.0 - share) * 0.92 + share * c), tint: tint[i] }
        }).collect();
    }

    /// Pixeles de una celda de caracteres (las del borde pueden quedar recortadas).
    fn pixels(&self, cell: usize) -> impl Iterator<Item = (usize, usize)> {
        let (cw, ch) = self.cell;
        let across = self.view.w.div_ceil(cw);
        let (x0, y0) = (cell % across * cw, cell / across * ch);
        let (x1, y1) = ((x0 + cw).min(self.view.w), (y0 + ch).min(self.view.h));
        (y0..y1).flat_map(move |y| (x0..x1).map(move |x| (x, y)))
    }

    fn swell(&self, t: f64) -> Vec<(isize, f32)> {
        let phase = |speed: f64| ((t * speed) % std::f64::consts::TAU) as f32;
        let (a, b, c) = (phase(2.4), phase(1.2), phase(3.1));
        (0..self.view.h).map(|y| {
            let row = self.view.world(0, y).1;
            let depth = (row - HORIZON).max(0.0);
            let shift = (2.0 + depth * 0.045) * (row * 0.23 - a).sin() + (1.0 + depth * 0.018) * (row * 0.061 + b).sin();
            ((shift * self.view.k) as isize, 0.78 + 0.22 * (row * 0.8 - c).sin())
        }).collect()
    }

    /// (haz, halo) de los faros en un pixel, interpolados de la rejilla gruesa.
    #[inline]
    fn lights_at(&self, x: usize, y: usize) -> [f32; 2] {
        let (w, h) = (self.light_w, self.beam.len() / self.light_w);
        let (x0, y0) = ((x / LIGHT_STEP).min(w - 1), (y / LIGHT_STEP).min(h - 1));
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (u, v) = ((x % LIGHT_STEP) as f32 / LIGHT_STEP as f32, (y % LIGHT_STEP) as f32 / LIGHT_STEP as f32);
        let (a, b, c, d) = (self.beam[y0 * w + x0], self.beam[y0 * w + x1], self.beam[y1 * w + x0], self.beam[y1 * w + x1]);
        std::array::from_fn(|i| {
            let top = a[i] + (b[i] - a[i]) * u;
            top + (c[i] + (d[i] - c[i]) * u - top) * v
        })
    }

    /// Esquina y tamano de una celda de caracteres (las del borde pueden quedar recortadas).
    #[inline]
    fn rect_of(&self, cell: usize) -> (usize, usize, usize, usize) {
        let (cw, ch) = self.cell;
        let across = self.view.w.div_ceil(cw);
        let (x0, y0) = (cell % across * cw, cell / across * ch);
        (x0, y0, cw.min(self.view.w - x0), ch.min(self.view.h - y0))
    }

    /// Compone la luz de una celda y elige su caracter; deja piedra y luz en `g` y `l`.
    /// `car`: el coche o sus luces alcanzan esta celda.
    fn ascii_cell(&self, cell: usize, car: bool, g: &mut [f32], l: &mut [f32]) {
        let w = self.view.w;
        let (x0, y0, width, height) = self.rect_of(cell);
        let mut sum = 0.0;
        for dy in 0..height {
            let y = y0 + dy;
            let start = y * w;
            let row = &self.px[start..start + w];
            let (shift, ripple) = self.rows[y];
            let (mirror_k, streak_k) = (ripple * 0.9 / 255.0, ripple * 0.93 / 255.0);
            let (g, l) = (&mut g[dy * width..(dy + 1) * width], &mut l[dy * width..(dy + 1) * width]);
            for (j, ((p, g), l)) in row[x0..x0 + width].iter().zip(g.iter_mut()).zip(l.iter_mut()).enumerate() {
                let (mut stone, mut light) = (f8(p.g), f16(p.l));
                if p.water > 0 {
                    let from = &row[(x0 + j).saturating_add_signed(shift).min(w - 1)];
                    let water = f8(p.water);
                    stone += from.mirror as f32 * mirror_k * water;
                    light += (from.streak as f32 * streak_k + f8(self.dash[start + x0 + j]) * 1.15) * water;
                }
                (*g, *l) = (stone, light);
            }
            if car {
                for (j, (g, l)) in g.iter_mut().zip(l.iter_mut()).enumerate() {
                    let (x, i) = (x0 + j, start + x0 + j);
                    let p = &row[x];
                    let (a, front_a) = (self.car_alpha[i], f8(p.front_a));
                    if a > 0.0 { // el coche se interpone entre nosotros y la luz
                        let hidden = a * f8(p.vis);
                        *g += hidden * (self.car_tone[i] - f8(p.gb));
                        *l = (*l - hidden * f16(p.lb) - f16(p.glow) * 0.8 * a * (1.0 - front_a)).max(0.0);
                    }
                    let [beam, flare] = self.lights_at(x, y);
                    let shine = beam * (0.3 + f8(p.front) * front_a * 1.1) * (1.0 - 0.9 * a * (1.0 - front_a));
                    *l += shine + (self.bulbs[i] + flare).min(1.3);
                }
            }
            for (g, l) in g.iter().zip(l.iter()) { sum += (g + l).min(1.0); }
        }
        let n = width * height;
        let level = (sum / n as f32 * 1.22 - 0.025).clamp(0.0, 1.0).powf(0.86);
        let glyph = &self.atlas[(level * 9.0).round() as usize];
        let tone = (level * 1.25 + 0.12).min(1.0) * 0.74;
        let cw = self.cell.0;
        for dy in 0..height {
            let span = dy * width..(dy + 1) * width;
            for ((g, l), ink) in g[span.clone()].iter_mut().zip(l[span].iter_mut()).zip(&glyph[dy * cw..dy * cw + width]) {
                let letter = ink * tone;
                let share = *l / (*g + *l + 1e-5);
                *g = *g * 0.55 + letter * (1.0 - share);
                *l = *l * 0.55 + letter * share;
            }
        }
    }

    /// Letras, postes, farolillos, color y vineta de una celda, ya en pixeles.
    fn paint_cell(&self, cell: usize, car: bool, (g, l): &mut (Vec<f32>, Vec<f32>), out: &mut [u32]) {
        self.ascii_cell(cell, car, g, l);
        let w = self.view.w;
        let (x0, y0, width, height) = self.rect_of(cell);
        for dy in 0..height {
            let start = (y0 + dy) * w + x0;
            let span = dy * width..(dy + 1) * width;
            let cells = g[span.clone()].iter().zip(&l[span]).zip(&self.px[start..start + width]);
            for (out, ((&g, &l), p)) in out[start..start + width].iter_mut().zip(cells) {
                let posts = f8(p.posts);
                let stone = g * (1.0 - posts) + posts * 0.2;
                let light = (l + f16(p.bloom)) * (1.0 - posts * 0.75) + f8(p.dot);
                let vig = p.vig as f32;
                let ch = |tint: u8| ((stone + light * f8(tint)).clamp(BG, 1.0) * vig + 0.5) as u32;
                *out = 0xFF00_0000 | ch(p.tint[0]) << 16 | ch(p.tint[1]) << 8 | ch(p.tint[2]);
            }
        }
    }

    /// Dibuja el coche y sus luces; devuelve las celdas que toca.
    fn drive(&mut self, car: Option<&Car>) -> Vec<usize> {
        let v = self.view;
        if self.car_on { // solo hay algo que limpiar si hubo coche
            self.car_tone.fill(0.0);
            self.car_alpha.fill(0.0);
            self.bulbs.fill(0.0);
            self.beam.fill([0.0; 2]);
        }
        self.car_on = car.is_some();
        let Some(car) = car else { return vec![] };
        for (pts, tone, edge) in &car.faces {
            v.poly(&mut self.car_tone, pts, tone / 255.0);
            v.poly(&mut self.car_alpha, pts, car.fade);
            if let Some(edge) = edge {
                for i in 0..pts.len() { v.line(&mut self.car_tone, pts[i], pts[(i + 1) % pts.len()], 1.0, edge / 255.0); }
            }
        }
        let (cw, ch) = self.cell;
        let across = v.w.div_ceil(cw);
        let mut touched = vec![false; across * v.h.div_ceil(ch)];
        // un disco desenfocado, en forma cerrada: desenfocar cada fotograma seria caro
        let r = (150.0 / car.z).clamp(1.5, 11.0);
        let halo = |sigma: f32, gain: f32| {
            let s = (sigma * sigma + r * r / 4.0).sqrt();
            ((gain * r * r / (2.0 * s * s)).min(1.3) * car.fade, s)
        };
        let (near, wide) = (halo(16.0, 3.5), halo(64.0, 7.0));
        for at in car.lights {
            let ring: Vec<P> = (0..16).map(|i| {
                let a = i as f32 / 16.0 * std::f32::consts::TAU;
                (at.0 + r * a.cos(), at.1 + r * a.sin())
            }).collect();
            v.poly(&mut self.bulbs, &ring, car.fade);
        }
        let nose = v.screen(car.nose);
        let lights = car.lights.map(|at| v.screen(at));
        for j in 0..self.beam.len() {
            let (x, y) = (j % self.light_w * LIGHT_STEP, j / self.light_w * LIGHT_STEP);
            let (dx, dy) = ((x as f32 - nose.0) / v.k, (y as f32 - nose.1) / v.k);
            let along = dx * car.ahead.0 + dy * car.ahead.1;
            if along > 0.0 {
                let side = (dx * car.ahead.1 - dy * car.ahead.0) / (18.0 + 0.34 * along);
                let fall = side * side + along / 420.0;
                if fall < 4.0 { self.beam[j][0] = (-fall).exp() * car.fade; }
            }
            for at in lights {
                let d2 = ((x as f32 - at.0).powi(2) + (y as f32 - at.1).powi(2)) / (v.k * v.k);
                for (amp, sigma) in [near, wide] {
                    if d2 < 9.0 * sigma * sigma { self.beam[j][1] += amp * (-d2 / (2.0 * sigma * sigma)).exp(); }
                }
            }
            if self.beam[j][0] > 0.04 || self.beam[j][1] > 0.02 { touched[y / ch * across + x / cw] = true; }
        }
        for (i, _) in self.car_alpha.iter().enumerate().filter(|(_, &a)| a > 0.0) {
            touched[i / v.w / ch * across + i % v.w / cw] = true;
        }
        (0..touched.len()).filter(|&c| touched[c]).collect()
    }

    /// Los reflejos que tiemblan: rayas cortas bajo cada farola y bajo los faros.
    fn dashes(&mut self, car: Option<&Car>) {
        self.dash.fill(0);
        let v = self.view;
        let time = |speed: f64| ((self.t * speed) % std::f64::consts::TAU) as f32;
        let (slow, wave_t, fast, flick) = (time(2.2), time(3.3), time(5.1), time(7.0));
        let lamps = self.lamps.iter().map(|l| (l.head.0, on_bridge(l.s, 0.0, 0.0).1, 0.93, l.z));
        let lights = car.into_iter().flat_map(|c| c.lights.map(|at| (at.0, HORIZON + FOCAL * EYE / c.z, c.fade * 0.8, c.z)));
        for (i, (x, foot, power, z)) in lamps.chain(lights).enumerate() {
            let i = i as f32;
            let length = (WORLD_H - foot).min(90.0 + 5200.0 / z);
            let mut y = foot + 6.0;
            while y < foot + length {
                let k = (y - foot) / length;
                let xs = x + (y * 0.19 - wave_t + i * 1.7).sin() * (2.0 + 13.0 * k) + 3.0 * (y * 0.53 + fast + i).sin();
                let value = (power * (1.25 - k * 0.7) * (0.76 + 0.24 * (y * 0.9 + flick + i * 3.0).sin())).clamp(0.0, 1.0);
                let wide = (y * 0.37 + slow + i).sin() > 0.1 && k < 0.75;
                for off in [-12.0, 0.0, 12.0] {
                    if off != 0.0 && !wide { continue; }
                    let shape = &self.sprites[if (y * 1.7 + off + i).sin() > 0.0 { 10 } else { 9 }];
                    let (sx, sy) = v.screen((xs + off, y));
                    let value = value * if off == 0.0 { 1.0 } else { 0.6 };
                    for &(dx, dy, a) in &shape.0 {
                        let (px, py) = (sx.round() as i32 + dx, sy.round() as i32 + dy);
                        if px < 0 || py < 0 || px >= v.w as i32 || py >= v.h as i32 { continue; }
                        let at = py as usize * v.w + px as usize;
                        self.dash[at] = self.dash[at].max(q8(value * a));
                    }
                }
                y += 11.0;
            }
        }
    }

    /// Rehace en `under` todo lo que se mueve bajo el aguanieve.
    fn compose(&mut self) {
        let car = vehicle(self.t);
        self.rows = self.swell(self.t);
        self.dashes(car.as_ref());
        let w = self.view.w;
        // donde estuvo el coche vuelve la noche quieta
        for c in std::mem::take(&mut self.car_cells) {
            for (x, y) in self.pixels(c) { self.under[y * w + x] = self.base[y * w + x]; }
        }
        let cells = self.drive(car.as_ref());
        let mut out = std::mem::take(&mut self.under);
        let mut scratch = (vec![0.0f32; self.cell.0 * self.cell.1], vec![0.0f32; self.cell.0 * self.cell.1]);
        // `cells` y `water_cells` van ordenadas
        for &c in &self.water_cells { self.paint_cell(c, cells.binary_search(&c).is_ok(), &mut scratch, &mut out); }
        for &c in &cells {
            if self.water_cells.binary_search(&c).is_err() { self.paint_cell(c, true, &mut scratch, &mut out); }
        }
        self.under = out;
        self.car_cells = cells;
    }

    fn light_at(&self, x: f32, y: f32) -> Light {
        let x = (x.max(0.0) as usize).min(self.view.w - 1) / LIGHT_STEP;
        let y = (y.max(0.0) as usize).min(self.view.h - 1) / LIGHT_STEP;
        self.light[y * self.light_w + x]
    }

    /// Pinta una forma aclarando lo que haya debajo; dos que se cruzan no se suman.
    fn stamp(&self, fb: &mut [u32], sprite: usize, x: f32, y: f32, ink: [f32; 3], value: f32) {
        let (x, y) = (x.round() as i32, y.round() as i32);
        let (w, h) = (self.view.w as i32, self.view.h as i32);
        for &(dx, dy, a) in &self.sprites[sprite].0 {
            let (px, py) = (x + dx, y + dy);
            if px < 0 || py < 0 || px >= w || py >= h { continue; }
            let i = (py * w + px) as usize;
            let (under, cur) = (self.under[i], fb[i]);
            let ch = |c: usize, shift: u32| {
                let lit = (((under >> shift) & 255) + (ink[c] * value * a * 255.0) as u32).min(255);
                lit.max((cur >> shift) & 255) << shift
            };
            fb[i] = 0xFF00_0000 | ch(0, 16) | ch(1, 8) | ch(2, 0);
        }
    }

    /// Posicion en pantalla, luz y brillo de una gota o copo; None si no se ve.
    fn seen(&self, world: P, bright: f32, boost: f32) -> Option<(P, Light, f32)> {
        let (x, y) = self.view.screen(world);
        let m = 10.0 * self.view.k;
        if x < -m || y < -m || x > self.view.w as f32 + m || y > self.view.h as f32 + m { return None; }
        let mut l = self.light_at(x, y);
        let mut beam = 0.0;
        if self.car_on { // el aguanieve se enciende en el haz de los faros
            beam = self.lights_at((x.max(0.0) as usize).min(self.view.w - 1), (y.max(0.0) as usize).min(self.view.h - 1))[0];
            let share = (beam * 2.0).min(1.0);
            l.ink = std::array::from_fn(|c| l.ink[c] + (l.tint[c] - l.ink[c]) * share);
        }
        let v = (bright * (0.8 + boost * (l.glow + beam * 2.2).min(1.0))).clamp(0.0, 1.0);
        (v > 0.04).then_some(((x, y), l, v))
    }

    fn weather(&self, fb: &mut [u32]) {
        let t = self.t;
        let gust = gust(t);
        let amount = 0.64 + 0.36 * gust; // aguanieve: la mitad quiere ser nieve
        let sway_t = ((t * 1.3) % std::f64::consts::TAU) as f32;
        for (layer, (flakes, drift)) in self.flakes.iter().zip([0.45, 0.75, 1.25]).enumerate() {
            for f in flakes {
                let fade = ((amount - f.rank) * 12.0 + 0.5).clamp(0.0, 1.0);
                if fade <= 0.0 { continue; }
                let y = (f.y as f64 * 1140.0 + f.vy as f64 * t).rem_euclid(1140.0) as f32 - 20.0;
                let sway = (6.0 + 5.0 * layer as f32) * (sway_t + f.sway).sin();
                let x = (f.x as f64 - self.path * drift + sway as f64).rem_euclid(1380.0) as f32 - 150.0;
                if let Some(((x, y), l, v)) = self.seen((x, y), f.v * fade, 1.5) {
                    self.stamp(fb, layer * 2 + f.kind, x, y, l.ink, v);
                }
            }
        }
        let amount = 0.13 + 0.55 * gust;
        let slope = 0.22 + 0.38 * gust;
        for d in &self.drops {
            let fade = ((amount - d.rank) * 14.0 + 0.5).clamp(0.0, 1.0);
            if fade <= 0.0 { continue; }
            let period = ((d.land + 30.0) / d.vy) as f64;
            let age = ((t + d.off as f64) % period) as f32; // desde la ultima llegada
            let y = -30.0 + d.vy * age;
            if let Some(((x, y), l, v)) = self.seen((d.x + slope * (d.land - y), y), d.v * fade, 1.4) {
                self.stamp(fb, 6, x, y, l.ink, v);
            }
            // donde una gota toca el rio se abre un circulo
            if d.river && age < 0.42 && fade > 0.15 && t > period {
                let grow = age / 0.42;
                let (x, y) = self.view.screen((d.x, d.land));
                let ink = self.light_at(x, y).ink;
                self.stamp(fb, if grow < 0.5 { 7 } else { 8 }, x, y, ink, ((0.75 - 0.55 * grow) * fade).clamp(0.0, 1.0));
            }
        }
    }
}

impl Scene for Sleet {
    fn resize(&mut self, p: &mut Painter, w: usize, h: usize) {
        p.resize(w, h);
        self.view = View::new(w, h);
        let k = self.view.k;
        // indices: 0-5 copos (dos por capa), 6 gota, 7-8 circulos en el agua
        self.sprites = vec![
            dot(1.1, k), dot(1.3, k), dot(1.9, k), plus(3.0, k), plus(4.6, k), star(4.8, k),
            sprite(&[Stroke::Seg((-3.5, 7.0), (3.5, -7.0))], 0.6, k), ring(3.6, k), ring(5.0, k),
            // 9-10: rayas de los reflejos
            sprite(&[Stroke::Seg((-3.0, 0.0), (3.0, 0.0))], 0.7, k),
            sprite(&[Stroke::Seg((-3.0, -1.8), (3.0, -1.8)), Stroke::Seg((-3.0, 1.8), (3.0, 1.8))], 0.6, k),
        ];
        self.place_lamps();
        self.build();
        unsafe { crate::malloc_trim(0); } // devuelve los buffers temporales
    }

    fn step(&mut self, p: &mut Painter, dt: f32) {
        // los fotogramas pesados llegan tarde: se avanza lo que ha pasado de verdad (sin saltos
        // grandes si el fondo estuvo tapado), nunca menos que el paso nominal
        let now = std::time::Instant::now();
        let real = self.last.replace(now).map_or(0.0, |last| (now - last).as_secs_f32());
        let dt = real.clamp(dt, dt.max(0.25));
        self.t += dt as f64;
        let t = self.t;
        let wind = 22.0 + 6.0 * ((t * 0.7) % std::f64::consts::TAU).sin() as f32 + 250.0 * gust(t);
        self.path += (wind * dt) as f64;
        self.stale += dt;
        // el ultimo `car_on` obliga a una pasada mas, que borra el coche
        let every = if self.car_on || vehicle(t).is_some() { CAR_EVERY } else { WATER_EVERY };
        if self.stale >= every {
            self.stale = 0.0;
            self.compose();
        }
        p.fb.copy_from_slice(&self.under);
        // cada farola respira por su cuenta
        for ((lamp, pixels), &(phase, speed)) in self.lamps.iter().zip(&self.breath).zip(&self.phase) {
            let wobble = 0.12 + 0.88 * ((t * speed as f64) % std::f64::consts::TAU + phase as f64).sin() as f32;
            for &(i, weight) in pixels {
                let c = rgb_of(self.under[i]);
                let d = 0.09 * wobble * weight;
                p.fb[i] = pack([c[0] + d * lamp.color[0], c[1] + d * lamp.color[1], c[2] + d * lamp.color[2]]);
            }
        }
        self.weather(&mut p.fb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_at_any_size() {
        for pals in [vec![crate::make_pal([255, 219, 161])], crate::NEON.iter().map(|(_, c)| crate::make_pal(*c)).collect()] {
            let mut s = Sleet::new(pals);
            let mut p = Painter::plain();
            for &(w, h) in &[(640usize, 360usize), (160, 100), (300, 900), (1200, 300)] {
                s.resize(&mut p, w, h);
                for _ in 0..400 { s.step(&mut p, 1.0 / 30.0); }
                assert_eq!(p.fb.len(), w * h);
                assert!(p.fb != s.base, "el aguanieve debe cambiar la imagen");
            }
        }
    }

    #[test]
    fn car_passes_and_leaves_no_trace() {
        assert!(vehicle(0.0).is_none() && vehicle(7.0).is_some() && vehicle(20.0).is_none());
        let mut s = Sleet::new(vec![crate::make_pal([255, 219, 161])]);
        let mut p = Painter::plain();
        s.resize(&mut p, 640, 360);
        s.t = 7.0;
        s.step(&mut p, 0.1);
        assert!(!s.car_cells.is_empty());
        s.t = 20.0;
        s.step(&mut p, 0.1);
        assert!(s.car_cells.is_empty() && !s.car_on);
        let (cw, ch) = s.cell;
        for i in 0..s.base.len() { // fuera del agua no queda nada del coche
            let cell = i / 640 / ch * 640usize.div_ceil(cw) + i % 640 / cw;
            if s.water_cells.binary_search(&cell).is_err() { assert_eq!(s.under[i], s.base[i]); }
        }
    }

    #[test]
    fn gust_repeats_and_rests() {
        assert_eq!(gust(0.0), 0.0);
        assert!(gust(12.0) > 0.7);
        assert!((gust(12.0 + CYCLE) - gust(12.0)).abs() < 1e-4);
    }
}
