//! Escena "snake": una serpiente que se juega sola siguiendo un ciclo hamiltoniano del
//! tablero (nunca muere) y tomando atajos seguros hacia la comida. Crece hasta llenar
//! buena parte de la pantalla y reinicia.
use super::{Painter, Pal, Rng, Scene};
use crate::galaga::glyph;
use std::collections::VecDeque;

/// Texto de pixeles (fuente 3x5 de `galaga::glyph`), `u` pixeles por punto.
pub(crate) fn text(p: &mut Painter, s: &str, x: i32, y: i32, u: i32, color: u32) {
    for (n, c) in s.chars().enumerate() {
        for (j, bits) in glyph(c).iter().enumerate() {
            for i in 0..3 {
                if bits >> (2 - i) & 1 == 1 { p.rect(x + (n as i32 * 4 + i) * u, y + j as i32 * u, u, u, color); }
            }
        }
    }
}

/// Ciclo hamiltoniano de un tablero gw x gh (gh par, gw >= 2): lista de celdas en orden.
fn hamiltonian(gw: i32, gh: i32) -> Vec<(i32, i32)> {
    debug_assert!(gh % 2 == 0 && gw >= 2);
    let mut c = Vec::with_capacity((gw * gh) as usize);
    for x in 0..gw { c.push((x, 0)); }
    for y in 1..gh {
        if y % 2 == 1 { for x in (1..gw).rev() { c.push((x, y)); } }
        else { for x in 1..gw { c.push((x, y)); } }
    }
    for y in (1..gh).rev() { c.push((0, y)); }
    c
}

pub struct Snake {
    hud: bool,
    t_arg: i32,
    pals: Vec<Pal>,
    rng: Rng,
    t: i32, ox: i32, oy: i32, gw: i32, gh: i32, wpx: i32, hpx: i32,
    cyc: Vec<(i32, i32)>,
    idx: Vec<u32>,
    body: VecDeque<(i32, i32)>, // frente = cola, fondo = cabeza
    occ: Vec<bool>,
    food: (i32, i32),
    acc: f32,
    pause: f32,
    score: u32,
    games: u32,
    t_all: f32,
}

impl Snake {
    pub fn new(pals: Vec<Pal>, tile: i32, hud: bool) -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
        Snake {
            hud, t_arg: tile, pals, rng: Rng(seed | 1), t: 20, ox: 0, oy: 0, gw: 0, gh: 0, wpx: 0, hpx: 0,
            cyc: vec![], idx: vec![], body: VecDeque::new(), occ: vec![], food: (0, 0),
            acc: 0.0, pause: 0.0, score: 0, games: 1, t_all: 0.0,
        }
    }

    fn n(&self) -> usize { self.cyc.len() }
    fn at(&self, x: i32, y: i32) -> usize { (y * self.gw + x) as usize }

    fn reset_game(&mut self) {
        let hud_h = if self.hud { self.hpx / 12 } else { 0 };
        let mut gw = (self.wpx / self.t).max(4);
        let mut gh = ((self.hpx - hud_h) / self.t).max(4);
        gh -= gh % 2;
        gw -= gw % 2;
        self.gw = gw; self.gh = gh;
        self.ox = (self.wpx - gw * self.t) / 2;
        self.oy = (self.hpx - hud_h - gh * self.t) / 2;
        self.cyc = hamiltonian(gw, gh);
        self.idx = vec![0; self.cyc.len()];
        for (i, &(x, y)) in self.cyc.iter().enumerate() { let a = self.at(x, y); self.idx[a] = i as u32; }
        self.occ = vec![false; self.cyc.len()];
        self.body.clear();
        let start = self.cyc[self.rng.range(0, self.n() as i32 - 1) as usize];
        self.body.push_back(start);
        let a = self.at(start.0, start.1);
        self.occ[a] = true;
        self.score = 0;
        self.acc = 0.0;
        self.pause = 0.0;
        self.place_food();
    }

    fn place_food(&mut self) {
        let free = self.n() - self.body.len();
        if free == 0 { return; }
        let mut k = self.rng.range(0, free as i32 - 1);
        for i in 0..self.n() {
            let (x, y) = self.cyc[i];
            if !self.occ[self.at(x, y)] {
                if k == 0 { self.food = (x, y); return; }
                k -= 1;
            }
        }
    }

    /// Un paso: sigue el ciclo o toma el mayor atajo que no cruce el cuerpo ni se pase de la comida.
    fn tick(&mut self) {
        let n = self.n() as i64;
        let head = *self.body.back().unwrap();
        let tail = *self.body.front().unwrap();
        let h = self.idx[self.at(head.0, head.1)] as i64;
        let mut next = self.cyc[((h + 1) % n) as usize];
        if (self.body.len() as i64) * 2 < n {
            let tdist = if self.body.len() == 1 { n } else { (self.idx[self.at(tail.0, tail.1)] as i64 - h).rem_euclid(n) };
            let fdist = (self.idx[self.at(self.food.0, self.food.1)] as i64 - h).rem_euclid(n);
            let mut best = 1;
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (x, y) = (head.0 + dx, head.1 + dy);
                if x < 0 || y < 0 || x >= self.gw || y >= self.gh || self.occ[self.at(x, y)] { continue; }
                let d = (self.idx[self.at(x, y)] as i64 - h).rem_euclid(n);
                if d > best && d <= fdist && d + 3 <= tdist { best = d; next = (x, y); }
            }
        }
        self.body.push_back(next);
        let a = self.at(next.0, next.1);
        self.occ[a] = true;
        if next == self.food {
            self.score += 1;
            if self.body.len() < self.n() { self.place_food(); }
        } else {
            let t = self.body.pop_front().unwrap();
            let a = self.at(t.0, t.1);
            self.occ[a] = false;
        }
    }

    fn update(&mut self, dt: f32) {
        self.t_all += dt;
        if self.pause > 0.0 {
            self.pause -= dt;
            if self.pause <= 0.0 { self.games += 1; self.reset_game(); }
            return;
        }
        let rate = 30.0 + self.body.len() as f32 * 0.05;
        self.acc += dt * rate;
        let mut steps = 0;
        while self.acc >= 1.0 && steps < 16 {
            self.acc -= 1.0;
            steps += 1;
            self.tick();
            if self.body.len() * 4 >= self.n() * 3 { self.pause = 2.5; break; }
        }
        if steps == 16 { self.acc = 0.0; }
    }

    fn render(&self, p: &mut Painter) {
        p.clear();
        let t = self.t;
        let inset = if t >= 8 { 1 } else { 0 };
        let cell = |p: &mut Painter, x: i32, y: i32, c: u32| p.rect(self.ox + x * t + inset, self.oy + y * t + inset, t - 2 * inset, t - 2 * inset, c);
        let len = self.body.len();
        let blink = self.pause > 0.0 && (self.t_all * 6.0) as i32 % 2 == 0;
        for (i, &(x, y)) in self.body.iter().enumerate() {
            let pal = |k: usize| &self.pals[k % self.pals.len()];
            let c = if i + 1 == len { pal(0).head }
                else if self.pals.len() >= 9 { pal(i / 3).bands[0] }
                else {
                    let f = i * 3 / len.max(1);
                    pal(0).bands[match f { 2 => 0, 1 => 1, _ => 2 }]
                };
            cell(p, x, y, if blink { self.pals[0].head } else { c });
        }
        if self.body.len() < self.n() {
            let pulse = (self.t_all * 5.0).sin() * 0.5 + 0.5;
            let s = (t as f32 * (0.35 + 0.25 * pulse)) as i32;
            let (fx, fy) = self.food;
            let c = if pulse > 0.5 { self.pals[0].head } else { self.pals[0].bands[0] };
            p.rect(self.ox + fx * t + (t - s) / 2, self.oy + fy * t + (t - s) / 2, s, s, c);
        }
        if self.hud {
            let u = (self.hpx / 300).max(2);
            let y = self.hpx - 8 * u - 8;
            let c = self.pals[0].bands[1];
            text(p, &format!("SCORE {:05}", self.score), self.ox + 8, y, u, c);
            let g = format!("GAME {}", self.games);
            text(p, &g, self.ox + self.gw * t - (g.len() as i32 * 4) * u - 8, y, u, c);
        }
    }
}

impl Scene for Snake {
    fn resize(&mut self, p: &mut Painter, w: usize, h: usize) {
        p.resize(w, h);
        self.wpx = w as i32;
        self.hpx = h as i32;
        self.t = if self.t_arg > 0 { self.t_arg } else { (h as i32 / 45).clamp(10, 32) };
        self.reset_game();
    }
    fn step(&mut self, p: &mut Painter, dt: f32) {
        self.update(dt.min(0.1));
        self.render(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_is_valid() {
        for &(w, h) in &[(2, 2), (4, 4), (6, 4), (30, 20), (7, 6)] {
            let c = hamiltonian(w, h);
            assert_eq!(c.len(), (w * h) as usize);
            let mut seen = std::collections::HashSet::new();
            for i in 0..c.len() {
                assert!(seen.insert(c[i]));
                let (a, b) = (c[i], c[(i + 1) % c.len()]);
                assert_eq!((a.0 - b.0).abs() + (a.1 - b.1).abs(), 1, "{w}x{h} salto en {i}");
            }
        }
    }

    #[test]
    fn long_simulation() {
        let mut s = Snake::new(vec![crate::make_pal([0, 255, 0])], 0, true);
        let mut p = Painter::plain();
        for &(w, h) in &[(640usize, 360usize), (1920, 1080), (200, 120)] {
            s.resize(&mut p, w, h);
            for _ in 0..20000 { s.step(&mut p, 1.0 / 30.0); }
            // invariante: el cuerpo es contiguo sin repeticiones
            let cells: std::collections::HashSet<_> = s.body.iter().collect();
            assert_eq!(cells.len(), s.body.len());
            let v: Vec<_> = s.body.iter().collect();
            for k in 1..v.len() { assert_eq!((v[k].0 - v[k - 1].0).abs() + (v[k].1 - v[k - 1].1).abs(), 1); }
        }
        assert!(s.games > 1);
    }
}
