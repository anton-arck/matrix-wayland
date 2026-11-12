//! Efecto Matrix como fondo de pantalla Wayland (wlr-layer-shell + wl_shm).
mod galaga;
mod pacman;
mod snake;
mod tron;

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            timer::{TimeoutAction, Timer},
            EventLoop,
        },
        calloop_wayland_source::WaylandSource,
        client::{
            globals::registry_queue_init,
            protocol::{wl_output, wl_shm, wl_surface},
            Connection, QueueHandle,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};

const NEON: [(&str, [u8; 3]); 9] = [
    ("green", [57, 255, 20]), ("cyan", [0, 255, 255]), ("magenta", [255, 0, 255]),
    ("pink", [255, 16, 240]), ("blue", [30, 144, 255]), ("purple", [191, 0, 255]),
    ("orange", [255, 95, 0]), ("yellow", [255, 255, 0]), ("red", [255, 20, 40]),
];
const BANDS: [f64; 4] = [1.0, 0.65, 0.38, 0.18];
const BG: u32 = 0xFF00_0000;

// ---------- utilidades ----------
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0 }
    fn f(&mut self) -> f64 { (self.next() >> 11) as f64 / (1u64 << 53) as f64 }
    fn rf(&mut self, lo: f32, hi: f32) -> f32 { lo + (hi - lo) * self.f() as f32 }
    fn range(&mut self, lo: i32, hi: i32) -> i32 { lo + (self.next() % (hi - lo + 1).max(1) as u64) as i32 }
}

fn rgb(c: [u8; 3]) -> u32 { 0xFF00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32 }

#[derive(Clone, Copy)]
struct Pal { head: u32, bands: [u32; 4] }
fn make_pal(c: [u8; 3]) -> Pal {
    let h = |v: u8| (v as f64 + (255.0 - v as f64) * 0.75) as u8;
    let s = |i: usize| rgb([(c[0] as f64 * BANDS[i]) as u8, (c[1] as f64 * BANDS[i]) as u8, (c[2] as f64 * BANDS[i]) as u8]);
    Pal { head: rgb([h(c[0]), h(c[1]), h(c[2])]), bands: [s(0), s(1), s(2), s(3)] }
}
fn parse_color(v: &str) -> Option<[u8; 3]> {
    let v = v.to_lowercase();
    if let Some((_, c)) = NEON.iter().find(|(n, _)| *n == v) { return Some(*c); }
    let h = v.trim_start_matches('#');
    if h.len() == 6 {
        let n = u32::from_str_radix(h, 16).ok()?;
        return Some([(n >> 16) as u8, (n >> 8) as u8, n as u8]);
    }
    None
}
fn charset(name: &str) -> Vec<char> {
    let kata: Vec<char> = (0xFF71u32..0xFF9E).filter_map(char::from_u32).collect();
    let digits: Vec<char> = "0123456789".chars().collect();
    match name {
        "binary" => "01".chars().collect(),
        "digits" => digits,
        "hex" => "0123456789ABCDEF".chars().collect(),
        "katakana" => kata,
        _ => kata.into_iter().chain(digits).collect(),
    }
}

// ---------- pintor: framebuffer en CPU + glifos rasterizados ----------
struct Glyph { w: usize, h: usize, left: i32, top: i32, cov: Vec<u8> }

struct Painter {
    fb: Vec<u32>,
    w: usize,
    h: usize,
    cell_w: usize,
    cell_h: usize,
    pad_x: usize,
    pad_y: usize,
    glyphs: HashMap<char, Glyph>,
}

impl Painter {
    fn new(font: &fontdue::Font, chars: &[char], px: f32) -> Self {
        let lm = font.horizontal_line_metrics(px).expect("la fuente no tiene metricas");
        let cell_w = (px * 0.5).round().max(4.0) as usize; // media anchura
        let cell_h = (lm.ascent - lm.descent).round().max(4.0) as usize;
        let baseline = lm.ascent.round() as i32;
        let mut glyphs = HashMap::new();
        for &c in chars {
            let (m, cov) = font.rasterize(c, px);
            // centrado horizontal en la celda; vertical respecto a la linea base
            let left = (cell_w as i32 - m.width as i32) / 2;
            let top = baseline - (m.ymin + m.height as i32);
            glyphs.insert(c, Glyph { w: m.width, h: m.height, left, top, cov });
        }
        Painter { fb: vec![], w: 0, h: 0, cell_w, cell_h, pad_x: 0, pad_y: 0, glyphs }
    }

    /// Sin fuente ni glifos (escenas de pixeles, p. ej. galaga): evita cargar la fuente CJK.
    fn plain() -> Self {
        Painter { fb: vec![], w: 0, h: 0, cell_w: 8, cell_h: 16, pad_x: 0, pad_y: 0, glyphs: HashMap::new() }
    }

    fn clear(&mut self) { self.fb.fill(BG); }

    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        let (x0, y0) = (x.max(0), y.max(0));
        let (x1, y1) = ((x + w).min(self.w as i32), (y + h).min(self.h as i32));
        if x0 >= x1 || y0 >= y1 { return; }
        for yy in y0..y1 {
            let row = yy as usize * self.w;
            self.fb[row + x0 as usize..row + x1 as usize].fill(c);
        }
    }

    fn resize(&mut self, w: usize, h: usize) -> (usize, usize) {
        self.w = w;
        self.h = h;
        self.fb = vec![BG; w * h];
        let (cols, rows) = (w / self.cell_w, h / self.cell_h);
        self.pad_x = (w - cols * self.cell_w) / 2;
        self.pad_y = (h - rows * self.cell_h) / 2;
        (cols, rows)
    }

    fn erase(&mut self, cx: usize, cy: usize) {
        let (x0, y0) = (self.pad_x + cx * self.cell_w, self.pad_y + cy * self.cell_h);
        for y in y0..y0 + self.cell_h {
            let row = y * self.w;
            self.fb[row + x0..row + x0 + self.cell_w].fill(BG);
        }
    }

    fn cell(&mut self, cx: usize, cy: usize, ch: char, color: u32) {
        self.erase(cx, cy);
        let g = match self.glyphs.get(&ch) { Some(g) => g, None => return };
        let (x0, y0) = ((self.pad_x + cx * self.cell_w) as i32, (self.pad_y + cy * self.cell_h) as i32);
        let (cr, cg, cb) = ((color >> 16) & 255, (color >> 8) & 255, color & 255);
        for gy in 0..g.h {
            let y = y0 + g.top + gy as i32;
            if y < y0 || y >= y0 + self.cell_h as i32 { continue; }
            for gx in 0..g.w {
                let x = x0 + g.left + gx as i32;
                if x < x0 || x >= x0 + self.cell_w as i32 { continue; }
                let a = g.cov[gy * g.w + gx] as u32;
                if a == 0 { continue; }
                let px = 0xFF00_0000 | (cr * a / 255) << 16 | (cg * a / 255) << 8 | (cb * a / 255);
                self.fb[y as usize * self.w + x as usize] = px;
            }
        }
    }
}

// ---------- logica del efecto ----------
/// Una escena anima el framebuffer del `Painter` (matrix, galaga...).
trait Scene {
    fn resize(&mut self, p: &mut Painter, w: usize, h: usize);
    fn step(&mut self, p: &mut Painter, dt: f32);
}

struct Matrix {
    cols: usize,
    rows: usize,
    grid: Vec<char>,
    head: Vec<i32>,
    length: Vec<i32>,
    speed: Vec<f64>,
    pals: Vec<Pal>,
    chars: Vec<char>,
    rng: Rng,
}

impl Matrix {
    fn new(pals: Vec<Pal>, chars: Vec<char>) -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
        Matrix { cols: 0, rows: 0, grid: vec![], head: vec![], length: vec![], speed: vec![], pals, chars, rng: Rng(seed | 1) }
    }
    fn new_len(&mut self) -> i32 { self.rng.range((self.rows as i32 / 4).max(4), (self.rows as i32).max(6)) }
    fn reset(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
        self.grid = vec![' '; cols * rows];
        self.head = (0..cols).map(|_| -self.rng.range(0, rows as i32)).collect();
        self.length = (0..cols).map(|_| self.new_len()).collect();
        self.speed = (0..cols).map(|_| 0.4 + self.rng.f() * 0.6).collect();
    }
    fn rand_char(&mut self) -> char { self.chars[(self.rng.next() % self.chars.len() as u64) as usize] }

    fn advance(&mut self, p: &mut Painter) {
        let ri = self.rows as i32;
        for x in 0..self.cols {
            if self.rng.f() > self.speed[x] { continue; }
            let (y, l) = (self.head[x], self.length[x]);
            let pal = self.pals[x % self.pals.len()];
            if y >= 0 && y < ri {
                let ch = self.rand_char();
                self.grid[y as usize * self.cols + x] = ch;
                p.cell(x, y as usize, ch, pal.head);
            }
            if y >= 1 && y - 1 < ri {
                p.cell(x, (y - 1) as usize, self.grid[(y - 1) as usize * self.cols + x], pal.bands[0]);
            }
            for b in 1..=3i32 {
                let t = y - ((b * l + 3) / 4).max(b + 1);
                if t >= 0 && t < ri {
                    p.cell(x, t as usize, self.grid[t as usize * self.cols + x], pal.bands[b as usize]);
                }
            }
            let t = y - l;
            if t >= 0 && t < ri {
                self.grid[t as usize * self.cols + x] = ' ';
                p.erase(x, t as usize);
            }
            self.head[x] += 1;
            if self.head[x] - l > ri {
                self.head[x] = -self.rng.range(0, ri / 2);
                self.length[x] = self.new_len();
            }
            if self.rng.f() < 0.05 && y >= 0 && y < ri {
                let ry = self.rng.range((y - l).max(0), y);
                let i = ry as usize * self.cols + x;
                if ry < ri && self.grid[i] != ' ' {
                    let ch = self.rand_char();
                    self.grid[i] = ch;
                    p.cell(x, ry as usize, ch, pal.bands[(((y - ry) * 4) / l).min(3) as usize]);
                }
            }
        }
    }
}

impl Scene for Matrix {
    fn resize(&mut self, p: &mut Painter, w: usize, h: usize) {
        let (cols, rows) = p.resize(w, h);
        self.reset(cols, rows);
    }
    fn step(&mut self, p: &mut Painter, _dt: f32) { self.advance(p); }
}

// ---------- cliente Wayland ----------
struct App {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    width: u32,
    height: u32,
    configured: bool,
    frame_pending: bool,
    exit: bool,
    painter: Painter,
    scene: Box<dyn Scene>,
    dt: f32,
}

impl App {
    fn draw(&mut self, qh: &QueueHandle<Self>) {
        self.scene.step(&mut self.painter, self.dt);
        let (w, h) = (self.width as i32, self.height as i32);
        let (buffer, canvas) = self.pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888).expect("create buffer");
        // SAFETY: u32 -> bytes; el framebuffer tiene exactamente w*h*4 bytes.
        let bytes = unsafe { std::slice::from_raw_parts(self.painter.fb.as_ptr() as *const u8, self.painter.fb.len() * 4) };
        canvas.copy_from_slice(bytes);
        let surf = self.layer.wl_surface();
        surf.damage_buffer(0, 0, w, h);
        surf.frame(qh, FrameCallbackData(surf.clone())); // si nadie ve el fondo, no habra callback y no se pinta
        self.frame_pending = true;
        buffer.attach_to(surf).expect("attach");
        self.layer.commit();
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState { &mut self.output_state }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) { self.exit = true; }
    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &LayerSurface, cfg: LayerSurfaceConfigure, _: u32) {
        let w = NonZeroU32::new(cfg.new_size.0).map_or(1366, NonZeroU32::get);
        let h = NonZeroU32::new(cfg.new_size.1).map_or(768, NonZeroU32::get);
        if !self.configured || (w, h) != (self.width, self.height) {
            self.width = w;
            self.height = h;
            self.scene.resize(&mut self.painter, w as usize, h as usize);
            self.configured = true;
            self.draw(qh);
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm { &mut self.shm }
}

delegate_registry!(App);
impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState { &mut self.registry_state }
    registry_handlers![OutputState];
}
smithay_client_toolkit::delegate_dispatch2!(App);

extern "C" {
    fn malloc_trim(pad: usize) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn signal(sig: i32, handler: usize) -> usize;
    fn setsid() -> i32;
    fn geteuid() -> u32;
}

static STOP: AtomicBool = AtomicBool::new(false);
static KEEP_THEME: AtomicBool = AtomicBool::new(false); // SIGHUP: salir sin restaurar waybar/fuzzel
extern "C" fn on_signal(_: i32) { STOP.store(true, Ordering::SeqCst); }
extern "C" fn on_hup(_: i32) { KEEP_THEME.store(true, Ordering::SeqCst); STOP.store(true, Ordering::SeqCst); }

/// El pidfile vive en $XDG_RUNTIME_DIR (privado del usuario, 0700). Sin respaldo a /tmp.
fn pidfile() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        Some(d) if d.is_absolute() => d.join("matrix-wayland.pid"),
        _ => { eprintln!("XDG_RUNTIME_DIR no esta definido; no se puede gestionar el pidfile"); std::process::exit(1) }
    }
}

fn write_pidfile(pid: u32) -> bool {
    let path = pidfile();
    // nunca seguir un symlink ni reutilizar un fichero ajeno
    if let Ok(m) = std::fs::symlink_metadata(&path) {
        if m.file_type().is_symlink() || m.uid() != unsafe { geteuid() } {
            eprintln!("pidfile sospechoso ({}); no se toca", path.display());
            return false;
        }
    }
    std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600)
        .open(&path)
        .and_then(|mut f| { use std::io::Write; write!(f, "{pid}") })
        .is_ok()
}

/// Borra el pidfile solo si pertenece a este proceso (no el de otra instancia).
fn remove_own_pidfile() {
    let path = pidfile();
    if std::fs::read_to_string(&path).ok().and_then(|t| t.trim().parse::<u32>().ok()) == Some(std::process::id()) {
        let _ = std::fs::remove_file(&path);
    }
}

/// PID del proceso en marcha, si el pidfile es nuestro, valido y no esta obsoleto.
fn running_pid() -> Option<i32> {
    let path = pidfile();
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if meta.file_type().is_symlink() || meta.uid() != unsafe { geteuid() } { return None; }
    let pid: i32 = std::fs::read_to_string(&path).ok()?.trim().parse().ok()?;
    if pid <= 1 { return None; }
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    if comm.trim() == "matrix-wayland" { Some(pid) } else { None }
}

/// keep_theme = true (restart): el proceso sale sin restaurar los colores, para que el\n/// siguiente arranque los aplique sin una recarga doble de waybar entre medias.
fn cmd_stop(keep_theme: bool) -> bool {
    match running_pid() {
        None => { eprintln!("matrix-wayland no esta en marcha"); false }
        Some(pid) => {
            unsafe { kill(pid, if keep_theme { 1 } else { 15 }) };
            for _ in 0..50 { // hasta 5 s para una salida limpia
                if !std::path::Path::new(&format!("/proc/{pid}")).exists() { println!("detenido"); return true; }
                std::thread::sleep(Duration::from_millis(100));
            }
            eprintln!("no salio en 5 s; usa: kill -9 {pid}");
            false
        }
    }
}

// ---------- opciones persistentes ----------
// start/restart recuerdan las ultimas opciones (escena, color, fps, --waybar...). Lo que
// escribas gana sobre lo recordado; --no-X apaga una bandera y --reset olvida todo.
// Se guardan en $XDG_STATE_HOME/matrix-wayland/options.
const VALUE_OPTS: [&str; 7] = ["-c", "-s", "--fps", "--size", "--font", "--scene", "--scale"];
const FLAG_OPTS: [&str; 3] = ["--waybar", "--fuzzel", "--hud"];

fn state_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("state")))?;
    Some(base.join("matrix-wayland").join("options"))
}

fn load_options() -> std::collections::BTreeMap<String, String> {
    let mut t = state_file().and_then(|p| std::fs::read_to_string(p).ok());
    if t.is_none() { // migracion: version anterior, fichero `integrations` con "waybar"/"fuzzel"
        t = state_file().and_then(|p| std::fs::read_to_string(p.with_file_name("integrations")).ok())
            .map(|old| old.lines().map(|l| format!("--{}\t\n", l.trim())).collect());
    }
    let t = t.unwrap_or_default();
    t.lines().filter(|l| !l.trim().is_empty()).map(|l| {
        let (k, v) = l.split_once('\t').unwrap_or((l, ""));
        (k.to_string(), v.to_string())
    }).filter(|(k, _)| VALUE_OPTS.contains(&k.as_str()) || FLAG_OPTS.contains(&k.as_str())).collect()
}

fn save_options(o: &std::collections::BTreeMap<String, String>) {
    let Some(p) = state_file() else { return };
    if let Some(d) = p.parent() { let _ = std::fs::create_dir_all(d); }
    let body: String = o.iter().map(|(k, v)| format!("{k}\t{v}\n")).collect();
    let _ = atomic_write(&p, &body);
}

/// Combina las opciones dadas con las recordadas y devuelve la lista completa y explicita
/// que recibe el proceso.
fn resolve_options(opts: &[String]) -> Vec<String> {
    let mut o = load_options();
    let (old_waybar, old_fuzzel) = (o.contains_key("--waybar"), o.contains_key("--fuzzel"));
    let mut rest = Vec::new();
    let mut it = opts.iter();
    while let Some(a) = it.next() {
        if a == "--reset" { o.clear(); }
        else if VALUE_OPTS.contains(&a.as_str()) {
            match it.next() { Some(v) => { o.insert(a.clone(), v.clone()); } None => rest.push(a.clone()) }
        }
        else if FLAG_OPTS.contains(&a.as_str()) { o.insert(a.clone(), String::new()); }
        else if let Some(f) = a.strip_prefix("--no-").map(|f| format!("--{f}")).filter(|f| FLAG_OPTS.contains(&f.as_str())) { o.remove(&f); }
        else { rest.push(a.clone()); } // desconocida: que la rechace el proceso con el uso
    }
    save_options(&o);
    // si se apaga una integracion, el proceso anterior (restart) pudo dejar su color: restaurar
    if old_waybar && !o.contains_key("--waybar") { waybar_apply(None); }
    if old_fuzzel && !o.contains_key("--fuzzel") { fuzzel_apply(None); }
    for (k, v) in &o { rest.push(k.clone()); if !v.is_empty() { rest.push(v.clone()); } }
    rest
}

/// Aplica (o quita) el color de waybar/fuzzel sin ninguna escena en marcha.
fn cmd_theme(opts: &[String]) {
    if let Some(pid) = running_pid() {
        eprintln!("hay una escena en marcha (pid {pid}); usa `matrix-wayland restart -c COLOR` para cambiar el color");
        std::process::exit(1);
    }
    if opts.iter().any(|o| o == "--off") {
        waybar_apply(None);
        fuzzel_apply(None);
        println!("colores de waybar y fuzzel restaurados");
        return;
    }
    let resolved = resolve_options(opts);
    let (mut color, mut wb, mut fz) = ("green".to_string(), false, false);
    let mut it = resolved.iter();
    while let Some(o) = it.next() {
        match o.as_str() {
            "-c" => if let Some(v) = it.next() { color = v.clone(); },
            "--waybar" => wb = true,
            "--fuzzel" => fz = true,
            _ => {}
        }
    }
    if !wb && !fz { eprintln!("nada que aplicar: usa --waybar y/o --fuzzel (se recuerdan para la proxima vez)"); std::process::exit(1); }
    let rgb = if color.to_lowercase() == "rainbow" { None } else { parse_color(&color) };
    if rgb.is_none() {
        eprintln!("el color {color:?} no sirve para el tema (use un nombre neon o #rrggbb; rainbow no tiene un color unico)");
        std::process::exit(2);
    }
    if wb { waybar_apply(rgb); }
    if fz { fuzzel_apply(rgb); }
    println!("tema {color} aplicado{}{}", if wb { " a waybar" } else { "" }, if fz { " y fuzzel" } else { "" });
}

fn cmd_start(opts: &[String]) {
    if let Some(pid) = running_pid() { eprintln!("ya esta en marcha (pid {pid}); usa `matrix-wayland restart`"); std::process::exit(1); }
    let opts = &resolve_options(opts);
    let exe = std::env::current_exe().expect("exe");
    let mut c = Command::new(exe);
    c.arg("--foreground").args(opts).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    unsafe { c.pre_exec(|| { setsid(); Ok(()) }); }
    let mut child = c.spawn().expect("no se pudo lanzar");
    if !write_pidfile(child.id()) { // ya, no cuando termine de arrancar
        let _ = child.kill(); // sin pidfile no habria forma de pararlo con `stop`
        let _ = child.wait();
        eprintln!("no se pudo registrar el pidfile; cancelado");
        std::process::exit(1);
    }
    // si falla al arrancar (opcion/color invalido, sin layer-shell...) lo decimos
    std::thread::sleep(Duration::from_millis(1500));
    if let Ok(Some(_)) = child.try_wait() {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() { use std::io::Read; let _ = e.read_to_string(&mut err); }
        eprint!("{err}");
        let _ = std::fs::remove_file(pidfile());
        std::process::exit(1);
    }
    println!("iniciado (pid {})", child.id());
}

// ---------- integracion con waybar (opt-in: --waybar) ----------
fn waybar_css_path() -> Option<PathBuf> {
    Some(config_dir()?.join("waybar").join("matrix-colors.css"))
}

fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

/// Escritura atomica (tmp + rename), y nunca sobre un symlink.
fn atomic_write(path: &std::path::Path, body: &str) -> Result<(), String> {
    if let Ok(m) = std::fs::symlink_metadata(path) {
        if m.file_type().is_symlink() { return Err(format!("{} es un symlink; no se toca", path.display())); }
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn hex(c: [u8; 3]) -> String { format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2]) }

/// Escribe el CSS de GTK con los colores (None = solo un comentario, para que el
/// style.css use sus colores por defecto).
fn write_waybar_css(accent: Option<[u8; 3]>) -> Result<(), String> {
    let path = waybar_css_path().ok_or("no se pudo resolver ~/.config/waybar")?;
    let body = match accent {
        Some(c) => format!(
            "/* generado por matrix-wayland */\n\
             @define-color matrix_accent #{h};\n\
             @define-color matrix_fg #{h};\n\
             @define-color matrix_bar_bg #000000;\n\
             @define-color matrix_mod_bg #000000;\n\
             @define-color matrix_mod_hover rgba({r},{g},{b},0.25);\n",
            h = hex(c), r = c[0], g = c[1], b = c[2]),
        None => "/* generado por matrix-wayland: inactivo, se usa el color por defecto del style.css */\n".to_string(),
    };
    atomic_write(&path, &body)
}

/// Tema de fuzzel (se lee al abrirlo: no hace falta recargar nada).
fn write_fuzzel_ini(accent: Option<[u8; 3]>) -> Result<(), String> {
    let path = config_dir().ok_or("no se pudo resolver ~/.config")?.join("fuzzel").join("matrix-colors.ini");
    let body = match accent {
        Some(c) => {
            let mix = |f: f64| hex([(c[0] as f64 * f) as u8, (c[1] as f64 * f) as u8, (c[2] as f64 * f) as u8]);
            let lite = hex([(c[0] as f64 + (255.0 - c[0] as f64) * 0.75) as u8, (c[1] as f64 + (255.0 - c[1] as f64) * 0.75) as u8, (c[2] as f64 + (255.0 - c[2] as f64) * 0.75) as u8]);
            let a = hex(c);
            format!(
                "# generado por matrix-wayland\n[colors]\nbackground=000000ff\ntext={a}ff\nprompt={a}ff\ninput={a}ff\n\
                 placeholder={p}ff\nmatch={lite}ff\nselection-match={lite}ff\nselection={a}40\nselection-text={a}ff\n\
                 counter={p}ff\nborder={a}ff\n",
                p = mix(0.55))
        }
        None => "# generado por matrix-wayland: inactivo, se usan los colores por defecto\n".to_string(),
    };
    atomic_write(&path, &body)
}

fn fuzzel_apply(accent: Option<[u8; 3]>) {
    if let Err(e) = write_fuzzel_ini(accent) { eprintln!("fuzzel: {e}"); }
}

/// SIGUSR2 hace que waybar recargue config y estilo. Solo a procesos nuestros.
fn reload_waybar() {
    let me = unsafe { geteuid() };
    let Ok(rd) = std::fs::read_dir("/proc") else { return };
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<i32>().ok()) else { continue };
        let owned = e.metadata().map(|m| m.uid() == me).unwrap_or(false);
        let is_waybar = std::fs::read_to_string(format!("/proc/{pid}/comm")).map(|c| c.trim() == "waybar").unwrap_or(false);
        if owned && is_waybar { unsafe { kill(pid, 12) }; }
    }
}

fn waybar_apply(accent: Option<[u8; 3]>) {
    match write_waybar_css(accent) {
        Ok(()) => reload_waybar(),
        Err(e) => eprintln!("waybar: {e}"),
    }
}

fn usage() -> ! {
    eprintln!("uso: matrix-wayland [start|stop|restart|status] [opciones]
  -c COLOR          green cyan magenta pink blue purple orange yellow red | #rrggbb | rainbow
  --scene NOMBRE    matrix (por defecto) | galaga | pacman | snake | tron
  -s CARACTERES     (matrix) mixed katakana binary digits hex
  --fps N           fotogramas por segundo (matrix 15, el resto 30)
  --size PX         (matrix) tamano del glifo      --font RUTA   fuente con katakana
  --scale N         (galaga) pixeles por pixel de sprite; (pacman, snake, tron) pixeles por casilla (por defecto, segun la pantalla)
  --hud             (galaga, pacman, snake, tron) puntuacion, nivel/ronda y vidas
  --waybar / --fuzzel      el color tambien en waybar / fuzzel
  --no-waybar --no-fuzzel --no-hud   apagan una bandera
  --reset           olvida las opciones recordadas
Las opciones se recuerdan entre arranques; lo que escribas gana. --foreground: primer plano.
  stop --keep-theme  apaga la escena y deja los colores de waybar/fuzzel
  theme [-c COLOR]   aplica el color a waybar/fuzzel sin escena (theme --off los restaura)");
    std::process::exit(2)
}

fn main() {
    let (mut color, mut set, mut fps, mut size) = ("green".to_string(), "mixed".to_string(), 0.0f64, 16.0f32);
    let (mut scene_name, mut scale, mut hud) = ("matrix".to_string(), 0i32, false);
    let mut font_path = "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc".to_string();
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let mut foreground = false;
    match a.first().map(String::as_str) {
        Some("stop") => { let keep = a.iter().any(|x| x == "--keep-theme"); std::process::exit(if cmd_stop(keep) { 0 } else { 1 }) }
        Some("theme") => { a.remove(0); cmd_theme(&a); return; }
        Some("status") => match running_pid() {
            Some(pid) => { println!("en marcha (pid {pid})"); std::process::exit(0) }
            None => { println!("detenido"); std::process::exit(3) }
        },
        Some("restart") => { let _ = cmd_stop(true); a.remove(0); cmd_start(&a); return; }
        Some("start") => { a.remove(0); cmd_start(&a); return; }
        Some("-h") | Some("--help") => usage(),
        _ => {}
    }
    if a.first().map(String::as_str) == Some("--foreground") { foreground = true; a.remove(0); }
    if !foreground { cmd_start(&a); return; }
    let (mut waybar, mut fuzzel) = (false, false);
    let mut i = 0;
    while i < a.len() {
        if a[i] == "--waybar" { waybar = true; i += 1; continue; }
        if a[i] == "--fuzzel" { fuzzel = true; i += 1; continue; }
        if a[i] == "--hud" { hud = true; i += 1; continue; }
        let val = a.get(i + 1).cloned();
        match a[i].as_str() {
            "-c" => color = val.unwrap_or_else(|| usage()),
            "-s" => set = val.unwrap_or_else(|| usage()),
            "--fps" => fps = val.and_then(|v| v.parse().ok()).unwrap_or_else(|| usage()),
            "--size" => size = val.and_then(|v| v.parse().ok()).unwrap_or_else(|| usage()),
            "--font" => font_path = val.unwrap_or_else(|| usage()),
            "--scene" => scene_name = val.unwrap_or_else(|| usage()),
            "--scale" => scale = val.and_then(|v| v.parse().ok()).unwrap_or_else(|| usage()),
            _ => usage(),
        }
        i += 2;
    }
    if !["matrix", "galaga", "pacman", "snake", "tron"].contains(&scene_name.as_str()) { eprintln!("escena desconocida: {scene_name:?} (matrix|galaga|pacman|snake|tron)"); std::process::exit(2) }
    if fps <= 0.0 { fps = if scene_name == "matrix" { 15.0 } else { 30.0 }; }
    let pals: Vec<Pal> = if color.to_lowercase() == "rainbow" {
        NEON.iter().map(|(_, c)| make_pal(*c)).collect()
    } else {
        match parse_color(&color) {
            Some(c) => vec![make_pal(c)],
            None => { eprintln!("color invalido: {color:?}"); std::process::exit(2) }
        }
    };
    if waybar {
        if color.to_lowercase() == "rainbow" { eprintln!("waybar: rainbow no tiene un color unico; se ignora --waybar"); waybar_apply(None); waybar = false; }
        else { waybar_apply(parse_color(&color)); }
    }
    if fuzzel {
        if color.to_lowercase() == "rainbow" { eprintln!("fuzzel: rainbow no tiene un color unico; se ignora --fuzzel"); fuzzel_apply(None); fuzzel = false; }
        else { fuzzel_apply(parse_color(&color)); }
    }
    let chars = charset(&set);
    let (painter, scene): (Painter, Box<dyn Scene>) = if scene_name == "galaga" {
        (Painter::plain(), Box::new(galaga::Galaga::new(pals, scale, hud)))
    } else if scene_name == "pacman" {
        (Painter::plain(), Box::new(pacman::Pacman::new(pals, scale, hud)))
    } else if scene_name == "snake" {
        (Painter::plain(), Box::new(snake::Snake::new(pals, scale, hud)))
    } else if scene_name == "tron" {
        (Painter::plain(), Box::new(tron::Tron::new(pals, scale, hud)))
    } else {
        let data = std::fs::read(&font_path).unwrap_or_else(|e| { eprintln!("no se pudo leer la fuente {font_path}: {e}"); std::process::exit(1) });
        let font = fontdue::Font::from_bytes(data, fontdue::FontSettings { scale: size, ..Default::default() })
            .unwrap_or_else(|e| { eprintln!("fuente invalida: {e}"); std::process::exit(1) });
        let painter = Painter::new(&font, &chars, size);
        drop(font); // ya estan rasterizados los glifos; libera ~270 MB de la fuente CJK
        unsafe { malloc_trim(0); }
        (painter, Box::new(Matrix::new(pals, chars)))
    };
    unsafe { signal(2, on_signal as *const () as usize); signal(15, on_signal as *const () as usize); signal(1, on_hup as *const () as usize); }

    let conn = Connection::connect_to_env().expect("no hay compositor Wayland");
    let (globals, event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor no disponible");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("wlr-layer-shell no disponible");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm no disponible");

    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Background, Some("matrix-wayland"), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(0, 0);
    layer.commit();

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new(1366 * 768 * 4, &shm).expect("pool"),
        shm,
        layer,
        width: 0,
        height: 0,
        configured: false,
        frame_pending: false,
        exit: false,
        painter,
        scene,
        dt: (1.0 / fps.max(1.0)) as f32,
    };

    let mut event_loop: EventLoop<App> = EventLoop::try_new().unwrap();
    let handle = event_loop.handle();
    WaylandSource::new(conn.clone(), event_queue).insert(handle.clone()).unwrap();
    let frame = Duration::from_secs_f64(1.0 / fps.max(1.0));
    let tqh = qh.clone();
    handle
        .insert_source(Timer::from_duration(frame), move |_, _, app| {
            if app.configured && !app.frame_pending {
                app.draw(&tqh);
            }
            TimeoutAction::ToDuration(frame)
        })
        .unwrap();

    while !app.exit && !STOP.load(Ordering::SeqCst) {
        event_loop.dispatch(Some(Duration::from_millis(250)), &mut app).unwrap();
    }
    remove_own_pidfile();
    if !KEEP_THEME.load(Ordering::SeqCst) { // un restart deja los colores para el siguiente arranque
        if waybar { waybar_apply(None); } // vuelve al color por defecto
        if fuzzel { fuzzel_apply(None); }
    }
}
