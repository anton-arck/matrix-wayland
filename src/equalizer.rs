//! Escena "equalizer": analizador de espectro con barras de LED cuadrados (como los ecualizadores
//! de las cadenas de musica). No captura audio: simula una pista con bombo, caja y hi-hats que
//! excitan las barras graves, medias y agudas, con caida suave y marcador de pico.
use super::{Painter, Pal, Rng, Scene};

pub struct Equalizer {
    rng: Rng,
    t_arg: i32,
    on: [u32; 3],  // color de los LED encendidos: bajo, medio, alto
    off: u32,      // LED apagado
    peak_col: u32,
    // geometria
    led: i32, ledh: i32, gap: i32, cols: usize, rows: i32, ox: i32, oy: i32,
    // estado de las barras
    lvl: Vec<f32>, peak: Vec<f32>, hold: Vec<f32>, vel: Vec<f32>, noise: Vec<f32>,
    // ritmo
    t: f32, beat: f32, kick: f32, snare: f32, hat: f32, bpm: f32, bass: f32,
}

fn scale(c: u32, k: f32) -> u32 {
    let ch = |s: u32| (((c >> s) & 0xFF) as f32 * k).min(255.0) as u32;
    0xFF00_0000 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

impl Equalizer {
    pub fn new(pals: Vec<Pal>, tile: i32) -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
        // rainbow: verde -> amarillo -> rojo, como los VU clasicos (orden NEON: verde=0, amarillo=7, rojo=8)
        let on = if pals.len() >= 9 {
            [pals[0].bands[0], pals[7].bands[0], pals[8].bands[0]]
        } else {
            let p = &pals[0];
            [p.bands[0], p.bands[0], p.head]
        };
        Equalizer {
            rng: Rng(seed | 1), t_arg: tile, on, off: scale(pals[0].bands[0], 0.07),
            peak_col: pals[0].head,
            led: 24, ledh: 8, gap: 2, cols: 0, rows: 0, ox: 0, oy: 0,
            lvl: vec![], peak: vec![], hold: vec![], vel: vec![], noise: vec![],
            t: 0.0, beat: 0.0, kick: 0.0, snare: 0.0, hat: 0.0, bpm: 124.0, bass: 0.0,
        }
    }

    fn color(&self, r: i32) -> u32 {
        let f = r as f32 / self.rows as f32;
        if f < 0.6 { self.on[0] } else if f < 0.82 { self.on[1] } else { self.on[2] }
    }

    fn drive(&mut self, dt: f32) {
        self.t += dt;
        // la energia sube y baja lentamente (estrofa / estribillo) y el tempo cambia de vez en cuando
        let energy = 0.72 + 0.28 * (self.t * 0.11).sin() * (self.t * 0.037).cos();
        let prev = self.beat;
        self.beat += dt * self.bpm / 60.0;
        let (pb, nb) = (prev, self.beat);
        if nb.floor() > pb.floor() {
            self.kick = 1.0;
            if (nb.floor() as i64) % 2 == 1 { self.snare = 1.0; }
            if (nb.floor() as i64) % 32 == 0 { self.bpm = self.rng.rf(110.0, 138.0); }
        }
        if (nb * 2.0).floor() > (pb * 2.0).floor() { self.hat = 1.0; }
        if self.rng.f() < 0.02 { self.hat = self.hat.max(0.7); } // contratiempos sueltos
        self.kick *= (-dt * 7.0).exp();
        self.snare *= (-dt * 9.0).exp();
        self.hat *= (-dt * 14.0).exp();
        // linea de bajo: ondula entre pulsos
        self.bass = 0.5 + 0.5 * (self.beat * std::f32::consts::PI * 0.5).sin();

        let n = self.cols;
        for i in 0..n {
            let x = i as f32 / (n.max(2) - 1) as f32;
            let g = |c: f32, w: f32| (-((x - c) / w).powi(2)).exp();
            // el ruido por barra (paseo aleatorio suavizado) da el parpadeo individual
            self.noise[i] += (self.rng.f() as f32 - self.noise[i]) * (1.0 - (-dt * 9.0).exp());
            let jitter = 0.55 + 0.9 * self.noise[i];
            let base = (0.16 + 0.34 * self.bass * g(0.12, 0.2)) * (1.0 - 0.55 * x);
            let target = (self.kick * g(0.08, 0.17) * 0.95
                + self.snare * g(0.42, 0.2) * 0.8
                + self.hat * x.powf(1.5) * 0.7
                + base) * energy * jitter;
            let target = target.clamp(0.0, 1.0);
            if target > self.lvl[i] {
                self.lvl[i] += (target - self.lvl[i]) * (1.0 - (-dt * 40.0).exp());
            } else {
                self.lvl[i] = (self.lvl[i] - dt * 1.5).max(target.min(self.lvl[i]));
            }
            // marcador de pico: se queda un momento arriba y luego cae con aceleracion
            if self.lvl[i] >= self.peak[i] {
                self.peak[i] = self.lvl[i];
                self.hold[i] = 0.45;
                self.vel[i] = 0.0;
            } else if self.hold[i] > 0.0 {
                self.hold[i] -= dt;
            } else {
                self.vel[i] += dt * 2.2;
                self.peak[i] = (self.peak[i] - self.vel[i] * dt).max(self.lvl[i]);
            }
        }
    }

    fn paint(&self, p: &mut Painter) {
        let (px, py) = (self.led + self.gap, self.ledh + self.gap);
        for i in 0..self.cols {
            let x = self.ox + i as i32 * px;
            let lit = (self.lvl[i] * self.rows as f32).round() as i32;
            let pk = ((self.peak[i] * self.rows as f32).round() as i32 - 1).clamp(0, self.rows - 1);
            for r in 0..self.rows {
                let y = self.oy - (r + 1) * py + self.gap;
                let c = if r == pk && pk >= lit && self.peak[i] > 0.02 { self.peak_col }
                    else if r < lit { self.color(r) } else { self.off };
                p.rect(x, y, self.led, self.ledh, c);
            }
        }
    }
}

impl Scene for Equalizer {
    fn resize(&mut self, p: &mut Painter, w: usize, h: usize) {
        p.resize(w, h);
        p.clear();
        // LED rectangulares: el doble de anchos que el lado base y bastante mas bajos
        let side = if self.t_arg > 0 { self.t_arg } else { (h as i32 / 38).clamp(8, 40) };
        self.led = side * 2;
        self.ledh = (side * 3 / 5).max(3);
        self.gap = (side / 5).max(2);
        let (px, py) = (self.led + self.gap, self.ledh + self.gap);
        let margin = (w as i32 / 24).max(px);
        self.cols = (((w as i32 - 2 * margin) / px).max(4)) as usize;
        self.rows = (((h as i32 * 88 / 100) / py).max(4)).min(80);
        self.ox = (w as i32 - self.cols as i32 * px + self.gap) / 2;
        self.oy = h as i32 - (h as i32 - self.rows * py) / 2 + self.gap; // bloque centrado
        let n = self.cols;
        self.lvl = vec![0.0; n];
        self.peak = vec![0.0; n];
        self.hold = vec![0.0; n];
        self.vel = vec![0.0; n];
        self.noise = vec![0.5; n];
    }
    fn step(&mut self, p: &mut Painter, dt: f32) {
        self.drive(dt.min(0.1));
        self.paint(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_stay_in_range() {
        for pals in [vec![crate::make_pal([0, 255, 0])], crate::NEON.iter().map(|(_, c)| crate::make_pal(*c)).collect()] {
            let mut s = Equalizer::new(pals, 0);
            let mut p = Painter::plain();
            for &(w, h) in &[(640usize, 360usize), (1920, 1080), (160, 100)] {
                s.resize(&mut p, w, h);
                for _ in 0..3000 { s.step(&mut p, 1.0 / 30.0); }
                assert!(s.lvl.iter().chain(s.peak.iter()).all(|&v| (0.0..=1.0).contains(&v)));
                assert!(s.lvl.iter().cloned().fold(0.0, f32::max) > 0.05);
            }
        }
    }
}
