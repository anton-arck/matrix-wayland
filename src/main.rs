//! Efecto Matrix como fondo de pantalla Wayland (wlr-layer-shell + wl_shm).
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

    fn step(&mut self, p: &mut Painter) {
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
    matrix: Matrix,
}

impl App {
    fn draw(&mut self, qh: &QueueHandle<Self>) {
        self.matrix.step(&mut self.painter);
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
            let (cols, rows) = self.painter.resize(w as usize, h as usize);
            self.matrix.reset(cols, rows);
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
extern "C" fn on_signal(_: i32) { STOP.store(true, Ordering::SeqCst); }

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

fn cmd_stop() -> bool {
    match running_pid() {
        None => { eprintln!("matrix-wayland no esta en marcha"); false }
        Some(pid) => {
            unsafe { kill(pid, 15) };
            for _ in 0..50 { // hasta 5 s para una salida limpia
                if !std::path::Path::new(&format!("/proc/{pid}")).exists() { println!("detenido"); return true; }
                std::thread::sleep(Duration::from_millis(100));
            }
            eprintln!("no salio en 5 s; usa: kill -9 {pid}");
            false
        }
    }
}

fn cmd_start(opts: &[String]) {
    if let Some(pid) = running_pid() { eprintln!("ya esta en marcha (pid {pid}); usa `matrix-wayland restart`"); std::process::exit(1); }
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

fn usage() -> ! {
    eprintln!("uso: matrix-wayland [start|stop|restart|status] [-c color|#rrggbb|rainbow] [-s binary|digits|hex|katakana|mixed] [--fps N] [--size PX] [--font RUTA]
  sin subcomando = start. --foreground lo deja en primer plano.");
    std::process::exit(2)
}

fn main() {
    let (mut color, mut set, mut fps, mut size) = ("green".to_string(), "mixed".to_string(), 15.0f64, 16.0f32);
    let mut font_path = "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc".to_string();
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let mut foreground = false;
    match a.first().map(String::as_str) {
        Some("stop") => std::process::exit(if cmd_stop() { 0 } else { 1 }),
        Some("status") => match running_pid() {
            Some(pid) => { println!("en marcha (pid {pid})"); std::process::exit(0) }
            None => { println!("detenido"); std::process::exit(3) }
        },
        Some("restart") => { let _ = cmd_stop(); a.remove(0); cmd_start(&a); return; }
        Some("start") => { a.remove(0); cmd_start(&a); return; }
        Some("-h") | Some("--help") => usage(),
        _ => {}
    }
    if a.first().map(String::as_str) == Some("--foreground") { foreground = true; a.remove(0); }
    if !foreground { cmd_start(&a); return; }
    let mut i = 0;
    while i < a.len() {
        let val = a.get(i + 1).cloned();
        match a[i].as_str() {
            "-c" => color = val.unwrap_or_else(|| usage()),
            "-s" => set = val.unwrap_or_else(|| usage()),
            "--fps" => fps = val.and_then(|v| v.parse().ok()).unwrap_or_else(|| usage()),
            "--size" => size = val.and_then(|v| v.parse().ok()).unwrap_or_else(|| usage()),
            "--font" => font_path = val.unwrap_or_else(|| usage()),
            _ => usage(),
        }
        i += 2;
    }
    let pals: Vec<Pal> = if color.to_lowercase() == "rainbow" {
        NEON.iter().map(|(_, c)| make_pal(*c)).collect()
    } else {
        match parse_color(&color) {
            Some(c) => vec![make_pal(c)],
            None => { eprintln!("color invalido: {color:?}"); std::process::exit(2) }
        }
    };
    let chars = charset(&set);
    let data = std::fs::read(&font_path).unwrap_or_else(|e| { eprintln!("no se pudo leer la fuente {font_path}: {e}"); std::process::exit(1) });
    let font = fontdue::Font::from_bytes(data, fontdue::FontSettings { scale: size, ..Default::default() })
        .unwrap_or_else(|e| { eprintln!("fuente invalida: {e}"); std::process::exit(1) });
    let painter = Painter::new(&font, &chars, size);
    drop(font);
    unsafe { malloc_trim(0); }
    // ya estan rasterizados los glifos; libera ~270 MB de la fuente CJK

    unsafe { signal(2, on_signal as *const () as usize); signal(15, on_signal as *const () as usize); }

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
        matrix: Matrix::new(pals, chars),
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
    let _ = std::fs::remove_file(pidfile());
}
