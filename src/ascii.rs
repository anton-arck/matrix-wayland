//! Rejilla de caracteres para las escenas de paisaje: cada celda tiene un glifo, su color y un fondo
//! suave (degradado en vertical con las celdas vecinas). Solo se repintan las celdas que cambian.
use crate::sleet::{distance, ramp_strokes, Stroke};
use super::Painter;

pub type Rgb = [f32; 3];

// indices del atlas: 0..=9 es la rampa " .:-=+*#%@"
pub const DOT: u8 = 1;
pub const DASH: u8 = 3;
pub const EQUALS: u8 = 4;
pub const PLUS: u8 = 5;
pub const STAR: u8 = 6;
pub const AT: u8 = 9;
pub const BAR: u8 = 10; // |
pub const SLASH: u8 = 11; // /
pub const BACKSLASH: u8 = 12; // \
pub const TILDE: u8 = 13; // ~
pub const CARET: u8 = 14; // ^

#[derive(Clone, Copy, Default)]
pub struct Cell { pub glyph: u8, pub fg: Rgb, pub bg: Rgb }

#[derive(Default)]
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    pub cw: usize,
    pub ch: usize,
    pub cells: Vec<Cell>,
    atlas: Vec<Vec<u8>>, // cobertura 0..255
    drawn: Vec<[u8; 10]>, // lo que ya hay en pantalla de cada celda
}

pub fn mix(a: Rgb, b: Rgb, f: f32) -> Rgb {
    let f = f.clamp(0.0, 1.0);
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * f)
}
pub fn scale(c: Rgb, k: f32) -> Rgb { c.map(|v| v * k) }
pub fn add(a: Rgb, b: Rgb, k: f32) -> Rgb { std::array::from_fn(|i| a[i] + b[i] * k) }

/// Caracter de la rampa para una intensidad 0..1.
pub fn ramp(level: f32) -> u8 { (level.clamp(0.0, 1.0) * 9.0).round() as u8 }

/// Pseudoaleatorio 0..1 fijo para un par de enteros.
pub fn hash(x: i32, y: i32) -> f32 {
    let mut n = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1);
    n = (n ^ (n >> 15)).wrapping_mul(0x85eb_ca6b);
    n = (n ^ (n >> 13)).wrapping_mul(0xc2b2_ae35);
    (n ^ (n >> 16)) as f32 / u32::MAX as f32
}

/// Ruido suave 0..1.
pub fn noise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (u, v) = (x - xi, y - yi);
    let (u, v) = (u * u * (3.0 - 2.0 * u), v * v * (3.0 - 2.0 * v));
    let (xi, yi) = (xi as i32, yi as i32);
    let top = hash(xi, yi) + (hash(xi + 1, yi) - hash(xi, yi)) * u;
    let low = hash(xi, yi + 1) + (hash(xi + 1, yi + 1) - hash(xi, yi + 1)) * u;
    top + (low - top) * v
}

impl Grid {
    /// Celdas de 9 x 10 escaladas a la pantalla, como las de sleet.
    pub fn resize(&mut self, p: &mut Painter, w: usize, h: usize) {
        p.resize(w, h);
        let k = (w as f32 / 1080.0).max(h as f32 / 608.0);
        let (cw, ch) = (((9.0 * k).round() as usize).max(4), ((10.0 * k).round() as usize).max(5));
        let (sx, sy) = (cw as f32 / 9.0, ch as f32 / 10.0);
        let mut strokes: Vec<Vec<Stroke>> = ramp_strokes().into();
        let seg = Stroke::Seg;
        strokes.push(vec![seg((4.5, 1.5), (4.5, 8.5))]);
        strokes.push(vec![seg((7.0, 1.5), (2.0, 8.5))]);
        strokes.push(vec![seg((2.0, 1.5), (7.0, 8.5))]);
        strokes.push(vec![seg((1.0, 6.0), (3.0, 4.2)), seg((3.0, 4.2), (6.0, 6.0)), seg((6.0, 6.0), (8.0, 4.2))]);
        strokes.push(vec![seg((1.8, 6.0), (4.5, 2.5)), seg((4.5, 2.5), (7.2, 6.0))]);
        self.atlas = strokes.iter().map(|glyph| {
            (0..cw * ch).map(|i| {
                let at = (((i % cw) as f32 + 0.5) / sx, ((i / cw) as f32 + 0.5) / sy);
                let d = glyph.iter().fold(f32::MAX, |m, &s| m.min(distance(s, at)));
                (((0.55 - d) * sx + 0.5).clamp(0.0, 1.0) * 255.0) as u8
            }).collect()
        }).collect();
        (self.cw, self.ch, self.cols, self.rows) = (cw, ch, w.div_ceil(cw), h.div_ceil(ch));
        self.cells = vec![Cell::default(); self.cols * self.rows];
        self.drawn = vec![[255; 10]; self.cols * self.rows];
    }

    /// Ancho de la pantalla medido en alturas de pantalla.
    pub fn aspect(&self) -> f32 { (self.cols * self.cw) as f32 / (self.rows * self.ch) as f32 }

    /// Centro de una celda; x e y se miden en alturas de pantalla (y va de 0 a 1).
    pub fn center(&self, c: usize, r: usize) -> (f32, f32) {
        let height = (self.rows * self.ch) as f32;
        ((c as f32 + 0.5) * self.cw as f32 / height, (r as f32 + 0.5) / self.rows as f32)
    }

    /// Celda que contiene un punto, si cae dentro.
    pub fn at(&self, x: f32, y: f32) -> Option<usize> {
        let height = (self.rows * self.ch) as f32;
        let (c, r) = ((x * height / self.cw as f32).floor(), (y * self.rows as f32).floor());
        (c >= 0.0 && r >= 0.0 && (c as usize) < self.cols && (r as usize) < self.rows).then(|| r as usize * self.cols + c as usize)
    }

    /// Pinta en el framebuffer las celdas que han cambiado.
    pub fn present(&mut self, p: &mut Painter) {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
        for r in 0..self.rows {
            for c in 0..self.cols {
                let i = r * self.cols + c;
                let cell = self.cells[i];
                // el fondo va de la media con la celda de arriba a la media con la de abajo
                let up = self.cells[if r > 0 { i - self.cols } else { i }].bg;
                let down = self.cells[if r + 1 < self.rows { i + self.cols } else { i }].bg;
                let (top, bottom) = (mix(cell.bg, up, 0.5), mix(cell.bg, down, 0.5));
                let (fg, t, b) = (cell.fg.map(q), top.map(q), bottom.map(q));
                let key = [cell.glyph, fg[0], fg[1], fg[2], t[0], t[1], t[2], b[0], b[1], b[2]];
                if self.drawn[i] == key { continue; }
                self.drawn[i] = key;
                let glyph = &self.atlas[cell.glyph as usize];
                let (x0, y0) = (c * self.cw, r * self.ch);
                let width = self.cw.min(p.w - x0);
                let height = self.ch as i32;
                for dy in 0..self.ch.min(p.h - y0) {
                    let bg: [i32; 3] = std::array::from_fn(|k| t[k] as i32 + (b[k] as i32 - t[k] as i32) * (2 * dy as i32 + 1) / (2 * height));
                    let ink: [i32; 3] = std::array::from_fn(|k| fg[k] as i32 - bg[k]);
                    let flat = 0xFF00_0000 | (bg[0] as u32) << 16 | (bg[1] as u32) << 8 | bg[2] as u32;
                    let start = (y0 + dy) * p.w + x0;
                    for (px, &cover) in p.fb[start..start + width].iter_mut().zip(&glyph[dy * self.cw..dy * self.cw + width]) {
                        *px = if cover == 0 {
                            flat
                        } else {
                            let ch = |k: usize| (bg[k] + ink[k] * cover as i32 / 255) as u32;
                            0xFF00_0000 | ch(0) << 16 | ch(1) << 8 | ch(2)
                        };
                    }
                }
            }
        }
    }
}
