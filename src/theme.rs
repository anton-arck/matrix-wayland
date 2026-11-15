//! Temas de color para otros programas (waybar, fuzzel, kitty, FireDragon).
//!
//! Cada destino es una entrada de `TARGETS`: escribe un fichero de colores generado y, si el
//! programa lo admite, le avisa para que lo recargue. Inactivo (`None`) deja el fichero
//! con solo un comentario, de modo que vuelven los colores propios del programa.
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::{atomic_write, config_dir, geteuid, hex, kill, Accent, NEON, RAINBOW_ORDER};

pub struct Target {
    /// Nombre para los mensajes.
    pub name: &'static str,
    /// Bandera de la linea de comandos (`--waybar`...), que tambien se recuerda entre arranques.
    pub flag: &'static str,
    write: fn(Option<Accent>) -> Result<(), String>,
    /// Avisa al programa de que hay colores nuevos (si no puede, no hace nada).
    reload: fn(),
}

pub const TARGETS: [Target; 7] = [
    Target { name: "waybar", flag: "--waybar", write: write_waybar_css, reload: reload_waybar },
    Target { name: "fuzzel", flag: "--fuzzel", write: write_fuzzel_ini, reload: || {} }, // se lee al abrirlo
    Target { name: "kitty", flag: "--kitty", write: write_kitty_conf, reload: reload_kitty },
    Target { name: "firedragon", flag: "--firedragon", write: write_firedragon_css, reload: || {} }, // solo al arrancar
    Target { name: "sway", flag: "--sway", write: write_sway_conf, reload: reload_sway },
    Target { name: "gtklock", flag: "--gtklock", write: write_gtklock_css, reload: || {} }, // se lee al bloquear
    Target { name: "mako", flag: "--mako", write: write_mako_conf, reload: reload_mako },
];

impl Target {
    pub fn apply(&self, accent: Option<Accent>) {
        match (self.write)(accent) {
            Ok(()) => (self.reload)(),
            Err(e) => eprintln!("{}: {e}", self.name),
        }
    }
}

pub fn by_flag(flag: &str) -> Option<&'static Target> { TARGETS.iter().find(|t| t.flag == flag) }

/// Envia `sig` a los procesos del usuario cuyo `comm` sea `comm` (nunca a los de otros usuarios).
fn signal_procs(comm: &str, sig: i32) {
    let me = unsafe { geteuid() };
    let Ok(rd) = std::fs::read_dir("/proc") else { return };
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<i32>().ok()) else { continue };
        let owned = e.metadata().map(|m| m.uid() == me).unwrap_or(false);
        let matches = std::fs::read_to_string(format!("/proc/{pid}/comm")).map(|c| c.trim() == comm).unwrap_or(false);
        if owned && matches && handles_signal(pid, sig) { unsafe { kill(pid, sig) }; }
    }
}

/// Un proceso recien lanzado aun no instalo su manejador de `sig`: la accion por defecto de SIGUSR1/2 es
/// terminar, asi que enviarla en el arranque (autostart de sway) mata waybar o kitty. Solo se envia si
/// el proceso ya captura (SigCgt) o ignora (SigIgn) esa senal.
fn handles_signal(pid: i32, sig: i32) -> bool {
    let Ok(st) = std::fs::read_to_string(format!("/proc/{pid}/status")) else { return false };
    let bit = 1u64 << (sig - 1);
    ["SigCgt:", "SigIgn:"].iter().any(|k| {
        st.lines().find_map(|l| l.strip_prefix(k))
            .and_then(|h| u64::from_str_radix(h.trim(), 16).ok())
            .is_some_and(|m| m & bit != 0)
    })
}

/// SIGUSR2 hace que waybar recargue config y estilo.
fn reload_waybar() { signal_procs("waybar", 12); }
/// SIGUSR1 hace que kitty relea su configuracion (y con ella el fichero de colores).
fn reload_kitty() { signal_procs("kitty", 10); }

/// Ejecuta un programa de control (`swaymsg reload`, `makoctl reload`) sin ruido; si no esta, no pasa nada.
fn run_quiet(cmd: &str, args: &[&str]) {
    let _ = std::process::Command::new(cmd).args(args)
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
}
/// Con colores: se aplican en caliente con `swaymsg client.*...`, sin recargar (un `reload` hace que sway
/// reinicie el `swaybar_command`, y waybar puede perderse). Sin colores hace falta recargar para recuperar
/// los del tema propio; entonces se vigila que waybar siga vivo y, si no, se vuelve a lanzar.
fn reload_sway() {
    let live = config_dir().and_then(|d| std::fs::read_to_string(d.join("sway").join("matrix-colors")).ok()).unwrap_or_default();
    let cmds: Vec<&str> = live.lines().filter(|l| l.starts_with("client.")).collect();
    if !cmds.is_empty() { return run_quiet("swaymsg", &[&cmds.join("; ")]); }
    let had_waybar = proc_running("waybar");
    run_quiet("swaymsg", &["reload"]);
    if had_waybar {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        if !proc_running("waybar") {
            use std::os::unix::process::CommandExt;
            let _ = std::process::Command::new("waybar").process_group(0)
                .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
        }
    }
}
fn proc_running(comm: &str) -> bool {
    let me = unsafe { geteuid() };
    std::fs::read_dir("/proc").into_iter().flatten().flatten().any(|e| {
        e.file_name().to_str().is_some_and(|n| n.parse::<i32>().is_ok())
            && e.metadata().map(|m| m.uid() == me).unwrap_or(false)
            && std::fs::read_to_string(e.path().join("comm")).map(|c| c.trim() == comm).unwrap_or(false)
    })
}
fn reload_mako() { run_quiet("makoctl", &["reload"]); }

fn shade(c: [u8; 3], f: f64) -> [u8; 3] { [(c[0] as f64 * f) as u8, (c[1] as f64 * f) as u8, (c[2] as f64 * f) as u8] }
fn lite(c: [u8; 3]) -> [u8; 3] { let l = |v: u8| (v as f64 + (255.0 - v as f64) * 0.75) as u8; [l(c[0]), l(c[1]), l(c[2])] }

/// Paleta comun para los programas con tema completo (kitty, FireDragon).
struct Scheme { fg: String, hi: String, url: String, dim: String, mid: String }
fn scheme(accent: Accent) -> Scheme {
    match accent {
        Accent::One(c) => Scheme { fg: hex(c), hi: hex(c), url: hex(lite(c)), dim: hex(shade(c, 0.28)), mid: hex(shade(c, 0.55)) },
        Accent::Rainbow => {
            let n = |i: usize| NEON[RAINBOW_ORDER[i % RAINBOW_ORDER.len()]].1;
            Scheme { fg: hex(n(0)), hi: hex(n(1)), url: hex(n(2)), dim: hex(shade(n(3), 0.28)), mid: hex(n(4)) }
        }
    }
}

/// Escribe el CSS de GTK con los colores (None = solo un comentario, para que el
/// style.css use sus colores por defecto).
fn write_waybar_css(accent: Option<Accent>) -> Result<(), String> {
    let path = config_dir().ok_or("no se pudo resolver ~/.config")?.join("waybar").join("matrix-colors.css");
    let body = match accent {
        Some(Accent::One(c)) => format!(
            "/* generado por matrix-wayland */\n\
             @define-color matrix_accent #{h};\n\
             @define-color matrix_fg #{h};\n\
             @define-color matrix_bar_bg #000000;\n\
             @define-color matrix_mod_bg #000000;\n\
             @define-color matrix_mod_hover rgba({r},{g},{b},0.25);\n",
            h = hex(c), r = c[0], g = c[1], b = c[2]),
        Some(Accent::Rainbow) => {
            // cada modulo (y cada boton de workspace) con un neon distinto, alternados
            let mut css = String::from("/* generado por matrix-wayland: rainbow */\n\
                @define-color matrix_accent #ffffff;\n@define-color matrix_fg #ffffff;\n\
                @define-color matrix_bar_bg #000000;\n@define-color matrix_mod_bg #000000;\n\
                @define-color matrix_mod_hover rgba(255,255,255,0.18);\n");
            let mut k = 0;
            let mut next = || { let c = hex(NEON[RAINBOW_ORDER[k % RAINBOW_ORDER.len()]].1); k += 1; c };
            css += &format!("#custom-launcher {{ color: #{}; }}\n", next());
            for n in 1..=10 { css += &format!("#workspaces button:nth-child({n}) {{ color: #{}; }}\n", next()); }
            for sel in ["#window", "#network", "#privacy", "#custom-updates", "#custom-keyboard-layout", "#cpu", "#temperature",
                        "#memory", "#wireplumber", "#battery", "#custom-settings", "#clock", "#custom-power"] {
                css += &format!("{sel} {{ color: #{}; }}\n", next());
            }
            css
        }
        None => "/* generado por matrix-wayland: inactivo, se usa el color por defecto del style.css */\n".to_string(),
    };
    atomic_write(&path, &body)
}

/// Tema de fuzzel (se lee al abrirlo: no hace falta recargar nada).
fn write_fuzzel_ini(accent: Option<Accent>) -> Result<(), String> {
    let path = config_dir().ok_or("no se pudo resolver ~/.config")?.join("fuzzel").join("matrix-colors.ini");
    let body = match accent {
        Some(Accent::One(c)) => {
            let mix = |f: f64| hex([(c[0] as f64 * f) as u8, (c[1] as f64 * f) as u8, (c[2] as f64 * f) as u8]);
            let lite = hex([(c[0] as f64 + (255.0 - c[0] as f64) * 0.75) as u8, (c[1] as f64 + (255.0 - c[1] as f64) * 0.75) as u8, (c[2] as f64 + (255.0 - c[2] as f64) * 0.75) as u8]);
            let a = hex(c);
            format!(
                "# generado por matrix-wayland\n[colors]\nbackground=000000ff\ntext={a}ff\nprompt={a}ff\ninput={a}ff\n\
                 placeholder={p}ff\nmatch={lite}ff\nselection-match={lite}ff\nselection={a}40\nselection-text={a}ff\n\
                 counter={p}ff\nborder={a}ff\n",
                p = mix(0.55))
        }
        Some(Accent::Rainbow) => {
            // un neon distinto por elemento (fuzzel pinta todas las filas con el mismo color de texto)
            let c = |i: usize| hex(NEON[RAINBOW_ORDER[i % RAINBOW_ORDER.len()]].1);
            format!(
                "# generado por matrix-wayland: rainbow\n[colors]\nbackground=000000ff\ntext={}ff\nprompt={}ff\ninput={}ff\n\
                 placeholder={}ff\nmatch={}ff\nselection-match={}ff\nselection={}40\nselection-text={}ff\n\
                 counter={}ff\nborder={}ff\n",
                c(0), c(1), c(2), c(3), c(4), c(5), c(6), c(7), c(8), c(1))
        }
        None => "# generado por matrix-wayland: inactivo, se usan los colores por defecto\n".to_string(),
    };
    atomic_write(&path, &body)
}


// ---------- kitty (--kitty) ----------
// kitty.conf debe tener `include matrix-colors.conf` DESPUES de sus propios colores (gana el ultimo).
// Solo cambia fondo, texto, cursor, seleccion, enlaces y pestanas; la paleta ANSI queda como esta.
fn write_kitty_conf(accent: Option<Accent>) -> Result<(), String> {
    let path = config_dir().ok_or("no se pudo resolver ~/.config")?.join("kitty").join("matrix-colors.conf");
    let body = match accent {
        Some(a) => {
            let s = scheme(a);
            format!(
                "# generado por matrix-wayland\nbackground #000000\nforeground #{fg}\ncursor #{hi}\ncursor_text_color #000000\n\
                 selection_background #{dim}\nselection_foreground #{url}\nurl_color #{url}\ntab_bar_background #000000\n\
                 active_tab_background #{hi}\nactive_tab_foreground #000000\n\
                 inactive_tab_background #000000\ninactive_tab_foreground #{mid}\n",
                fg = s.fg, hi = s.hi, dim = s.dim, url = s.url, mid = s.mid)
        }
        None => "# generado por matrix-wayland: inactivo, se usan los colores de kitty.conf\n".to_string(),
    };
    atomic_write(&path, &body)
}

// ---------- FireDragon (--firedragon) ----------
// Firefox solo lee el CSS al arrancar: hay que reiniciar el navegador para ver el color nuevo.
// userChrome.css y userContent.css deben importar los ficheros generados (ver el README).
fn firedragon_chrome_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("no se pudo resolver $HOME")?;
    let base = home.join(".firedragon");
    let rd = std::fs::read_dir(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    let mut profiles: Vec<PathBuf> = rd.flatten().map(|e| e.path())
        .filter(|p| p.is_dir() && p.to_str().is_some_and(|s| s.ends_with(".default-release"))).collect();
    profiles.sort();
    let p = profiles.into_iter().next().ok_or("no hay perfil *.default-release en ~/.firedragon")?;
    Ok(p.join("chrome"))
}

fn write_firedragon_css(accent: Option<Accent>) -> Result<(), String> {
    let dir = firedragon_chrome_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let write = |name: &str, body: &str| -> Result<(), String> { atomic_write(&Path::new(&dir).join(name), body) };
    let Some(a) = accent else {
        let off = "/* generado por matrix-wayland: inactivo, se usan los colores del navegador */\n";
        write("matrix-colors.css", off)?;
        return write("matrix-content-colors.css", off);
    };
    let s = scheme(a);
    let (fg, hi, url, dim, mid) = (&s.fg, &s.hi, &s.url, &s.dim, &s.mid);
    write("matrix-colors.css", &format!(
        "/* generado por matrix-wayland (interfaz del navegador) */\n\
         :root {{\n\
         \x20 --matrix-fg: #{fg}; --matrix-bg: #000000; --matrix-dim: #{dim}; --matrix-mid: #{mid};\n\
         \x20 --toolbar-bgcolor: #000000 !important; --toolbar-color: #{fg} !important;\n\
         \x20 --lwt-accent-color: #000000 !important; --lwt-text-color: #{fg} !important;\n\
         \x20 --toolbar-field-background-color: #000000 !important; --toolbar-field-color: #{fg} !important;\n\
         \x20 --toolbar-field-border-color: #{mid} !important; --toolbar-field-focus-background-color: #000000 !important;\n\
         \x20 --toolbar-field-focus-border-color: #{hi} !important; --focus-outline-color: #{hi} !important;\n\
         \x20 --tab-selected-bgcolor: #{dim} !important; --tab-selected-textcolor: #{fg} !important;\n\
         \x20 --tab-loading-fill: #{hi} !important; --toolbarbutton-icon-fill: #{fg} !important;\n\
         \x20 --arrowpanel-background: #000000 !important; --arrowpanel-color: #{fg} !important;\n\
         \x20 --arrowpanel-border-color: #{mid} !important;\n\
         \x20 --sidebar-background-color: #000000 !important; --sidebar-text-color: #{fg} !important;\n\
         }}\n\
         #navigator-toolbox, #nav-bar, #TabsToolbar, #PersonalToolbar, #titlebar, #sidebar-box {{\n\
         \x20 background-color: #000000 !important; color: #{fg} !important;\n\
         }}\n"))?;
    write("matrix-content-colors.css", &format!(
        "/* generado por matrix-wayland (about:* y nueva pestana) */\n\
         @-moz-document url-prefix(\"about:\") {{\n\
         \x20 :root {{\n\
         \x20   --in-content-page-background: #000000 !important; --in-content-page-color: #{fg} !important;\n\
         \x20   --in-content-text-color: #{fg} !important; --in-content-box-background: #000000 !important;\n\
         \x20   --in-content-box-border-color: #{mid} !important; --in-content-link-color: #{url} !important;\n\
         \x20   --in-content-primary-button-background: #{hi} !important; --in-content-primary-button-text-color: #000000 !important;\n\
         \x20   --newtab-background-color: #000000 !important; --newtab-background-color-secondary: #000000 !important;\n\
         \x20   --newtab-text-primary-color: #{fg} !important;\n\
         \x20 }}\n\
         }}\n"))
}

// ---------- sway (--sway) ----------
// El config de sway debe terminar con `include $HOME/.config/sway/matrix-colors` (despues del
// `client.*` propio: gana el ultimo). Solo cambia los colores de los bordes de las ventanas.
fn write_sway_conf(accent: Option<Accent>) -> Result<(), String> {
    let path = config_dir().ok_or("no se pudo resolver ~/.config")?.join("sway").join("matrix-colors");
    let body = match accent {
        Some(a) => {
            let s = scheme(a);
            format!(
                "# generado por matrix-wayland\n\
                 # clase                  borde     fondo     texto     indicador hijo\n\
                 client.focused          #{hi} #{dim} #{fg} #{hi} #{hi}\n\
                 client.focused_inactive #{mid} #000000 #{mid} #{mid} #{mid}\n\
                 client.unfocused        #{dim} #000000 #{mid} #{dim} #{dim}\n\
                 client.urgent           #{url} #{url} #000000 #{url} #{url}\n",
                hi = s.hi, dim = s.dim, fg = s.fg, mid = s.mid, url = s.url)
        }
        None => "# generado por matrix-wayland: inactivo, se usan los colores de config.d/theme\n".to_string(),
    };
    atomic_write(&path, &body)
}

// ---------- mako (--mako) ----------
// El config de mako debe terminar con `include=/ruta/absoluta/mako/matrix-colors` (mako exige
// ruta absoluta; lo ultimo gana).
fn write_mako_conf(accent: Option<Accent>) -> Result<(), String> {
    let path = config_dir().ok_or("no se pudo resolver ~/.config")?.join("mako").join("matrix-colors");
    let body = match accent {
        Some(a) => {
            let s = scheme(a);
            format!(
                "# generado por matrix-wayland\nbackground-color=#000000\ntext-color=#{fg}\nborder-color=#{hi}\nprogress-color=over #{dim}\n",
                fg = s.fg, hi = s.hi, dim = s.dim)
        }
        None => "# generado por matrix-wayland: inactivo, se usan los colores de config\n".to_string(),
    };
    atomic_write(&path, &body)
}

// ---------- gtklock (--gtklock) ----------
// El style.css de gtklock define colores por defecto con `@define-color matrix_*` y despues hace
// `@import url("matrix-colors.css");` (el import sobrescribe los defaults); ver el README.
fn write_gtklock_css(accent: Option<Accent>) -> Result<(), String> {
    let path = config_dir().ok_or("no se pudo resolver ~/.config")?.join("gtklock").join("matrix-colors.css");
    let body = match accent {
        Some(a) => {
            let s = scheme(a);
            format!(
                "/* generado por matrix-wayland */\n\
                 @define-color matrix_fg #{fg};\n@define-color matrix_hi #{hi};\n@define-color matrix_lock_bg #000000;\n\
                 @define-color matrix_field_bg #000000;\n@define-color matrix_field_border #{hi};\n\
                 @define-color matrix_field_text #{fg};\n@define-color matrix_hover #{dim};\n",
                fg = s.fg, hi = s.hi, dim = s.dim)
        }
        None => "/* generado por matrix-wayland: inactivo, se usan los colores por defecto de style.css */\n".to_string(),
    };
    atomic_write(&path, &body)
}
