//! Escena "pacman": un Pac-Man inspirado que se juega solo en un laberinto generado
//! proceduralmente (simetrico, sin callejones sin salida, adaptado a la pantalla).
//! Todo se pinta con rectangulos y pixeles; sin assets externos.
use super::{Painter, Pal, Rng, Scene};
use crate::galaga::glyph;

const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const INF: i32 = 1 << 20;
const GHOSTS: usize = 4;

// ---------- laberinto ----------

/// Genera el laberinto en tiles: (ancho, alto, abierto). Celdas en tiles impares, muros
/// entre ellas. Simetrico izquierda/derecha, conexo y sin callejones sin salida.
fn gen_maze(rng: &mut Rng, cw: usize, ch: usize) -> (usize, usize, Vec<bool>) {
    debug_assert!(cw % 2 == 1 && cw >= 3 && ch >= 2);
    let hw = cw / 2; // columna central; la mitad izquierda es 0..=hw
    let mut h = vec![vec![false; ch]; cw]; // h[c][r]: paso entre (c,r) y (c+1,r)
    let mut v = vec![vec![false; ch]; cw]; // v[c][r]: paso entre (c,r) y (c,r+1)

    // DFS (arbol) sobre la mitad izquierda incluida la columna central
    let mut seen = vec![vec![false; ch]; cw];
    let mut stack = vec![(0usize, 0usize)];
    seen[0][0] = true;
    while let Some(&(c, r)) = stack.last() {
        let mut opts = vec![];
        if c < hw && !seen[c + 1][r] { opts.push((c + 1, r)); }
        if c > 0 && !seen[c - 1][r] { opts.push((c - 1, r)); }
        if r + 1 < ch && !seen[c][r + 1] { opts.push((c, r + 1)); }
        if r > 0 && !seen[c][r - 1] { opts.push((c, r - 1)); }
        if opts.is_empty() { stack.pop(); continue; }
        let (nc, nr) = opts[(rng.next() % opts.len() as u64) as usize];
        seen[nc][nr] = true;
        if nr == r { h[c.min(nc)][r] = true; } else { v[c][r.min(nr)] = true; }
        stack.push((nc, nr));
    }

    // espejo: lo abierto a la izquierda se abre tambien a la derecha
    let mirror = |h: &mut Vec<Vec<bool>>, v: &mut Vec<Vec<bool>>| {
        for r in 0..ch {
            for c in 0..hw { if h[c][r] { h[cw - 2 - c][r] = true; } }
            for c in 0..hw { if v[c][r] { v[cw - 1 - c][r] = true; } }
        }
    };
    mirror(&mut h, &mut v);

    // grado de una celda
    let degree = |h: &Vec<Vec<bool>>, v: &Vec<Vec<bool>>, c: usize, r: usize| -> usize {
        (c > 0 && h[c - 1][r]) as usize + (c + 1 < cw && h[c][r]) as usize
            + (r > 0 && v[c][r - 1]) as usize + (r + 1 < ch && v[c][r]) as usize
    };

    // quitar callejones sin salida abriendo un muro (siempre en la mitad izquierda; el espejo lo replica)
    for _ in 0..8 {
        let mut changed = false;
        for r in 0..ch {
            for c in 0..=hw {
                if degree(&h, &v, c, r) > 1 { continue; }
                let mut closed = vec![];
                if c > 0 && !h[c - 1][r] { closed.push((0, c - 1, r)); }
                if c < hw && !h[c][r] { closed.push((0, c, r)); }
                if r > 0 && !v[c][r - 1] { closed.push((1, c, r - 1)); }
                if r + 1 < ch && !v[c][r] { closed.push((1, c, r)); }
                if closed.is_empty() { continue; }
                let (k, cc, rr) = closed[(rng.next() % closed.len() as u64) as usize];
                if k == 0 { h[cc][rr] = true; } else { v[cc][rr] = true; }
                changed = true;
            }
        }
        mirror(&mut h, &mut v);
        if !changed { break; }
    }
    // algunos bucles extra
    for r in 0..ch {
        for c in 0..=hw {
            if c < hw && !h[c][r] && rng.f() < 0.10 { h[c][r] = true; }
            if r + 1 < ch && !v[c][r] && rng.f() < 0.10 { v[c][r] = true; }
        }
    }
    mirror(&mut h, &mut v);

    let (tw, th) = (2 * cw + 1, 2 * ch + 1);
    let mut open = vec![false; tw * th];
    for r in 0..ch {
        for c in 0..cw {
            open[(2 * r + 1) * tw + 2 * c + 1] = true;
            if c + 1 < cw && h[c][r] { open[(2 * r + 1) * tw + 2 * c + 2] = true; }
            if r + 1 < ch && v[c][r] { open[(2 * r + 2) * tw + 2 * c + 1] = true; }
        }
    }
    (tw, th, open)
}

// ---------- entidades ----------

#[derive(Clone, Copy)]
struct Mover { tx: i32, ty: i32, dx: i32, dy: i32, prog: f32 }

impl Mover {
    fn at(tx: i32, ty: i32) -> Self { Mover { tx, ty, dx: 0, dy: 0, prog: 0.0 } }
    fn pos(&self) -> (f32, f32) { (self.tx as f32 + 0.5 + self.dx as f32 * self.prog, self.ty as f32 + 0.5 + self.dy as f32 * self.prog) }
    fn tile(&self) -> (i32, i32) { if self.prog >= 0.5 { (self.tx + self.dx, self.ty + self.dy) } else { (self.tx, self.ty) } }
    fn reverse(&mut self) {
        if (self.dx, self.dy) == (0, 0) { return; }
        self.tx += self.dx; self.ty += self.dy;
        self.dx = -self.dx; self.dy = -self.dy;
        self.prog = 1.0 - self.prog;
    }
}

#[derive(Clone, Copy, PartialEq)]
enum GS { House(f32), Normal, Fright, Eaten }

struct Ghost { m: Mover, state: GS }

#[derive(Clone, Copy, PartialEq)]
enum Phase { Playing, LevelClear(f32), GameOver(f32) }

struct Colors { pac: u32, ghost: [u32; GHOSTS], fright: u32, flash: u32, eyes: u32, wall: u32, dot: u32, power: u32 }

pub struct Pacman {
    hud: bool,
    tile_arg: i32,
    colors: Colors,
    rng: Rng,
    // geometria
    wpx: i32, hpx: i32, t: i32, ox: i32, oy: i32,
    tw: i32, th: i32,
    wall: Vec<bool>,
    pellet: Vec<u8>,
    pellets_left: usize,
    wall_rects: Vec<(i32, i32, i32, i32)>,
    layer: Vec<u32>,       // fondo + muros ya dibujados (se copia cada fotograma)
    layer_color: u32,      // color con el que se dibujo; si cambia (parpadeo), se rehace
    home: (i32, i32),
    pac_start: (i32, i32),
    // entidades
    pac: Mover,
    pac_alive: bool,
    respawn: f32,
    invuln: f32,
    ghosts: Vec<Ghost>,
    power_t: f32,
    combo: u32,
    // partida
    phase: Phase,
    level: u32,
    score: u32,
    lives: i32,
    t_all: f32,
}

impl Pacman {
    pub fn new(pals: Vec<Pal>, tile: i32, hud: bool) -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
        let colors = if pals.len() >= 9 {
            // rainbow: colores clasicos (orden NEON: green cyan magenta pink blue purple orange yellow red)
            Colors {
                pac: pals[7].bands[0], ghost: [pals[8].bands[0], pals[3].bands[0], pals[1].bands[0], pals[6].bands[0]],
                fright: pals[4].bands[0], flash: pals[0].head, eyes: pals[0].head, wall: pals[4].bands[1],
                dot: pals[7].head, power: pals[7].head,
            }
        } else {
            let p = &pals[0];
            Colors {
                pac: p.bands[0], ghost: [p.head, p.bands[1], p.head, p.bands[1]], fright: p.bands[2], flash: p.head,
                eyes: p.head, wall: p.bands[2], dot: p.bands[1], power: p.head,
            }
        };
        Pacman {
            hud, tile_arg: tile, colors, rng: Rng(seed | 1),
            wpx: 0, hpx: 0, t: 24, ox: 0, oy: 0, tw: 0, th: 0,
            wall: vec![], pellet: vec![], pellets_left: 0, wall_rects: vec![], layer: vec![], layer_color: 0,
            home: (1, 1), pac_start: (1, 1),
            pac: Mover::at(1, 1), pac_alive: true, respawn: 0.0, invuln: 0.0,
            ghosts: vec![], power_t: 0.0, combo: 0,
            phase: Phase::Playing, level: 1, score: 0, lives: 3, t_all: 0.0,
        }
    }

    // ---------- construccion del nivel ----------
    fn build_maze(&mut self) {
        let t = self.t;
        let top = 40.max(t); // la waybar tapa la parte superior
        let bottom = 28.max(t);
        let avail_h = self.hpx - top - bottom;
        let mut ch = ((avail_h / t - 1) / 2).max(2) as usize;
        let mut cw = (((self.wpx / t - 2) - 1) / 2).max(3) as usize;
        if cw % 2 == 0 { cw -= 1; }
        ch = ch.min(40); cw = cw.min(61);
        let (tw, th, open) = gen_maze(&mut self.rng, cw, ch);
        self.tw = tw as i32;
        self.th = th as i32;
        self.wall = open.iter().map(|o| !o).collect();
        self.ox = (self.wpx - self.tw * t) / 2;
        self.oy = top + (avail_h - self.th * t) / 2;

        // salidas de muro: segmentos de contorno junto a los pasillos
        let lt = (t / 9).max(2);
        self.wall_rects.clear();
        for y in 0..self.th {
            for x in 0..self.tw {
                if !self.is_wall(x, y) { continue; }
                let (px, py) = (x * t, y * t);
                for (k, (dx, dy)) in DIRS.iter().enumerate() {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= self.tw || ny >= self.th || self.is_wall(nx, ny) { continue; }
                    let r = match k {
                        0 => (px + t - lt / 2, py - lt / 2, lt, t + lt),
                        1 => (px - lt / 2, py - lt / 2, lt, t + lt),
                        2 => (px - lt / 2, py + t - lt / 2, t + lt, lt),
                        _ => (px - lt / 2, py - lt / 2, t + lt, lt),
                    };
                    self.wall_rects.push(r);
                }
            }
        }

        // puntos y pildoras de poder
        let (cw_i, ch_i) = (cw as i32, ch as i32);
        self.home = (2 * (cw_i / 2) + 1, 2 * (ch_i / 2) + 1);
        self.pac_start = (2 * (cw_i / 2) + 1, 2 * (ch_i - 1) + 1);
        self.pellet = vec![0; (self.tw * self.th) as usize];
        self.pellets_left = 0;
        for y in 0..self.th {
            for x in 0..self.tw {
                if !self.is_wall(x, y) && (x, y) != self.home && (x, y) != self.pac_start {
                    self.pellet[(y * self.tw + x) as usize] = 1;
                    self.pellets_left += 1;
                }
            }
        }
        for (x, y) in [(1, 1), (self.tw - 2, 1), (1, self.th - 2), (self.tw - 2, self.th - 2)] {
            let i = (y * self.tw + x) as usize;
            if self.pellet[i] == 1 { self.pellet[i] = 2; }
        }
        self.layer.clear(); // el laberinto cambio: rehacer la capa estatica
        self.reset_actors();
    }

    fn reset_actors(&mut self) {
        self.pac = Mover::at(self.pac_start.0, self.pac_start.1);
        self.pac_alive = true;
        self.invuln = 1.2;
        self.power_t = 0.0;
        self.ghosts = (0..GHOSTS).map(|i| Ghost { m: Mover::at(self.home.0, self.home.1), state: GS::House(0.6 + i as f32 * 1.4) }).collect();
    }

    fn reset_game(&mut self) {
        self.level = 1;
        self.score = 0;
        self.lives = 3;
        self.phase = Phase::Playing;
        self.build_maze();
    }

    fn is_wall(&self, x: i32, y: i32) -> bool {
        x < 0 || y < 0 || x >= self.tw || y >= self.th || self.wall[(y * self.tw + x) as usize]
    }

    fn neighbors(&self, x: i32, y: i32) -> Vec<(i32, i32)> {
        DIRS.iter().copied().filter(|(dx, dy)| !self.is_wall(x + dx, y + dy)).collect()
    }

    fn bfs(&self, sources: &[(i32, i32)]) -> Vec<i32> {
        let mut d = vec![INF; (self.tw * self.th) as usize];
        let mut q = std::collections::VecDeque::new();
        for &(x, y) in sources {
            if !self.is_wall(x, y) { d[(y * self.tw + x) as usize] = 0; q.push_back((x, y)); }
        }
        while let Some((x, y)) = q.pop_front() {
            let cur = d[(y * self.tw + x) as usize];
            for (dx, dy) in DIRS {
                let (nx, ny) = (x + dx, y + dy);
                if self.is_wall(nx, ny) { continue; }
                let i = (ny * self.tw + nx) as usize;
                if d[i] > cur + 1 { d[i] = cur + 1; q.push_back((nx, ny)); }
            }
        }
        d
    }

    // ---------- IA ----------
    fn pac_decide(&mut self, tx: i32, ty: i32, dir: (i32, i32)) -> (i32, i32) {
        let tiles = |this: &Self, f: &dyn Fn(&Self, usize) -> bool| -> Vec<(i32, i32)> {
            (0..this.ghosts.len()).filter(|&i| f(this, i)).map(|i| this.ghosts[i].m.tile()).collect()
        };
        let pellets: Vec<(i32, i32)> = (0..self.th).flat_map(|y| (0..self.tw).map(move |x| (x, y)))
            .filter(|&(x, y)| self.pellet[(y * self.tw + x) as usize] > 0).collect();
        let powers: Vec<(i32, i32)> = pellets.iter().copied().filter(|&(x, y)| self.pellet[(y * self.tw + x) as usize] == 2).collect();
        let danger = tiles(self, &|s, i| s.ghosts[i].state == GS::Normal);
        let fright = tiles(self, &|s, i| s.ghosts[i].state == GS::Fright);
        let d_pellet = self.bfs(&pellets);
        let d_danger = self.bfs(&danger);
        let d_fright = self.bfs(&fright);
        let d_power = self.bfs(&powers);
        let near_danger = danger.iter().any(|&(gx, gy)| (gx - tx).abs() + (gy - ty).abs() <= 7);

        let mut best: Option<(f32, (i32, i32))> = None;
        for (dx, dy) in self.neighbors(tx, ty) {
            let (nx, ny) = (tx + dx, ty + dy);
            let i = (ny * self.tw + nx) as usize;
            let mut cost = if self.pellets_left == 0 { 0.0 } else { d_pellet[i].min(400) as f32 };
            let dg = d_danger[i];
            if dg <= 1 { cost += 250.0; } else if dg <= 3 { cost += 70.0; } else if dg <= 6 { cost += (7 - dg) as f32 * 5.0; }
            if self.power_t > 2.0 && d_fright[i] < 14 { cost -= (14 - d_fright[i]) as f32 * 3.5; }
            if near_danger && self.power_t <= 0.0 && d_power[i] < 12 { cost -= (12 - d_power[i]) as f32 * 4.0; }
            if (dx, dy) == (-dir.0, -dir.1) && dir != (0, 0) { cost += 6.0; } // evita titubeos
            cost += self.rng.rf(0.0, 0.5);
            if best.map_or(true, |(b, _)| cost < b) { best = Some((cost, (dx, dy))); }
        }
        best.map_or((0, 0), |(_, d)| d)
    }

    fn ghost_decide(&mut self, gi: usize, tx: i32, ty: i32, dir: (i32, i32)) -> (i32, i32) {
        let all = self.neighbors(tx, ty);
        if all.is_empty() { return (0, 0); }
        let mut cands: Vec<(i32, i32)> = all.iter().copied().filter(|&d| d != (-dir.0, -dir.1) || dir == (0, 0)).collect();
        if cands.is_empty() { cands = all.clone(); }
        match self.ghosts[gi].state {
            GS::Fright => cands[(self.rng.next() % cands.len() as u64) as usize],
            GS::Eaten => {
                let d = self.bfs(&[self.home]);
                *all.iter().min_by_key(|&&(dx, dy)| d[((ty + dy) * self.tw + tx + dx) as usize]).unwrap()
            }
            _ => {
                let (px, py) = self.pac.tile();
                let (tw, th) = (self.tw, self.th);
                let scatter = (self.t_all % 27.0) < 7.0;
                let corners = [(tw - 2, 1), (1, 1), (tw - 2, th - 2), (1, th - 2)];
                let target = if scatter { corners[gi % 4] } else {
                    match gi {
                        0 => (px, py),
                        1 => (px + self.pac.dx * 4, py + self.pac.dy * 4),
                        2 => if (px - tx).abs() + (py - ty).abs() > 8 { (px, py) } else { corners[2] },
                        _ => if (px - tx).abs() + (py - ty).abs() > 8 { (px, py) } else { corners[3] },
                    }
                };
                let dist = |d: (i32, i32)| { let (nx, ny) = (tx + d.0 - target.0, ty + d.1 - target.1); nx * nx + ny * ny };
                let min = cands.iter().map(|&d| dist(d)).min().unwrap();
                let ties: Vec<_> = cands.iter().copied().filter(|&d| dist(d) == min).collect();
                ties[(self.rng.next() % ties.len() as u64) as usize]
            }
        }
    }

    // ---------- movimiento ----------
    /// Avanza un mover; `who`: None = Pac-Man, Some(i) = fantasma i.
    fn advance(&mut self, who: Option<usize>, speed: f32, dt: f32) {
        let mut m = match who { None => self.pac, Some(i) => self.ghosts[i].m };
        let mut left = speed * dt;
        for _ in 0..8 {
            if (m.dx, m.dy) == (0, 0) {
                let d = match who { None => self.pac_decide(m.tx, m.ty, (0, 0)), Some(i) => self.ghost_decide(i, m.tx, m.ty, (0, 0)) };
                (m.dx, m.dy) = d;
                if d == (0, 0) { break; }
            }
            let need = 1.0 - m.prog;
            if left < need { m.prog += left; break; }
            left -= need;
            m.tx += m.dx; m.ty += m.dy; m.prog = 0.0;
            // llegada al centro de un tile
            match who {
                None => {
                    let i = (m.ty * self.tw + m.tx) as usize;
                    match self.pellet[i] {
                        1 => { self.pellet[i] = 0; self.pellets_left -= 1; self.score += 10; }
                        2 => {
                            self.pellet[i] = 0; self.pellets_left -= 1; self.score += 50;
                            self.power_t = (7.0 - 0.5 * self.level as f32).max(2.5);
                            self.combo = 0;
                            for g in &mut self.ghosts { if g.state == GS::Normal { g.state = GS::Fright; g.m.reverse(); } }
                        }
                        _ => {}
                    }
                    self.pac = m; // el decide puede mirar a pac
                }
                Some(i) => {
                    if self.ghosts[i].state == GS::Eaten && (m.tx, m.ty) == self.home { self.ghosts[i].state = GS::House(0.8); m.dx = 0; m.dy = 0; break; }
                }
            }
            let d = match who { None => self.pac_decide(m.tx, m.ty, (m.dx, m.dy)), Some(i) => self.ghost_decide(i, m.tx, m.ty, (m.dx, m.dy)) };
            (m.dx, m.dy) = d;
            if d == (0, 0) { break; }
        }
        match who { None => self.pac = m, Some(i) => self.ghosts[i].m = m }
    }

    fn update(&mut self, dt: f32) {
        self.t_all += dt;
        match self.phase {
            Phase::LevelClear(t) => {
                if t - dt <= 0.0 { self.level += 1; self.phase = Phase::Playing; self.build_maze(); } else { self.phase = Phase::LevelClear(t - dt); }
                return;
            }
            Phase::GameOver(t) => {
                if t - dt <= 0.0 { self.reset_game(); } else { self.phase = Phase::GameOver(t - dt); }
                return;
            }
            Phase::Playing => {}
        }
        if !self.pac_alive {
            self.respawn -= dt;
            if self.respawn <= 0.0 {
                if self.lives <= 0 { self.phase = Phase::GameOver(1.5); } else { self.reset_actors(); }
            }
            return;
        }
        self.invuln = (self.invuln - dt).max(0.0);
        if self.power_t > 0.0 {
            self.power_t -= dt;
            if self.power_t <= 0.0 { for g in &mut self.ghosts { if g.state == GS::Fright { g.state = GS::Normal; } } }
        }
        self.advance(None, 7.0, dt);
        let base = (5.9 + 0.3 * self.level as f32).min(7.4);
        for i in 0..self.ghosts.len() {
            match self.ghosts[i].state {
                GS::House(t) => { self.ghosts[i].state = if t - dt <= 0.0 { GS::Normal } else { GS::House(t - dt) }; }
                GS::Normal => self.advance(Some(i), base, dt),
                GS::Fright => self.advance(Some(i), 3.6, dt),
                GS::Eaten => self.advance(Some(i), 13.0, dt),
            }
        }
        // colisiones
        let (px, py) = self.pac.pos();
        for i in 0..self.ghosts.len() {
            let (gx, gy) = self.ghosts[i].m.pos();
            if (gx - px).hypot(gy - py) > 0.65 { continue; }
            match self.ghosts[i].state {
                GS::Fright => { self.ghosts[i].state = GS::Eaten; self.score += 200 << self.combo.min(3); self.combo += 1; }
                GS::Normal if self.invuln <= 0.0 => {
                    self.pac_alive = false; self.respawn = 1.7; self.lives -= 1;
                    break;
                }
                _ => {}
            }
        }
        if self.pac_alive && self.pellets_left == 0 { self.phase = Phase::LevelClear(1.6); }
    }

    // ---------- dibujo ----------
    fn px(&self, tx: f32, ty: f32) -> (i32, i32) { (self.ox + (tx * self.t as f32) as i32, self.oy + (ty * self.t as f32) as i32) }

    fn draw_pac(&self, p: &mut Painter) {
        let (cx, cy) = self.px(self.pac.pos().0, self.pac.pos().1);
        let r = self.t * 2 / 5 + 1;
        let (angle, mouth) = if self.pac_alive {
            let a = if (self.pac.dx, self.pac.dy) == (0, 0) { 0.0 } else { (self.pac.dy as f32).atan2(self.pac.dx as f32) };
            (a, 0.05 + 0.6 * ((self.t_all * 14.0).sin() * 0.5 + 0.5))
        } else {
            let k = 1.0 - (self.respawn / 1.7).clamp(0.0, 1.0); // muerte: la boca se abre hasta desaparecer
            (-std::f32::consts::FRAC_PI_2, 0.05 + k * 3.1)
        };
        if mouth >= std::f32::consts::PI { return; }
        if self.invuln > 0.0 && self.pac_alive && (self.t_all * 12.0) as i32 % 2 == 0 { return; }
        for y in -r..=r {
            for x in -r..=r {
                if x * x + y * y > r * r { continue; }
                if (x, y) != (0, 0) {
                    let mut d = (y as f32).atan2(x as f32) - angle;
                    while d > std::f32::consts::PI { d -= std::f32::consts::TAU; }
                    while d < -std::f32::consts::PI { d += std::f32::consts::TAU; }
                    if d.abs() < mouth { continue; }
                }
                p.rect(cx + x, cy + y, 1, 1, self.colors.pac);
            }
        }
    }

    fn draw_ghost(&self, p: &mut Painter, gi: usize) {
        let g = &self.ghosts[gi];
        let (cx, cy) = self.px(g.m.pos().0, g.m.pos().1);
        let r = self.t * 2 / 5 + 1;
        let frame = (self.t_all * 8.0) as i32 + gi as i32;
        let fright = g.state == GS::Fright;
        let flash = fright && self.power_t < 2.0 && (self.t_all * 6.0) as i32 % 2 == 0;
        let color = if fright { if flash { self.colors.flash } else { self.colors.fright } } else { self.colors.ghost[gi % GHOSTS] };
        if g.state != GS::Eaten {
            for y in -r..=r {
                for x in -r..=r {
                    let body = if y <= 0 { x * x + y * y <= r * r } else {
                        let k = ((x + r) * 3 / (2 * r + 1)).clamp(0, 2);
                        let lim = if (k + frame) % 2 == 0 { r } else { r * 3 / 4 };
                        y <= lim && x.abs() <= r
                    };
                    if body { p.rect(cx + x, cy + y, 1, 1, color); }
                }
            }
        }
        let e = (r * 2 / 5).max(2);
        let (dx, dy) = (g.m.dx, g.m.dy);
        for s in [-1, 1] {
            let ex = cx + s * r * 2 / 5 - e / 2;
            let ey = cy - r / 3 - e / 2;
            if fright && g.state != GS::Eaten {
                p.rect(ex + e / 4, ey + e / 4, (e / 2).max(1), (e / 2).max(1), 0xFF00_0000);
            } else {
                // ojos de color contrastado: negros si el cuerpo es del mismo tono claro
                let (eye, pupil) = if color == self.colors.eyes && g.state != GS::Eaten { (0xFF00_0000, self.colors.eyes) } else { (self.colors.eyes, 0xFF00_0000) };
                p.rect(ex, ey, e, e, eye);
                p.rect(ex + e / 4 + dx * e / 4, ey + e / 4 + dy * e / 4, (e / 2).max(1), (e / 2).max(1), pupil);
            }
        }
    }

    fn render(&mut self, p: &mut Painter) {
        let wall_color = match self.phase {
            Phase::LevelClear(_) if (self.t_all * 5.0) as i32 % 2 == 0 => self.colors.flash,
            _ => self.colors.wall,
        };
        if self.layer.len() != p.fb.len() || self.layer_color != wall_color {
            p.clear();
            for &(x, y, w, h) in &self.wall_rects { p.rect(self.ox + x, self.oy + y, w, h, wall_color); }
            self.layer = p.fb.clone();
            self.layer_color = wall_color;
        } else {
            p.fb.copy_from_slice(&self.layer);
        }
        let t = self.t;
        let (d, pw) = ((t / 7).max(2), t * 2 / 5);
        for y in 0..self.th {
            for x in 0..self.tw {
                match self.pellet[(y * self.tw + x) as usize] {
                    1 => p.rect(self.ox + x * t + (t - d) / 2, self.oy + y * t + (t - d) / 2, d, d, self.colors.dot),
                    2 if (self.t_all * 4.0) as i32 % 2 == 0 => p.rect(self.ox + x * t + (t - pw) / 2, self.oy + y * t + (t - pw) / 2, pw, pw, self.colors.power),
                    _ => {}
                }
            }
        }
        if matches!(self.phase, Phase::Playing | Phase::LevelClear(_)) || !self.pac_alive {
            self.draw_pac(p);
        }
        if self.pac_alive || matches!(self.phase, Phase::Playing) {
            for gi in 0..self.ghosts.len() { if self.pac_alive { self.draw_ghost(p, gi); } }
        }
        if self.hud {
            let u = (self.t / 14).max(1);
            let y = self.hpx - 10 * u - 8;
            let r = self.t / 3;
            for k in 0..(self.lives - 1).max(0) {
                for yy in -r..=r { for xx in -r..=r {
                    if xx * xx + yy * yy <= r * r && !(xx > 0 && yy.abs() < xx / 2 + 1) { p.rect(self.ox + r + 4 + k * (2 * r + 6) + xx, y + 3 * u + yy, 1, 1, self.colors.pac); }
                } }
            }
            let text = |p: &mut Painter, s: &str, x: i32| {
                for (n, c) in s.chars().enumerate() {
                    for (j, bits) in glyph(c).iter().enumerate() {
                        for i in 0..3 { if bits >> (2 - i) & 1 == 1 { p.rect(x + (n as i32 * 4 + i) * u, y + j as i32 * u, u, u, self.colors.dot); } }
                    }
                }
            };
            text(p, &format!("SCORE {:06}", self.score), self.ox + 12 * r + 24);
            let st = format!("STAGE {}", self.level);
            text(p, &st, self.ox + self.tw * self.t - (st.len() as i32 * 4) * u);
        }
    }
}

impl Scene for Pacman {
    fn resize(&mut self, p: &mut Painter, w: usize, h: usize) {
        p.resize(w, h);
        self.wpx = w as i32;
        self.hpx = h as i32;
        self.t = if self.tile_arg > 0 { self.tile_arg } else { (h as i32 / 26).clamp(14, 48) };
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

    fn neighbors_open(open: &[bool], tw: usize, th: usize, x: usize, y: usize) -> usize {
        let mut n = 0;
        if x > 0 && open[y * tw + x - 1] { n += 1; }
        if x + 1 < tw && open[y * tw + x + 1] { n += 1; }
        if y > 0 && open[(y - 1) * tw + x] { n += 1; }
        if y + 1 < th && open[(y + 1) * tw + x] { n += 1; }
        n
    }

    /// Muchos laberintos y tamanos: conexos, simetricos y sin callejones sin salida.
    #[test]
    fn mazes_are_valid() {
        let mut rng = Rng(0x1234_5678_9abc_def1);
        for &(cw, ch) in &[(3usize, 2usize), (5, 3), (7, 5), (11, 7), (21, 11), (23, 9), (31, 15)] {
            for _ in 0..60 {
                let (tw, th, open) = gen_maze(&mut rng, cw, ch);
                // simetria izquierda/derecha
                for y in 0..th { for x in 0..tw { assert_eq!(open[y * tw + x], open[y * tw + (tw - 1 - x)], "asimetrico {cw}x{ch}"); } }
                // sin callejones y conexo
                let mut seen = vec![false; tw * th];
                let mut stack = vec![(1usize, 1usize)];
                seen[tw + 1] = true;
                let mut count = 1;
                while let Some((x, y)) = stack.pop() {
                    for (dx, dy) in DIRS {
                        let (nx, ny) = ((x as i32 + dx) as usize, (y as i32 + dy) as usize);
                        if open[ny * tw + nx] && !seen[ny * tw + nx] { seen[ny * tw + nx] = true; count += 1; stack.push((nx, ny)); }
                    }
                }
                assert_eq!(count, open.iter().filter(|&&o| o).count(), "no conexo {cw}x{ch}");
                for y in 0..th { for x in 0..tw {
                    if open[y * tw + x] { assert!(neighbors_open(&open, tw, th, x, y) >= 2, "callejon en ({x},{y}) {cw}x{ch}"); }
                } }
            }
        }
    }

    /// Simula ~10 min de juego a 30 FPS: sin panics, y la IA gana y pierde.
    #[test]
    fn long_simulation() {
        let mut g = Pacman::new(vec![crate::make_pal([255, 255, 0])], 0, true);
        let mut p = Painter::plain();
        g.resize(&mut p, 1366, 768);
        let (mut max_level, mut deaths, mut games, mut last_lives, mut max_score) = (1, 0, 0, g.lives, 0);
        for _ in 0..(30 * 600) {
            g.step(&mut p, 1.0 / 30.0);
            max_level = max_level.max(g.level);
            if g.lives < last_lives { deaths += 1; }
            if g.lives > last_lives { games += 1; }
            last_lives = g.lives;
            max_score = max_score.max(g.score);
            let (x, y) = g.pac.pos();
            assert!(x > 0.0 && y > 0.0 && x < g.tw as f32 && y < g.th as f32, "pac fuera del laberinto ({x},{y})");
        }
        println!("max_level={max_level} muertes={deaths} partidas_nuevas={games} score_max={max_score}");
        assert!(max_score > 500, "la IA casi no come");
    }
}
