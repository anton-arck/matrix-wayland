//! Escena "galaga": un shoot'em up inspirado en Galaga que se juega solo.
//! Sprites propios (pixel art), sin assets externos. Todo se pinta con rectangulos.
use super::{Pal, Painter, Rng, Scene};

type Spr = &'static [&'static str];

const SHIP: Spr = &[
    "....#....",
    "....#....",
    "...###...",
    "...###...",
    "#.#####.#",
    "#.#####.#",
    "#########",
    "##.###.##",
    "#..#.#..#",
];
const BEE: [Spr; 2] = [
    &["...#...#...", "....#.#....", "...#####...", "..##.#.##..", ".#########.", "##.#####.##", "#..#...#..#", "....#.#...."],
    &["...#...#...", "..#.#.#.#..", "..#######..", ".##.###.##.", "##.#####.##", ".#.#####.#.", "#..#...#..#", "...#...#..."],
];
const MOTH: [Spr; 2] = [
    &["#.........#", "##..###..##", "###.###.###", ".#########.", "..#######..", ".##.###.##.", "##...#...##", "#.........#"],
    &["..#.....#..", ".###.#.###.", "#####.#####", ".#########.", "..#######..", "..##.#.##..", ".#...#...#.", "#.........#"],
];
const BOSS: [Spr; 2] = [
    &["#.#.....#.#", "#.#######.#", "###.###.###", "#####.#####", ".#########.", "..#.###.#..", "..#.....#..", "...##.##..."],
    &[".#.#...#.#.", "#.#######.#", "###.###.###", "#####.#####", ".#########.", "#.#.###.#.#", "..#.....#..", "...##.##..."],
];

pub(crate) fn glyph(c: char) -> [u8; 5] {
    match c {
        '0' | 'O' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 7, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' | 'S' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 1, 1, 1],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        'C' => [7, 4, 4, 4, 7],
        'R' => [6, 5, 6, 5, 5],
        'E' => [7, 4, 7, 4, 7],
        'T' => [7, 2, 2, 2, 2],
        'A' => [2, 5, 7, 5, 5],
        'G' => [7, 4, 5, 5, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'U' => [5, 5, 5, 5, 7],
        'P' => [7, 5, 7, 4, 4],
        'D' => [6, 5, 5, 5, 6],
        'L' => [4, 4, 4, 4, 7],
        'B' => [6, 5, 6, 5, 6],
        _ => [0; 5],
    }
}

const COLS: i32 = 8;
const SPACING_X: f32 = 14.0;
const SPACING_Y: f32 = 11.0;
const TOP_Y: f32 = 22.0;
const KIND_ROWS: [u8; 5] = [2, 1, 1, 0, 0];

#[derive(Clone, Copy)]
enum ES {
    Enter { delay: f32, t: f32, side: f32, y0: f32, cx: f32, cy: f32 },
    Form,
    Dive { t: f32, dir: f32, shots: u8, st: f32 },
    Ret { t: f32, x0: f32, cx: f32 },
}

struct Enemy { kind: u8, col: i32, row: i32, x: f32, y: f32, state: ES, alive: bool }
struct Bul { x: f32, y: f32, vy: f32 }
struct Part { x: f32, y: f32, vx: f32, vy: f32, life: f32 }
struct Star { x: f32, y: f32, speed: f32, layer: u8 }

#[derive(Clone, Copy, PartialEq)]
enum Phase { Playing, WaveClear(f32), GameOver(f32) }

pub struct Galaga {
    w: f32,
    h: f32,
    scale: i32,
    auto_scale: bool,
    hud: bool,
    pals: Vec<Pal>,
    rng: Rng,
    stars: Vec<Star>,
    enemies: Vec<Enemy>,
    pbul: Vec<Bul>,
    ebul: Vec<Bul>,
    parts: Vec<Part>,
    // jugador
    px: f32,
    alive: bool,
    respawn: f32,
    invuln: f32,
    cooldown: f32,
    tx: f32,
    retarget: f32,
    threat_delay: Option<f32>,
    // partida
    phase: Phase,
    stage: u32,
    score: u32,
    lives: i32,
    t: f32,
    form_t: f32,
    dive_timer: f32,
}

fn bez(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), u: f32) -> (f32, f32) {
    let v = 1.0 - u;
    (v * v * p0.0 + 2.0 * v * u * p1.0 + u * u * p2.0, v * v * p0.1 + 2.0 * v * u * p1.1 + u * u * p2.1)
}

impl Galaga {
    pub fn new(pals: Vec<Pal>, scale: i32, hud: bool) -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
        Galaga {
            w: 1.0, h: 1.0, scale: scale.max(1), auto_scale: scale <= 0, hud, pals, rng: Rng(seed | 1),
            stars: vec![], enemies: vec![], pbul: vec![], ebul: vec![], parts: vec![],
            px: 0.0, alive: true, respawn: 0.0, invuln: 0.0, cooldown: 0.0, tx: 0.0, retarget: 0.0, threat_delay: None,
            phase: Phase::Playing, stage: 1, score: 0, lives: 3, t: 0.0, form_t: 0.0, dive_timer: 3.0,
        }
    }

    fn py(&self) -> f32 { self.h - 12.0 }

    fn reset_game(&mut self) {
        self.stage = 1;
        self.score = 0;
        self.lives = 3;
        self.alive = true;
        self.invuln = 0.0;
        self.px = self.w / 2.0;
        self.tx = self.px;
        self.pbul.clear();
        self.ebul.clear();
        self.parts.clear();
        self.phase = Phase::Playing;
        self.spawn_wave();
    }

    fn spawn_wave(&mut self) {
        self.enemies.clear();
        self.ebul.clear();
        self.form_t = 0.0;
        self.dive_timer = 4.0;
        let mut i = 0;
        for row in 0..5 {
            for col in 0..COLS {
                if row == 0 && !(2..=5).contains(&col) { continue; }
                let group = (i / 4) as f32;
                let side = if (i / 4) % 2 == 0 { 1.0 } else { -1.0 };
                let delay = group * 1.0 + (i % 4) as f32 * 0.14;
                let y0 = self.rng.rf(20.0, 50.0);
                let cx = self.w / 2.0 + side * self.rng.rf(10.0, 60.0);
                let cy = self.h * self.rng.rf(0.45, 0.65);
                self.enemies.push(Enemy {
                    kind: KIND_ROWS[row as usize], col, row, x: -30.0, y: -30.0, alive: true,
                    state: ES::Enter { delay, t: 0.0, side, y0, cx, cy },
                });
                i += 1;
            }
        }
    }

    fn slot(&self, col: i32, row: i32) -> (f32, f32) {
        let breath = 1.0 + 0.06 * (self.form_t * 0.9).sin();
        let x = self.w / 2.0 + (col as f32 - (COLS as f32 - 1.0) / 2.0) * SPACING_X * breath + (self.form_t * 0.45).sin() * 10.0;
        (x, TOP_Y + row as f32 * SPACING_Y * breath)
    }

    fn explode(&mut self, x: f32, y: f32, n: usize) {
        for _ in 0..n {
            let a = self.rng.rf(0.0, std::f32::consts::TAU);
            let s = self.rng.rf(15.0, 45.0);
            self.parts.push(Part { x, y, vx: a.cos() * s, vy: a.sin() * s, life: self.rng.rf(0.3, 0.6) });
        }
    }

    fn kill_player(&mut self) {
        let (x, y) = (self.px, self.py());
        self.explode(x, y, 16);
        self.alive = false;
        self.respawn = 1.6;
        self.lives -= 1;
        self.ebul.clear();
        self.threat_delay = None;
        if self.lives <= 0 { self.phase = Phase::GameOver(2.4); }
    }

    // ---------- IA del jugador ----------
    fn player_ai(&mut self, dt: f32) {
        let py = self.py();
        // amenazas: balas enemigas y picados cercanos por encima
        let mut dodge: Option<f32> = None;
        for b in &self.ebul {
            if b.y < py && b.y > py - 48.0 && (b.x - self.px).abs() < 8.5 {
                dodge = Some(if b.x >= self.px { -1.0 } else { 1.0 });
            }
        }
        for e in &self.enemies {
            if e.alive && matches!(e.state, ES::Dive { .. }) && e.y > py - 55.0 && (e.x - self.px).abs() < 15.0 {
                dodge = Some(if e.x >= self.px { -1.0 } else { 1.0 });
            }
        }
        let mut vmax = 80.0;
        match dodge {
            Some(mut dir) => {
                if self.threat_delay.is_none() { self.threat_delay = Some(self.rng.rf(0.05, 0.28)); } // tiempo de reaccion
                let d = self.threat_delay.unwrap() - dt;
                self.threat_delay = Some(d);
                if d <= 0.0 {
                    if (dir < 0.0 && self.px < 16.0) || (dir > 0.0 && self.px > self.w - 16.0) { dir = -dir; }
                    self.tx = self.px + dir * 30.0;
                    vmax = 115.0;
                }
            }
            None => self.threat_delay = None,
        }
        if self.threat_delay.is_none() || dodge.is_none() {
            self.retarget -= dt;
            if self.retarget <= 0.0 {
                self.retarget = self.rng.rf(0.25, 0.6);
                let mut best: Option<(f32, f32)> = None;
                for e in &self.enemies {
                    if !e.alive || matches!(e.state, ES::Enter { .. }) { continue; }
                    let score = (e.x - self.px).abs() + (4 - e.row).max(0) as f32 * 6.0;
                    if best.map_or(true, |(s, _)| score < s) { best = Some((score, e.x)); }
                }
                self.tx = match best {
                    Some((_, x)) => x + self.rng.rf(-2.0, 2.0),
                    None => self.w / 2.0 + (self.t * 0.7).sin() * 30.0,
                };
            }
        }
        let v = ((self.tx - self.px) * 4.0).clamp(-vmax, vmax);
        self.px = (self.px + v * dt).clamp(8.0, self.w - 8.0);

        // disparo: si hay un enemigo casi en la vertical
        self.cooldown -= dt;
        if self.cooldown <= 0.0 && self.pbul.len() < 2 && (dodge.is_none() || self.threat_delay.map_or(true, |d| d > 0.0)) {
            let aligned = self.enemies.iter().any(|e| e.alive && !matches!(e.state, ES::Enter { .. }) && e.y < py && (e.x - self.px).abs() < 3.5);
            if aligned {
                self.pbul.push(Bul { x: self.px, y: py - 7.0, vy: -170.0 });
                self.cooldown = 0.3;
            }
        }
    }

    // ---------- actualizacion ----------
    fn update_enemies(&mut self, dt: f32) {
        let (w, h, stage) = (self.w, self.h, self.stage as f32);
        let (px, py) = (self.px, self.py());
        for i in 0..self.enemies.len() {
            if !self.enemies[i].alive { continue; }
            let (col, row) = (self.enemies[i].col, self.enemies[i].row);
            let slot = self.slot(col, row);
            let st = self.enemies[i].state;
            match st {
                ES::Enter { mut delay, mut t, side, y0, cx, cy } => {
                    if delay > 0.0 {
                        delay -= dt;
                        self.enemies[i].state = ES::Enter { delay, t, side, y0, cx, cy };
                    } else {
                        t += dt / 2.2;
                        let p0 = (if side > 0.0 { w + 6.0 } else { -6.0 }, y0);
                        let u = t.min(1.0);
                        let (x, y) = bez(p0, (cx, cy), slot, u);
                        self.enemies[i].x = x;
                        self.enemies[i].y = y;
                        self.enemies[i].state = if t >= 1.0 { ES::Form } else { ES::Enter { delay, t, side, y0, cx, cy } };
                    }
                }
                ES::Form => { self.enemies[i].x = slot.0; self.enemies[i].y = slot.1; }
                ES::Dive { mut t, dir, mut shots, mut st } => {
                    t += dt;
                    let e = &mut self.enemies[i];
                    e.y += (62.0 + stage * 5.0) * dt;
                    e.x += dir * 38.0 * (t * 2.2).cos() * dt + (px - e.x) * 0.45 * dt;
                    st -= dt;
                    let (ex, ey) = (e.x, e.y);
                    let go_ret = e.y > h + 8.0;
                    if go_ret {
                        let (x0, cx) = (self.rng.rf(20.0, w - 20.0), self.rng.rf(20.0, w - 20.0));
                        self.enemies[i].state = ES::Ret { t: 0.0, x0, cx };
                        continue;
                    }
                    if shots > 0 && st <= 0.0 && ey > h * 0.25 && ey < py - 30.0 {
                        self.ebul.push(Bul { x: ex, y: ey + 5.0, vy: 70.0 + stage * 3.0 });
                        shots -= 1;
                        st = 0.55;
                    }
                    self.enemies[i].state = ES::Dive { t, dir, shots, st };
                }
                ES::Ret { mut t, x0, cx } => {
                    t += dt / 1.7;
                    let (x, y) = bez((x0, -8.0), (cx, h * 0.35), slot, t.min(1.0));
                    self.enemies[i].x = x;
                    self.enemies[i].y = y;
                    self.enemies[i].state = if t >= 1.0 { ES::Form } else { ES::Ret { t, x0, cx } };
                }
            }
        }
    }

    fn schedule_dives(&mut self, dt: f32) {
        self.dive_timer -= dt;
        if self.dive_timer > 0.0 { return; }
        self.dive_timer = ((2.6 - 0.18 * self.stage as f32).max(0.8)) * self.rng.rf(0.7, 1.3);
        let max_divers = (1 + self.stage / 2).min(4) as usize;
        let divers = self.enemies.iter().filter(|e| e.alive && matches!(e.state, ES::Dive { .. } | ES::Ret { .. })).count();
        if divers >= max_divers { return; }
        let cands: Vec<usize> = (0..self.enemies.len()).filter(|&i| self.enemies[i].alive && matches!(self.enemies[i].state, ES::Form)).collect();
        if cands.is_empty() { return; }
        let i = cands[(self.rng.next() % cands.len() as u64) as usize];
        let dir = if self.enemies[i].x < self.w / 2.0 { 1.0 } else { -1.0 };
        self.enemies[i].state = ES::Dive { t: 0.0, dir, shots: 1 + (self.stage > 2) as u8, st: 0.0 };
    }

    fn collisions(&mut self) {
        // balas del jugador contra enemigos
        let mut i = 0;
        while i < self.pbul.len() {
            let (bx, by) = (self.pbul[i].x, self.pbul[i].y);
            let hit = self.enemies.iter().position(|e| e.alive && !matches!(e.state, ES::Enter { delay, .. } if delay > 0.0)
                && (e.x - bx).abs() < 6.0 && (e.y - by).abs() < 5.0);
            if let Some(k) = hit {
                let (ex, ey, kind, diving) = (self.enemies[k].x, self.enemies[k].y, self.enemies[k].kind, !matches!(self.enemies[k].state, ES::Form));
                self.enemies[k].alive = false;
                self.score += [50, 80, 150][kind as usize] * if diving { 2 } else { 1 };
                self.explode(ex, ey, 10);
                self.pbul.swap_remove(i);
            } else { i += 1; }
        }
        // enemigos y balas contra el jugador
        if self.alive && self.invuln <= 0.0 {
            let (px, py) = (self.px, self.py());
            let shot = self.ebul.iter().any(|b| (b.x - px).abs() < 4.5 && (b.y - py).abs() < 5.0);
            let rammed = self.enemies.iter().any(|e| e.alive && !matches!(e.state, ES::Enter { .. } | ES::Form) && (e.x - px).abs() < 8.0 && (e.y - py).abs() < 8.0);
            if shot || rammed { self.kill_player(); }
        }
    }

    fn update(&mut self, dt: f32) {
        self.t += dt;
        for s in &mut self.stars {
            s.y += s.speed * dt;
            if s.y > self.h { s.y -= self.h; s.x = (self.rng.f() as f32) * self.w; }
        }
        for p in &mut self.parts { p.x += p.vx * dt; p.y += p.vy * dt; p.life -= dt; }
        self.parts.retain(|p| p.life > 0.0);
        for b in &mut self.pbul { b.y += b.vy * dt; }
        for b in &mut self.ebul { b.y += b.vy * dt; }
        let h = self.h;
        self.pbul.retain(|b| b.y > -6.0);
        self.ebul.retain(|b| b.y < h + 6.0);

        match self.phase {
            Phase::Playing => {
                self.form_t += dt;
                self.update_enemies(dt);
                self.schedule_dives(dt);
                self.invuln = (self.invuln - dt).max(0.0);
                if self.alive { self.player_ai(dt); } else {
                    self.respawn -= dt;
                    if self.respawn <= 0.0 { self.alive = true; self.px = self.w / 2.0; self.tx = self.px; self.invuln = 1.8; }
                }
                self.collisions();
                if self.phase == Phase::Playing && self.enemies.iter().all(|e| !e.alive) { self.phase = Phase::WaveClear(1.8); }
            }
            Phase::WaveClear(t) => {
                self.form_t += dt;
                if self.alive { self.player_ai(dt); }
                if t - dt <= 0.0 { self.stage += 1; self.phase = Phase::Playing; self.spawn_wave(); } else { self.phase = Phase::WaveClear(t - dt); }
            }
            Phase::GameOver(t) => {
                self.form_t += dt;
                self.update_enemies(dt);
                if t - dt <= 0.0 { self.reset_game(); } else { self.phase = Phase::GameOver(t - dt); }
            }
        }
    }

    // ---------- dibujo ----------
    fn color(&self, kind: u8) -> u32 {
        let pal = &self.pals[(kind as usize * 3) % self.pals.len()];
        match kind { 0 => pal.bands[0], 1 => pal.bands[1], _ => pal.head }
    }

    fn sprite(&self, p: &mut Painter, spr: Spr, cx: f32, cy: f32, color: u32, scale: i32) {
        let (sw, sh) = (spr[0].len() as i32, spr.len() as i32);
        let x0 = ((cx * self.scale as f32) as i32) - sw * scale / 2;
        let y0 = ((cy * self.scale as f32) as i32) - sh * scale / 2;
        for (j, row) in spr.iter().enumerate() {
            let b = row.as_bytes();
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'#' {
                    let s = i;
                    while i < b.len() && b[i] == b'#' { i += 1; }
                    p.rect(x0 + s as i32 * scale, y0 + j as i32 * scale, (i - s) as i32 * scale, scale, color);
                } else { i += 1; }
            }
        }
    }

    fn text(&self, p: &mut Painter, s: &str, x: i32, y: i32, u: i32, color: u32) {
        for (n, c) in s.chars().enumerate() {
            let g = glyph(c);
            for (j, bits) in g.iter().enumerate() {
                for i in 0..3 {
                    if bits >> (2 - i) & 1 == 1 { p.rect(x + (n as i32 * 4 + i) * u, y + j as i32 * u, u, u, color); }
                }
            }
        }
    }

    fn render(&self, p: &mut Painter) {
        let sc = self.scale;
        p.clear();
        let dim = &self.pals[0];
        for (i, s) in self.stars.iter().enumerate() {
            if (self.t * 3.0 + i as f32) as i32 % 7 == 0 { continue; } // parpadeo
            let c = if s.layer == 2 { dim.bands[2] } else { dim.bands[3] };
            let sz = if s.layer == 2 { sc } else { (sc / 2).max(1) };
            p.rect((s.x * sc as f32) as i32, (s.y * sc as f32) as i32, sz, sz, c);
        }
        let frame = (self.t * 3.0) as i32;
        for (i, e) in self.enemies.iter().enumerate() {
            if !e.alive || matches!(e.state, ES::Enter { delay, .. } if delay > 0.0) { continue; }
            let f = ((frame + i as i32) % 2) as usize;
            let spr = match e.kind { 0 => BEE[f], 1 => MOTH[f], _ => BOSS[f] };
            self.sprite(p, spr, e.x, e.y, self.color(e.kind), sc);
        }
        let head = dim.head;
        for b in &self.pbul { p.rect((b.x * sc as f32) as i32 - sc / 2, (b.y * sc as f32) as i32 - 2 * sc, sc, 4 * sc, head); }
        for b in &self.ebul { p.rect((b.x * sc as f32) as i32 - sc / 2, (b.y * sc as f32) as i32 - 2 * sc, sc, 4 * sc, dim.bands[1]); }
        if self.alive && (self.invuln <= 0.0 || (self.t * 12.0) as i32 % 2 == 0) {
            self.sprite(p, SHIP, self.px, self.py(), head, sc);
        }
        for q in &self.parts {
            let c = if q.life > 0.35 { dim.head } else if q.life > 0.18 { dim.bands[0] } else { dim.bands[2] };
            p.rect((q.x * sc as f32) as i32, (q.y * sc as f32) as i32, sc, sc, c);
        }
        if self.hud {
            // abajo, para que no lo tape la waybar (la capa de fondo cubre toda la pantalla)
            let u = (sc / 2).max(1);
            let y = (self.h * sc as f32) as i32 - 12 * u;
            for k in 0..(self.lives - 1).max(0) {
                self.sprite(p, SHIP, 8.0 + k as f32 * 11.0, self.h - 7.0, dim.bands[1], u);
            }
            let x0 = (8.0 + 3.0 * 11.0) as i32 * sc;
            self.text(p, &format!("SCORE {:06}", self.score), x0, y, u, dim.bands[0]);
            let st = format!("STAGE {}", self.stage);
            let wpx = (st.len() as i32 * 4 - 1) * u;
            self.text(p, &st, (self.w * sc as f32) as i32 - wpx - 6 * u, y, u, dim.bands[0]);
        }
    }
}

impl Scene for Galaga {
    fn resize(&mut self, p: &mut Painter, w: usize, h: usize) {
        if self.auto_scale { self.scale = (h as i32 / 190).max(2); } // ~190 unidades de alto
        p.resize(w, h);
        self.w = w as f32 / self.scale as f32;
        self.h = h as f32 / self.scale as f32;
        let n = (self.w * self.h / 700.0) as usize;
        self.stars = (0..n).map(|_| {
            let layer = (self.rng.next() % 3) as u8;
            Star { x: self.rng.f() as f32 * self.w, y: self.rng.f() as f32 * self.h, speed: [9.0, 16.0, 28.0][layer as usize], layer }
        }).collect();
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

    /// Simula ~10 min de juego a 30 FPS: sin panics, y la IA ni invencible ni inutil.
    #[test]
    fn long_simulation() {
        let pals = vec![crate::make_pal([255, 255, 0])];
        let mut g = Galaga::new(pals, 0, true);
        let mut p = Painter::plain();
        g.resize(&mut p, 1366, 768);
        let (mut max_stage, mut resets, mut last_lives, mut deaths, mut kills_score) = (1, 0, g.lives, 0, 0);
        for _ in 0..(30 * 600) {
            g.step(&mut p, 1.0 / 30.0);
            max_stage = max_stage.max(g.stage);
            if g.lives < last_lives { deaths += 1; }
            if g.lives > last_lives { resets += 1; } // vuelve a 3 vidas = nueva partida
            last_lives = g.lives;
            kills_score = kills_score.max(g.score);
            assert!(g.px >= 0.0 && g.px <= g.w, "jugador fuera de pantalla: {}", g.px);
        }
        println!("max_stage={max_stage} partidas_nuevas={resets} muertes={deaths} score_max={kills_score}");
        assert!(max_stage >= 2, "la IA no supera ni la primera oleada");
        assert!(deaths >= 1 || max_stage >= 6, "la IA parece invencible y sin avanzar");
    }

    #[test]
    fn sprites_are_rectangular() {
        for s in [SHIP, BEE[0], BEE[1], MOTH[0], MOTH[1], BOSS[0], BOSS[1]] {
            assert!(s.iter().all(|r| r.len() == s[0].len()), "sprite no rectangular: {s:?}");
        }
        assert!(BEE.iter().chain(&MOTH).chain(&BOSS).all(|s| s.len() == 8 && s[0].len() == 11));
    }
}
