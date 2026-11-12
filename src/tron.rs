//! Escena "tron": motos de luz que se pelean solas. Cada una deja una estela de neon y
//! esquiva eligiendo el giro con mas espacio libre alcanzable; gana la ultima en pie.
use super::{Painter, Pal, Rng, Scene};
use crate::snake::text;

struct Bike { x: i32, y: i32, dx: i32, dy: i32, px: i32, py: i32, alive: bool, col: u32, hot: u32, wins: u32 }

pub struct Tron {
    hud: bool,
    t_arg: i32,
    rng: Rng,
    cols: Vec<(u32, u32)>, // (estela, cabeza)
    t: i32, ox: i32, oy: i32, gw: i32, gh: i32, wpx: i32, hpx: i32,
    grid: Vec<u8>,
    bikes: Vec<Bike>,
    acc: f32,
    pause: f32,
    round: u32,
    stamp: Vec<u32>,
    stamp_id: u32,
    t_all: f32,
}

impl Tron {
    pub fn new(pals: Vec<Pal>, tile: i32, hud: bool) -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
        let cols: Vec<(u32, u32)> = if pals.len() >= 9 {
            // cian, naranja, magenta, amarillo (orden NEON: green cyan magenta pink blue purple orange yellow red)
            [1, 6, 2, 7].iter().map(|&i| (pals[i].bands[0], pals[i].head)).collect()
        } else {
            let p = &pals[0];
            vec![(p.bands[0], p.head), (p.head, p.bands[0]), (p.bands[1], p.head)]
        };
        Tron {
            hud, t_arg: tile, rng: Rng(seed | 1), cols, t: 12, ox: 0, oy: 0, gw: 0, gh: 0, wpx: 0, hpx: 0,
            grid: vec![], bikes: vec![], acc: 0.0, pause: 0.0, round: 1, stamp: vec![], stamp_id: 0, t_all: 0.0,
        }
    }

    fn cx(&self, x: i32) -> i32 { self.ox + x * self.t + self.t / 2 }
    fn cy(&self, y: i32) -> i32 { self.oy + y * self.t + self.t / 2 }
    fn free(&self, x: i32, y: i32) -> bool { x >= 0 && y >= 0 && x < self.gw && y < self.gh && self.grid[(y * self.gw + x) as usize] == 0 }

    fn reset_round(&mut self, p: &mut Painter) {
        let hud_h = if self.hud { self.hpx / 12 } else { 0 };
        self.gw = (self.wpx / self.t).max(8);
        self.gh = ((self.hpx - hud_h) / self.t).max(8);
        self.ox = (self.wpx - self.gw * self.t) / 2;
        self.oy = (self.hpx - hud_h - self.gh * self.t) / 2;
        self.grid = vec![0; (self.gw * self.gh) as usize];
        self.stamp = vec![0; self.grid.len()];
        self.stamp_id = 0;
        let wins: Vec<u32> = self.bikes.iter().map(|b| b.wins).collect();
        self.bikes.clear();
        for (i, &(col, hot)) in self.cols.iter().enumerate() {
            loop {
                let (x, y) = (self.rng.range(2, self.gw - 3), self.rng.range(2, self.gh - 3));
                if !self.free(x, y) { continue; }
                // lejos de las otras motos
                if self.bikes.iter().any(|b: &Bike| (b.x - x).abs() + (b.y - y).abs() < 8.min(self.gw / 3)) { continue; }
                let (dx, dy) = [(1, 0), (-1, 0), (0, 1), (0, -1)][self.rng.range(0, 3) as usize];
                self.grid[(y * self.gw + x) as usize] = i as u8 + 1;
                self.bikes.push(Bike { x, y, dx, dy, px: x, py: y, alive: true, col, hot, wins: wins.get(i).copied().unwrap_or(0) });
                break;
            }
        }
        self.acc = 0.0;
        self.pause = 0.0;
        p.clear();
        for i in 0..self.bikes.len() { self.draw_head(p, i); }
    }

    /// Espacio alcanzable (con tope) desde una celda: mide si un giro lleva a un callejon.
    fn area(&mut self, x: i32, y: i32, cap: usize) -> usize {
        self.stamp_id += 1;
        let id = self.stamp_id;
        let mut st = vec![(x, y)];
        self.stamp[(y * self.gw + x) as usize] = id;
        let mut n = 0;
        while let Some((cx, cy)) = st.pop() {
            n += 1;
            if n >= cap { break; }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (cx + dx, cy + dy);
                if self.free(nx, ny) && self.stamp[(ny * self.gw + nx) as usize] != id {
                    self.stamp[(ny * self.gw + nx) as usize] = id;
                    st.push((nx, ny));
                }
            }
        }
        n
    }

    fn decide(&mut self, i: usize) {
        let (x, y, dx, dy) = { let b = &self.bikes[i]; (b.x, b.y, b.dx, b.dy) };
        let mut best = (f32::MIN, dx, dy);
        for (k, &(ndx, ndy)) in [(dx, dy), (dy, -dx), (-dy, dx)].iter().enumerate() {
            let (nx, ny) = (x + ndx, y + ndy);
            if !self.free(nx, ny) { continue; }
            let a = self.area(nx, ny, 700) as f32;
            // linea recta libre por delante: prefiere pasillos largos
            let mut run = 0;
            while run < 12 && self.free(x + ndx * (run + 1), y + ndy * (run + 1)) { run += 1; }
            let mut score = a + run as f32 * 2.0 + self.rng.rf(0.0, 6.0);
            if k == 0 { score += 8.0; }
            if k > 0 && self.rng.f() < 0.04 { score += 40.0; } // giros espontaneos
            if score > best.0 { best = (score, ndx, ndy); }
        }
        self.bikes[i].dx = best.1;
        self.bikes[i].dy = best.2;
    }

    fn draw_head(&self, p: &mut Painter, i: usize) {
        let b = &self.bikes[i];
        let w = (self.t / 3).max(2);
        p.rect(self.cx(b.x) - w, self.cy(b.y) - w, 2 * w, 2 * w, b.hot);
    }

    fn draw_seg(&self, p: &mut Painter, (x0, y0): (i32, i32), (x1, y1): (i32, i32), c: u32) {
        let w = (self.t / 3).max(2);
        let h = (w / 2).max(1);
        let (ax, bx) = (self.cx(x0).min(self.cx(x1)), self.cx(x0).max(self.cx(x1)));
        let (ay, by) = (self.cy(y0).min(self.cy(y1)), self.cy(y0).max(self.cy(y1)));
        p.rect(ax - h, ay - h, bx - ax + 2 * h + 1, by - ay + 2 * h + 1, c);
    }

    fn tick(&mut self, p: &mut Painter) {
        let n = self.bikes.len();
        for i in 0..n { if self.bikes[i].alive { self.decide(i); } }
        let next: Vec<(i32, i32)> = self.bikes.iter().map(|b| (b.x + b.dx, b.y + b.dy)).collect();
        let mut dies = vec![false; n];
        for i in 0..n {
            if !self.bikes[i].alive { continue; }
            if !self.free(next[i].0, next[i].1) { dies[i] = true; }
            for j in 0..n { if j != i && self.bikes[j].alive && next[j] == next[i] { dies[i] = true; } }
        }
        for i in 0..n {
            if !self.bikes[i].alive { continue; }
            let (x, y) = (self.bikes[i].x, self.bikes[i].y);
            // la cabeza vieja pasa a ser estela
            let (px, py) = (self.bikes[i].px, self.bikes[i].py);
            let w = (self.t / 3).max(2);
            p.rect(self.cx(x) - w, self.cy(y) - w, 2 * w, 2 * w, 0xFF00_0000);
            self.draw_seg(p, (px, py), (x, y), self.bikes[i].col);
            if dies[i] {
                self.bikes[i].alive = false;
                // explosion: cuadrado brillante en el punto de choque
                let w = self.t.max(4);
                p.rect(self.cx(x) - w / 2, self.cy(y) - w / 2, w, w, self.bikes[i].hot);
                continue;
            }
            let b = &mut self.bikes[i];
            b.px = x; b.py = y;
            b.x = next[i].0; b.y = next[i].1;
            let (a, seg, col) = ((b.y * self.gw + b.x) as usize, ((b.px, b.py), (b.x, b.y)), b.col);
            self.grid[a] = i as u8 + 1;
            self.draw_seg(p, seg.0, seg.1, col);
            self.draw_head(p, i);
        }
    }

    fn update(&mut self, p: &mut Painter, dt: f32) {
        self.t_all += dt;
        if self.pause > 0.0 {
            self.pause -= dt;
            if self.pause <= 0.0 { self.round += 1; self.reset_round(p); }
            return;
        }
        self.acc += dt * 22.0;
        let mut steps = 0;
        while self.acc >= 1.0 && steps < 8 {
            self.acc -= 1.0;
            steps += 1;
            self.tick(p);
            let alive = self.bikes.iter().filter(|b| b.alive).count();
            if alive <= 1 {
                if let Some(b) = self.bikes.iter_mut().find(|b| b.alive) { b.wins += 1; }
                self.pause = 2.5;
                break;
            }
        }
        if steps == 8 { self.acc = 0.0; }
    }

    fn hud(&self, p: &mut Painter) {
        let u = (self.hpx / 300).max(2);
        let y = self.hpx - 8 * u - 8;
        // fondo del HUD limpio y marcador
        p.rect(0, self.hpx - 10 * u - 12, self.wpx, 10 * u + 12, 0xFF00_0000);
        let mut x = self.ox + 8;
        for (i, b) in self.bikes.iter().enumerate() {
            let s = format!("P{} {:02}", i + 1, b.wins);
            text(p, &s, x, y, u, b.col);
            x += (s.len() as i32 + 2) * 4 * u;
        }
        let r = format!("ROUND {}", self.round);
        text(p, &r, self.ox + self.gw * self.t - (r.len() as i32 * 4) * u - 8, y, u, self.cols[0].1);
    }
}

impl Scene for Tron {
    fn resize(&mut self, p: &mut Painter, w: usize, h: usize) {
        p.resize(w, h);
        self.wpx = w as i32;
        self.hpx = h as i32;
        self.t = if self.t_arg > 0 { self.t_arg } else { (h as i32 / 80).clamp(6, 16) };
        self.bikes.clear();
        self.reset_round(p);
    }
    fn step(&mut self, p: &mut Painter, dt: f32) {
        self.update(p, dt.min(0.1));
        if self.hud { self.hud(p); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_simulation() {
        for pals in [vec![crate::make_pal([0, 255, 0])], crate::NEON.iter().map(|(_, c)| crate::make_pal(*c)).collect()] {
            let mut s = Tron::new(pals, 0, true);
            let mut p = Painter::plain();
            for &(w, h) in &[(640usize, 360usize), (1920, 1080), (160, 100)] {
                s.resize(&mut p, w, h);
                for _ in 0..20000 { s.step(&mut p, 1.0 / 30.0); }
            }
            assert!(s.round > 3);
        }
    }
}
