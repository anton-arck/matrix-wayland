//! Escena "scope": osciloscopio mono como el Scope de Sonic Pi. Dibuja la forma de onda de lo que
//! suena (`parec` sobre el monitor de la salida) con disparo en cruce por cero para que la onda
//! quede quieta, y ganancia automatica. `--simulate` (o sin `parec`) dibuja una onda sintetica.
use super::{Painter, Pal, Scene};
use crate::equalizer::{scale, spawn_capture, Shared, FFT_N, RATE};

const WIN: usize = FFT_N / 2; // muestras dibujadas (~23 ms)

pub struct Scope {
    audio: Option<Shared>,
    t_arg: i32,
    cols: Vec<u32>, // gradiente a lo ancho (un solo color si no es rainbow)
    glow: Vec<u32>,
    axis: u32,
    w: i32, h: i32,
    gain: f32,
    t: f32,
    buf: Vec<f32>,
    ys: Vec<f32>,
}

fn lerp_col(a: u32, b: u32, f: f32) -> u32 {
    let ch = |c: u32, s: u32| ((c >> s) & 0xFF) as f32;
    let m = |s: u32| (ch(a, s) + (ch(b, s) - ch(a, s)) * f) as u32;
    0xFF00_0000 | m(16) << 16 | m(8) << 8 | m(0)
}

impl Scope {
    pub fn new(pals: Vec<Pal>, tile: i32, simulate: bool) -> Self {
        let audio = if simulate { None } else { spawn_capture() };
        if !simulate && audio.is_none() { eprintln!("scope: no hay parec (pulseaudio-utils / pipewire-pulse); se simula la onda"); }
        // rainbow: un degradado que recorre los neones de izquierda a derecha
        let order = [0usize, 1, 4, 5, 2, 3, 8, 6, 7];
        let stops: Vec<u32> = if pals.len() >= 9 { order.iter().map(|&i| pals[i].bands[0]).collect() } else { vec![pals[0].bands[0]] };
        let n = 256;
        let cols: Vec<u32> = (0..n).map(|i| {
            if stops.len() == 1 { return stops[0]; }
            let f = i as f32 / (n - 1) as f32 * (stops.len() - 1) as f32;
            let k = (f.floor() as usize).min(stops.len() - 2);
            lerp_col(stops[k], stops[k + 1], f - k as f32)
        }).collect();
        let glow = cols.iter().map(|&c| scale(c, 0.28)).collect();
        Scope { audio, t_arg: tile, axis: scale(cols[0], 0.12), cols, glow, w: 0, h: 0, gain: 1.0, t: 0.0, buf: vec![0.0; FFT_N], ys: vec![] }
    }

    fn fetch(&mut self, dt: f32) {
        self.t += dt;
        if let Some(a) = &self.audio {
            self.buf = a.lock().unwrap().clone();
        } else {
            // onda sintetica: bajo con envolvente de bombo, mas un par de armonicos que se mueven
            let beat = (self.t * 2.0).fract();
            let env = 0.35 + 0.65 * (-beat * 5.0).exp();
            for i in 0..FFT_N {
                let t = self.t + i as f32 / RATE as f32;
                let tau = std::f32::consts::TAU;
                self.buf[i] = env * (0.6 * (tau * 110.0 * t).sin() + 0.3 * (tau * (330.0 + 40.0 * (self.t * 0.7).sin()) * t).sin()
                    + 0.15 * (tau * 1250.0 * t).sin());
            }
        }
    }
}

impl Scene for Scope {
    fn resize(&mut self, p: &mut Painter, w: usize, h: usize) {
        p.resize(w, h);
        self.w = w as i32;
        self.h = h as i32;
        self.ys = vec![0.0; w];
    }

    fn step(&mut self, p: &mut Painter, dt: f32) {
        self.fetch(dt.min(0.1));
        // disparo: primer cruce ascendente por cero en la primera mitad, para que la onda no baile
        let start = (1..FFT_N - WIN).find(|&i| self.buf[i - 1] < 0.0 && self.buf[i] >= 0.0).unwrap_or(0);
        let win = &self.buf[start..start + WIN];
        // ganancia automatica: sube al instante con el pico y baja despacio; el suelo evita amplificar ruido
        let peak = win.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        let want = 1.0 / peak.max(0.04);
        self.gain = if want < self.gain { want } else { self.gain + (want - self.gain) * (1.0 - (-dt * 1.5).exp()) };
        let g = self.gain.min(25.0) * 0.85;

        p.clear();
        let (w, h) = (self.w, self.h);
        let cy = h / 2;
        let amp = h as f32 * 0.46;
        let th = self.t_arg.max(if self.t_arg > 0 { 1 } else { (h / 260).clamp(2, 6) });
        p.rect(0, cy, w, 1, self.axis);
        for x in 0..w as usize {
            let f = x as f32 / (w - 1).max(1) as f32 * (WIN - 1) as f32;
            let (i, fr) = (f.floor() as usize, f.fract());
            let v = win[i] * (1.0 - fr) + win[(i + 1).min(WIN - 1)] * fr;
            self.ys[x] = cy as f32 - (v * g).clamp(-1.0, 1.0) * amp;
        }
        // dos pasadas: halo ancho y tenue, y encima el trazo
        for pass in 0..2 {
            let t = if pass == 0 { th * 3 } else { th };
            for x in 0..w as usize {
                let y1 = self.ys[x];
                let y0 = if x > 0 { self.ys[x - 1] } else { y1 };
                let (a, b) = (y0.min(y1) as i32, y0.max(y1) as i32);
                let ci = x * (self.cols.len() - 1) / (w as usize - 1).max(1);
                let c = if pass == 0 { self.glow[ci] } else { self.cols[ci] };
                p.rect(x as i32 - t / 2, a - t / 2, t, b - a + t, c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_sim() {
        for pals in [vec![crate::make_pal([0, 255, 0])], crate::NEON.iter().map(|(_, c)| crate::make_pal(*c)).collect()] {
            let mut s = Scope::new(pals, 0, true);
            let mut p = Painter::plain();
            for &(w, h) in &[(640usize, 360usize), (1920, 1080), (160, 100)] {
                s.resize(&mut p, w, h);
                for _ in 0..200 { s.step(&mut p, 1.0 / 30.0); }
                assert!(p.fb.iter().any(|&c| c != 0xFF00_0000));
            }
        }
    }
}
