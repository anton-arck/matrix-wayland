//! Escenas "equalizer" y "spectrum" (la misma pero en espejo): analizador de espectro con barras de LED rectangulares (como los
//! ecualizadores de las cadenas de musica), con caida suave y marcador de pico.
//! Reacciona a lo que suena de verdad: `parec` captura el monitor de la salida por defecto
//! (PipeWire/PulseAudio) y una FFT propia reparte el espectro en barras logaritmicas. Con
//! `--simulate` (o si no hay `parec`) simula una pista con bombo, caja y hi-hats.
use super::{Painter, Pal, Rng, Scene};
use std::io::Read;
use std::sync::{Arc, Mutex};

pub(crate) const RATE: usize = 44100;
pub(crate) const FFT_N: usize = 2048;

extern "C" { fn prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i32; }

/// Ultimas muestras capturadas (mono, -1..1), compartidas con el hilo lector.
pub(crate) type Shared = Arc<Mutex<Vec<f32>>>;

/// Hilo que lanza `parec` sobre el monitor de la salida por defecto y vuelca las muestras.
/// Si parec termina (p. ej. PipeWire se reinicia) lo relanza a los 2 s.
pub(crate) fn spawn_capture() -> Option<Shared> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut probe = Command::new("parec");
    probe.arg("--version").stdout(Stdio::null()).stderr(Stdio::null());
    if probe.status().is_err() { return None; }
    let shared: Shared = Arc::new(Mutex::new(vec![0.0; FFT_N]));
    let out = shared.clone();
    std::thread::spawn(move || loop {
        let mut c = Command::new("parec");
        c.args(["-d", "@DEFAULT_MONITOR@", "--format=s16le", "--rate=44100", "--channels=1", "--latency-msec=20", "--property=stream.monitor=true"])
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        // que parec muera con nosotros (PR_SET_PDEATHSIG = 1, SIGTERM = 15)
        unsafe { c.pre_exec(|| { prctl(1, 15, 0, 0, 0); Ok(()) }); }
        if let Ok(mut child) = c.spawn() {
            if let Some(mut so) = child.stdout.take() {
                let mut buf = [0u8; 2048];
                while let Ok(n) = so.read(&mut buf) {
                    if n == 0 { break; }
                    let new: Vec<f32> = buf[..n / 2 * 2].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
                    let mut v = out.lock().unwrap();
                    v.extend(new);
                    let excess = v.len().saturating_sub(FFT_N);
                    v.drain(..excess);
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    });
    Some(shared)
}

/// FFT radix-2 in situ (len potencia de 2).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 { j ^= bit; bit >>= 1; }
        j ^= bit;
        if i < j { re.swap(i, j); im.swap(i, j); }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for s in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (s + k, s + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr; im[b] = im[a] - ti;
                re[a] += tr; im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
}

/// Espectro en barras logaritmicas (45 Hz - 14 kHz) normalizado a 0..1 con ganancia automatica.
struct Spectrum { win: Vec<f32>, re: Vec<f32>, im: Vec<f32>, top: f32 }

impl Spectrum {
    fn new() -> Self {
        let win = (0..FFT_N).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT_N as f32).cos()).collect();
        Spectrum { win, re: vec![0.0; FFT_N], im: vec![0.0; FFT_N], top: -40.0 }
    }

    /// Rango de frecuencias (Hz) de la barra i de n.
    fn edges(i: usize, n: usize) -> (f32, f32) {
        let f = |k: usize| 45.0 * (14000.0f32 / 45.0).powf(k as f32 / n as f32);
        (f(i), f(i + 1))
    }

    fn bars(&mut self, samples: &[f32], dt: f32, out: &mut [f32]) {
        let n = out.len();
        for i in 0..FFT_N { self.re[i] = samples[i] * self.win[i]; self.im[i] = 0.0; }
        fft(&mut self.re, &mut self.im);
        let bin_hz = RATE as f32 / FFT_N as f32;
        let mag = |b: f32| { // interpolacion lineal entre bins vecinos
            let (b0, fr) = (b.floor() as usize, b.fract());
            let m = |k: usize| { let k = k.min(FFT_N / 2 - 1); (self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt() };
            m(b0) * (1.0 - fr) + m(b0 + 1) * fr
        };
        let mut db = vec![-120.0f32; n];
        let mut frame_max = -120.0f32;
        for i in 0..n {
            let (lo, hi) = Self::edges(i, n);
            let (b_lo, b_hi) = (lo / bin_hz, hi / bin_hz);
            let mut m = mag((b_lo + b_hi) / 2.0);
            let mut b = b_lo.ceil();
            while b < b_hi { m = m.max(mag(b)); b += 1.0; }
            let amp = 4.0 * m / FFT_N as f32; // 1.0 = seno a fondo de escala
            // +3 dB por octava: la musica cae hacia los agudos y asi las barras altas no quedan vacias
            let tilt = 3.0 * ((lo * hi).sqrt() / 1000.0).log2();
            db[i] = 20.0 * amp.max(1e-7).log10() + tilt;
            frame_max = frame_max.max(db[i]);
        }
        // referencia: sube al instante con el pico y baja lento; con suelo para no amplificar el ruido
        self.top = if frame_max > self.top { frame_max } else { (self.top - 4.0 * dt).max(frame_max) };
        // margen de 3 dB sobre el pico y rodilla suave por encima del 70 %: la barra mas fuerte se queda
        // en ~0.9 en vez de pegarse al techo (antes, con la referencia = pico, tocaba el 100 % en cada golpe)
        let top = self.top.max(-42.0) + 3.0;
        for i in 0..n {
            let x = (db[i] - (top - 50.0)) / 50.0;
            out[i] = if x < 0.7 { x } else { 0.7 + 0.3 * ((x - 0.7) / 0.3).tanh() }.clamp(0.0, 1.0);
        }
    }
}

pub struct Equalizer {
    rng: Rng,
    t_arg: i32,
    bar_w: i32, // ancho de barra en px (0 = el doble del alto base)
    audio: Option<Shared>,
    mirror: bool, // barras desde la linea central hacia arriba y hacia abajo
    spec: Spectrum,
    tgt: Vec<f32>,
    on: [u32; 3],  // color de los LED encendidos: bajo, medio, alto
    off: u32,      // LED apagado
    peak_col: u32,
    // geometria
    wpx: i32, hpx: i32,
    led: i32, ledh: i32, gap: i32, cols: usize, rows: i32, ox: i32, oy: i32,
    // estado de las barras
    lvl: Vec<f32>, peak: Vec<f32>, hold: Vec<f32>, vel: Vec<f32>, noise: Vec<f32>,
    // ritmo
    t: f32, beat: f32, kick: f32, snare: f32, hat: f32, bpm: f32, bass: f32,
}

pub(crate) fn scale(c: u32, k: f32) -> u32 {
    let ch = |s: u32| (((c >> s) & 0xFF) as f32 * k).min(255.0) as u32;
    0xFF00_0000 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

impl Equalizer {
    pub fn new(pals: Vec<Pal>, tile: i32, simulate: bool, mirror: bool, bar_w: i32) -> Self {
        let audio = if simulate { None } else { spawn_capture() };
        if !simulate && audio.is_none() { eprintln!("equalizer: no hay parec (pulseaudio-utils / pipewire-pulse); se simula la musica"); }
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
        // rainbow: verde -> amarillo -> rojo, como los VU clasicos (orden NEON: verde=0, amarillo=7, rojo=8)
        let on = if pals.len() >= 9 {
            [pals[0].bands[0], pals[7].bands[0], pals[8].bands[0]]
        } else {
            let p = &pals[0];
            [p.bands[0], p.bands[0], p.head]
        };
        Equalizer {
            rng: Rng(seed | 1), t_arg: tile, bar_w, audio, mirror, spec: Spectrum::new(), tgt: vec![], on, off: scale(pals[0].bands[0], 0.07),
            peak_col: pals[0].head,
            wpx: 0, hpx: 0, led: 24, ledh: 8, gap: 2, cols: 0, rows: 0, ox: 0, oy: 0,
            lvl: vec![], peak: vec![], hold: vec![], vel: vec![], noise: vec![],
            t: 0.0, beat: 0.0, kick: 0.0, snare: 0.0, hat: 0.0, bpm: 124.0, bass: 0.0,
        }
    }

    fn color(&self, r: i32) -> u32 {
        let f = r as f32 / self.rows as f32;
        if f < 0.5 { self.on[0] } else if f < 0.75 { self.on[1] } else { self.on[2] }
    }

    /// Objetivo (0..1) de cada barra a partir de la musica simulada.
    fn simulate(&mut self, dt: f32) {
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
            self.tgt[i] = target.clamp(0.0, 1.0);
        }
    }

    fn drive(&mut self, dt: f32) {
        let n = self.cols;
        if let Some(a) = &self.audio {
            let samples = a.lock().unwrap().clone();
            let mut t = std::mem::take(&mut self.tgt);
            self.spec.bars(&samples, dt, &mut t);
            self.tgt = t;
        } else {
            self.simulate(dt);
        }
        for i in 0..n {
            let target = self.tgt[i];
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

    /// Reparte `total` px en `n` tramos separados por `gap`; devuelve (inicio, largo) del tramo i.
    /// Asi los LED rellenan el espacio exacto sin sobrar ni faltar un pixel.
    fn span(i: i32, n: i32, total: i32, gap: i32) -> (i32, i32) {
        let a = i * (total + gap) / n;
        let b = (i + 1) * (total + gap) / n - gap;
        (a, b - a)
    }

    fn paint(&self, p: &mut Painter) {
        let (px, py) = (self.led + self.gap, self.ledh + self.gap);
        // spectrum (espejo): los LED se estiran para cubrir toda la pantalla, sin margen
        let reach = (self.hpx / 2 - self.gap / 2).max(1);
        for i in 0..self.cols {
            let (x, wid) = if self.mirror { Self::span(i as i32, self.cols as i32, self.wpx, self.gap) }
                else { (self.ox + i as i32 * px, self.led) };
            let lit = (self.lvl[i] * self.rows as f32).round() as i32;
            let pk = ((self.peak[i] * self.rows as f32).round() as i32 - 1).clamp(0, self.rows - 1);
            for r in 0..self.rows {
                let c = if r == pk && pk >= lit && self.peak[i] > 0.02 { self.peak_col }
                    else if r < lit { self.color(r) } else { self.off };
                if self.mirror {
                    // la fila 0 de cada mitad queda pegada a la linea central
                    let (s, l) = Self::span(r, self.rows, reach, self.gap);
                    p.rect(x, self.hpx / 2 - self.gap / 2 - s - l, wid, l, c);
                    p.rect(x, self.hpx / 2 + self.gap / 2 + s, wid, l, c);
                } else {
                    p.rect(x, self.oy - (r + 1) * py + self.gap, wid, self.ledh, c);
                }
            }
        }
    }
}

impl Scene for Equalizer {
    fn resize(&mut self, p: &mut Painter, w: usize, h: usize) {
        p.resize(w, h);
        p.clear();
        self.wpx = w as i32;
        self.hpx = h as i32;
        // LED rectangulares: el doble de anchos que el lado base y bastante mas bajos
        let side = if self.t_arg > 0 { self.t_arg } else { (h as i32 / 38).clamp(8, 40) };
        self.led = if self.bar_w > 0 { self.bar_w } else { side * 2 };
        self.ledh = (side * 3 / 5).max(3);
        self.gap = (side / 5).max(2);
        let (px, py) = (self.led + self.gap, self.ledh + self.gap);
        let margin = if self.mirror { 0 } else { (w as i32 / 24).max(px) };
        self.cols = (((w as i32 - 2 * margin + self.gap) / px).max(4)) as usize;
        let span = if self.mirror { (h as i32 / 2 - self.gap / 2).max(1) } else { h as i32 * 88 / 100 };
        self.rows = (((span + self.gap) / py).max(4)).min(80);
        self.ox = (w as i32 - self.cols as i32 * px + self.gap) / 2;
        self.oy = if self.mirror { h as i32 / 2 - self.gap / 2 } // linea central
            else { h as i32 - (h as i32 - self.rows * py) / 2 + self.gap }; // bloque centrado
        let n = self.cols;
        self.lvl = vec![0.0; n];
        self.peak = vec![0.0; n];
        self.hold = vec![0.0; n];
        self.vel = vec![0.0; n];
        self.noise = vec![0.5; n];
        self.tgt = vec![0.0; n];
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
        for mirror in [false, true] { for pals in [vec![crate::make_pal([0, 255, 0])], crate::NEON.iter().map(|(_, c)| crate::make_pal(*c)).collect()] {
            let mut s = Equalizer::new(pals, 0, true, mirror, 0);
            let mut p = Painter::plain();
            for &(w, h) in &[(640usize, 360usize), (1920, 1080), (160, 100)] {
                s.resize(&mut p, w, h);
                for _ in 0..3000 { s.step(&mut p, 1.0 / 30.0); }
                assert!(s.lvl.iter().chain(s.peak.iter()).all(|&v| (0.0..=1.0).contains(&v)));
                assert!(s.lvl.iter().cloned().fold(0.0, f32::max) > 0.05);
            }
        } }
    }
}

#[cfg(test)]
mod audio_tests {
    use super::*;

    #[test]
    fn sine_lights_the_right_bar() {
        let mut sp = Spectrum::new();
        let n = 48;
        for &f in &[100.0f32, 440.0, 1000.0, 5000.0, 12000.0] {
            let x: Vec<f32> = (0..FFT_N).map(|i| 0.5 * (2.0 * std::f32::consts::PI * f * i as f32 / RATE as f32).sin()).collect();
            let mut out = vec![0.0; n];
            sp.top = -40.0;
            sp.bars(&x, 0.03, &mut out);
            let best = (0..n).max_by(|&a, &b| out[a].partial_cmp(&out[b]).unwrap()).unwrap();
            let (lo, hi) = Spectrum::edges(best, n);
            assert!(f >= lo * 0.85 && f <= hi * 1.15, "{f} Hz cae en la barra {best} ({lo}-{hi})");
        }
    }

    #[test]
    fn loud_input_keeps_headroom() {
        let mut sp = Spectrum::new();
        // musica muy fuerte y densa: varios tonos a fondo de escala; nada debe pegarse al techo
        let x: Vec<f32> = (0..FFT_N).map(|i| {
            let t = i as f32 / RATE as f32;
            [60.0f32, 250.0, 1000.0, 4000.0].iter().map(|f| 0.24 * (std::f32::consts::TAU * f * t).sin()).sum()
        }).collect();
        let mut out = vec![0.0; 48];
        for _ in 0..30 { sp.bars(&x, 0.03, &mut out); }
        let m = out.iter().cloned().fold(0.0, f32::max);
        assert!(m > 0.8 && m < 0.95, "pico {m}");
    }

    #[test]
    fn silence_is_dark() {
        let mut sp = Spectrum::new();
        let mut out = vec![1.0; 32];
        sp.bars(&vec![0.0; FFT_N], 0.03, &mut out);
        assert!(out.iter().all(|&v| v == 0.0));
    }
}
