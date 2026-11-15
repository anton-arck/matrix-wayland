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

pub const TARGETS: [Target; 11] = [
    Target { name: "waybar", flag: "--waybar", write: write_waybar_css, reload: reload_waybar },
    Target { name: "fuzzel", flag: "--fuzzel", write: write_fuzzel_ini, reload: || {} }, // se lee al abrirlo
    Target { name: "kitty", flag: "--kitty", write: write_kitty_conf, reload: reload_kitty },
    Target { name: "firedragon", flag: "--firedragon", write: write_firedragon_css, reload: || {} }, // solo al arrancar
    Target { name: "sway", flag: "--sway", write: write_sway_conf, reload: reload_sway },
    Target { name: "gtklock", flag: "--gtklock", write: write_gtklock_css, reload: || {} }, // se lee al bloquear
    Target { name: "mako", flag: "--mako", write: write_mako_conf, reload: reload_mako },
    Target { name: "zellij", flag: "--zellij", write: write_zellij_theme, reload: reload_zellij },
    Target { name: "nvim", flag: "--nvim", write: write_nvim_lua, reload: reload_nvim },
    Target { name: "starship", flag: "--starship", write: write_starship_toml, reload: || {} }, // lo relee en cada prompt
    Target { name: "lazygit", flag: "--lazygit", write: write_lazygit_yml, reload: || {} }, // se lee al abrirlo
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
struct Scheme { fg: String, hi: String, url: String, dim: String, mid: String, faint: String }
fn scheme(accent: Accent) -> Scheme {
    match accent {
        Accent::One(c) => Scheme { fg: hex(c), hi: hex(c), url: hex(lite(c)), dim: hex(shade(c, 0.28)), mid: hex(shade(c, 0.55)), faint: hex(shade(c, 0.10)) },
        Accent::Rainbow => {
            let n = |i: usize| NEON[RAINBOW_ORDER[i % RAINBOW_ORDER.len()]].1;
            Scheme { fg: hex(n(0)), hi: hex(n(1)), url: hex(n(2)), dim: hex(shade(n(3), 0.28)), mid: hex(n(4)), faint: hex(shade(n(3), 0.10)) }
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

// ---------- zellij (--zellij) ----------
// zellij no tiene `include`: escribe el tema `matrix` en `themes/matrix.kdl` y el config.kdl del
// usuario debe tener `theme "matrix"`. Inactivo, el tema `matrix` es una copia de
// `themes/matrix-off.kdl` (el tema propio del usuario, cuyo nombre interno es `matrix-off`).
fn rgb(h: &str) -> String {
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
    format!("{} {} {}", c(0), c(2), c(4))
}

fn zellij_theme_body(accent: Option<Accent>, dir: &Path) -> Result<String, String> {
    let Some(a) = accent else {
        let off = dir.join("matrix-off.kdl");
        let src = std::fs::read_to_string(&off).map_err(|e| format!("{}: {e} (copia tu tema a matrix-off.kdl)", off.display()))?;
        return Ok(src.replace("matrix-off", "matrix"));
    };
    let s = scheme(a);
    let (fg, hi, url, dim, mid) = (rgb(&s.fg), rgb(&s.hi), rgb(&s.url), rgb(&s.dim), rgb(&s.mid));
    let k = "0 0 0".to_string();
    let red = "255 0 0".to_string();
    let orange = "255 158 59".to_string();
    // (seccion, base, fondo, emphasis_0..3)
    let sections: [(&str, &String, &String, [&String; 4]); 8] = [
        ("text_unselected", &fg, &k, [&url, &hi, &fg, &mid]),
        ("text_selected", &k, &hi, [&url, &k, &fg, &mid]),
        ("ribbon_unselected", &fg, &dim, [&url, &hi, &fg, &mid]),
        ("ribbon_selected", &k, &hi, [&url, &fg, &mid, &dim]),
        ("frame_selected", &hi, &k, [&url, &fg, &mid, &dim]),
        ("frame_unselected", &dim, &k, [&mid, &dim, &dim, &dim]),
        ("exit_code_success", &fg, &k, [&url, &k, &hi, &mid]),
        ("exit_code_error", &red, &k, [&orange, &k, &k, &k]),
    ];
    let mut out = String::from("// generado por matrix-wayland\nthemes {\n    matrix {\n");
    for (name, base, bg, e) in sections {
        out.push_str(&format!("        {name} {{\n            base {base}\n            background {bg}\n"));
        for (i, c) in e.iter().enumerate() { out.push_str(&format!("            emphasis_{i} {c}\n")); }
        out.push_str("        }\n");
    }
    out.push_str("    }\n}\n");
    Ok(out)
}

fn write_zellij_theme(accent: Option<Accent>) -> Result<(), String> {
    let dir = config_dir().ok_or("no se pudo resolver ~/.config")?.join("zellij").join("themes");
    let body = zellij_theme_body(accent, &dir)?;
    atomic_write(&dir.join("matrix.kdl"), &body)
}

/// zellij vigila su config.kdl y recarga (tema incluido) cuando cambia; basta con tocarlo.
fn reload_zellij() {
    let Some(cfg) = config_dir().map(|d| d.join("zellij").join("config.kdl")) else { return };
    if let Ok(f) = std::fs::OpenOptions::new().append(true).open(&cfg) { let _ = f.set_modified(std::time::SystemTime::now()); }
}

// ---------- nvim (--nvim) ----------
// Escribe `~/.local/state/nvim/matrix-colors.lua`: sobrescribe grupos de resaltado conservando el
// fondo y el resto del colorscheme del usuario. El config de nvim debe hacer `dofile` de ese fichero
// en cada `ColorScheme` (ver el README). Inactivo es solo un comentario y el colorscheme queda intacto.
fn nvim_lua_body(accent: Option<Accent>) -> String {
    let Some(a) = accent else { return "-- generado por matrix-wayland: inactivo, se usa el colorscheme\n_G.matrix_ll = nil\n".to_string() };
    let s = scheme(a);
    format!(
        "-- generado por matrix-wayland\nlocal fg, hi, url, dim, mid, faint = \"#{fg}\", \"#{hi}\", \"#{url}\", \"#{dim}\", \"#{mid}\", \"#{faint}\"\nlocal k = \"#000000\"\n\
         local map = {{\n\
         \x20 Normal = {{ fg = fg }}, NormalFloat = {{ fg = fg }}, FloatBorder = {{ fg = hi }}, WinSeparator = {{ fg = dim }},\n\
         \x20 Comment = {{ fg = mid, italic = true }}, String = {{ fg = url }}, Character = {{ fg = url }}, Number = {{ fg = url }},\n\
         \x20 Boolean = {{ fg = url }}, Constant = {{ fg = url }}, Identifier = {{ fg = fg }}, Function = {{ fg = hi, bold = true }},\n\
         \x20 Statement = {{ fg = hi }}, Keyword = {{ fg = hi }}, Operator = {{ fg = fg }}, Type = {{ fg = url }}, Special = {{ fg = url }},\n\
         \x20 PreProc = {{ fg = mid }}, Title = {{ fg = hi, bold = true }}, Directory = {{ fg = hi }},\n\
         \x20 LineNr = {{ fg = mid }}, CursorLineNr = {{ fg = hi, bold = true }}, Visual = {{ bg = dim }},\n\
         \x20 Search = {{ fg = k, bg = hi }}, IncSearch = {{ fg = k, bg = url }}, MatchParen = {{ fg = k, bg = hi }},\n\
         \x20 Pmenu = {{ fg = fg }}, PmenuSel = {{ fg = k, bg = hi }}, StatusLine = {{ fg = hi }},\n\
         \x20 [\"@variable\"] = {{ fg = fg }}, [\"@property\"] = {{ fg = fg }}, [\"@keyword\"] = {{ fg = hi }}, [\"@function\"] = {{ fg = hi, bold = true }},\n\
         \x20 [\"@string\"] = {{ fg = url }}, [\"@number\"] = {{ fg = url }}, [\"@constant\"] = {{ fg = url }}, [\"@type\"] = {{ fg = url }},\n\
         \x20 [\"@comment\"] = {{ fg = mid, italic = true }}, [\"@operator\"] = {{ fg = fg }}, [\"@punctuation\"] = {{ fg = mid }},\n\
         \x20 CursorLine = {{ bg = faint }}, ColorColumn = {{ bg = faint }}, Folded = {{ fg = mid, bg = faint }}, SignColumn = {{ fg = mid }},\n\
         \x20 [\"@variable.builtin\"] = {{ fg = url }}, [\"@function.builtin\"] = {{ fg = hi, bold = true }}, [\"@constant.builtin\"] = {{ fg = url }},\n\
         \x20 [\"@type.builtin\"] = {{ fg = url }}, [\"@module\"] = {{ fg = fg }}, [\"@variable.parameter\"] = {{ fg = fg }}, [\"@constructor\"] = {{ fg = url }},\n\
         \x20 [\"@keyword.function\"] = {{ fg = hi }}, [\"@keyword.return\"] = {{ fg = hi }}, [\"@keyword.conditional\"] = {{ fg = hi }},\n\
         \x20 [\"@keyword.repeat\"] = {{ fg = hi }}, [\"@keyword.import\"] = {{ fg = hi }}, [\"@keyword.operator\"] = {{ fg = hi }},\n\
         \x20 [\"@string.escape\"] = {{ fg = hi }}, [\"@string.special\"] = {{ fg = hi }}, [\"@tag\"] = {{ fg = hi }}, [\"@attribute\"] = {{ fg = mid }},\n\
         \x20 [\"@punctuation.bracket\"] = {{ fg = mid }}, [\"@punctuation.delimiter\"] = {{ fg = mid }}, [\"@boolean\"] = {{ fg = url }},\n\
         \x20 GitSignsAdd = {{ fg = hi }}, GitSignsChange = {{ fg = url }}, GitSignsDelete = {{ fg = mid }},\n\
         \x20 TelescopeSelection = {{ fg = hi, bg = faint }}, TelescopeBorder = {{ fg = hi }}, TelescopeMatching = {{ fg = url, bold = true }},\n\
         \x20 NeoTreeDirectoryName = {{ fg = hi }}, NeoTreeDirectoryIcon = {{ fg = hi }}, NeoTreeFileName = {{ fg = fg }}, NeoTreeRootName = {{ fg = hi, bold = true }},\n\
         \x20 SnacksDashboardHeader = {{ fg = hi, bold = true }}, SnacksDashboardIcon = {{ fg = hi }}, SnacksDashboardKey = {{ fg = url }},\n\
         \x20 SnacksDashboardDesc = {{ fg = fg }}, SnacksDashboardFooter = {{ fg = mid }}, SnacksDashboardSpecial = {{ fg = url }},\n\
         \x20 SnacksDashboardDir = {{ fg = mid }}, SnacksDashboardFile = {{ fg = fg }}, SnacksDashboardTitle = {{ fg = hi, bold = true }},\n\
         \x20 SnacksDashboardNormal = {{ fg = fg }}, SnacksDashboardTerminal = {{ fg = fg }},\n\
         \x20 WhichKey = {{ fg = url }}, WhichKeyGroup = {{ fg = hi }}, WhichKeyDesc = {{ fg = fg }}, WhichKeySeparator = {{ fg = mid }},\n\
         }}\n\
         ",
        fg = s.fg, hi = s.hi, url = s.url, dim = s.dim, mid = s.mid, faint = s.faint) + NVIM_LUA_TAIL
}

/// Parte fija del Lua generado (usa los locals fg, hi, url, dim, mid, faint, k y la tabla `map`).
const NVIM_LUA_TAIL: &str = r##"
local function apply(t)
  for name, o in pairs(t) do
    local c = vim.api.nvim_get_hl(0, { name = name, link = false })
    vim.api.nvim_set_hl(0, name, vim.tbl_extend("force", c, o))
  end
end
apply(map)

-- lualine: cada grupo que crea pasa por highlight(); se corrige ahi para que sobreviva a sus setup()
-- (ColorScheme, arranque tardio). Fondo negro en c/x, color en a/z, discreto en b/y.
local modebg = { normal = hi, insert = url, visual = mid, replace = url, command = mid, terminal = hi, inactive = k }
local function secbg(l, mode)
  if l == "a" or l == "z" then return modebg[mode] or hi end
  if l == "b" or l == "y" then return dim end
  return k
end
_G.matrix_ll = { fix = function(name, fg_, bg_)
  local fl, fm, tl, tm = name:match("^lualine_transitional_lualine_(%a)_(.-)_to_lualine_(%a)_(.-)$")
  if fl then
    return secbg(fl, fm:match("(%a+)$")), secbg(tl, tm:match("(%a+)$"))
  end
  local l, rest = name:match("^lualine_(%a)_(.+)$")
  if not l then return fg_, bg_ end
  local mode = rest:match("(%a+)$")
  if rest == mode then -- grupo de la seccion (lualine_a_normal)
    if l == "a" or l == "z" then return mode == "inactive" and mid or k, secbg(l, mode) end
    if l == "b" or l == "y" then return (mode == "inactive" and mid or hi), secbg(l, mode) end
    return (mode == "inactive" and mid or fg), k
  end
  return fg_, secbg(l, mode) -- componentes: conservan su color de texto
end }
local okh, H = pcall(require, "lualine.highlight")
if okh and not H._matrix_wrapped then
  local orig = H.highlight
  H.highlight = function(name, fg_, bg_, gui, link)
    local m = _G.matrix_ll
    if m and not link then fg_, bg_ = m.fix(name, fg_, bg_) end
    return orig(name, fg_, bg_, gui, link)
  end
  H._matrix_wrapped = true
end
-- los grupos que lualine ya habia creado se corrigen una vez, y de nuevo cuando acabe su setup()
local function refix()
  for name in pairs(vim.api.nvim_get_hl(0, {})) do
    if name:match("^lualine_") then
      local c = vim.api.nvim_get_hl(0, { name = name, link = false })
      local f, b = _G.matrix_ll.fix(name, c.fg and string.format("#%06x", c.fg), c.bg and string.format("#%06x", c.bg))
      apply({ [name] = { fg = f, bg = b } })
    end
  end
  pcall(vim.cmd, "redrawstatus!")
end
refix()
vim.defer_fn(refix, 150)
"##;

fn write_nvim_lua(accent: Option<Accent>) -> Result<(), String> {
    let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("state")))
        .ok_or("no se pudo resolver ~/.local/state")?;
    let dir = state.join("nvim");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    atomic_write(&dir.join("matrix-colors.lua"), &nvim_lua_body(accent))
}

/// Cada nvim en marcha escucha en `$XDG_RUNTIME_DIR/nvim.<pid>.0`; se le manda recargar el colorscheme
/// (lo que dispara el `ColorScheme` que relee el fichero). Solo a procesos vivos y con limite de tiempo.
fn reload_nvim() {
    let Some(rt) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else { return };
    let Ok(rd) = std::fs::read_dir(&rt) else { return };
    for e in rd.flatten() {
        let name = e.file_name();
        let Some(pid) = name.to_str().and_then(|n| n.strip_prefix("nvim.")).and_then(|n| n.split('.').next()).and_then(|n| n.parse::<i32>().ok()) else { continue };
        if !Path::new(&format!("/proc/{pid}")).exists() { continue; }
        let sock = e.path();
        run_quiet("timeout", &["2", "nvim", "--server", &sock.to_string_lossy(), "--remote-expr",
            "execute('if exists(\"g:colors_name\") | execute \"colorscheme \" . g:colors_name | endif')"]);
    }
}

// ---------- starship (--starship) ----------
// starship no tiene `include`: reescribe el bloque entre `# >>> matrix-wayland >>>` y
// `# <<< matrix-wayland <<<` de starship.toml (arriba del todo, antes de cualquier [tabla]). Define una paleta
// que sobrescribe los nombres de color estandar (green, purple...); rojo se deja para los errores.
const STARSHIP_BEGIN: &str = "# >>> matrix-wayland >>>";
const STARSHIP_END: &str = "# <<< matrix-wayland <<<";

/// Sustituye el cuerpo entre marcadores; None si faltan (o estan desordenados).
fn replace_block(src: &str, body: &str) -> Option<String> {
    let b = src.find(STARSHIP_BEGIN)?;
    let e = src[b..].find(STARSHIP_END)? + b;
    let head_end = b + STARSHIP_BEGIN.len();
    Some(format!("{}\n{}{}", &src[..head_end], body, &src[e..]))
}

fn starship_block(accent: Option<Accent>) -> String {
    let Some(a) = accent else { return "# inactivo: se usan los colores de la terminal\n".to_string() };
    let s = scheme(a);
    let mut out = String::from("palette = \"matrix\"\n");
    let map: [(&str, &String); 11] = [
        ("green", &s.hi), ("bright-green", &s.url), ("yellow", &s.url), ("bright-yellow", &s.url),
        ("blue", &s.mid), ("bright-blue", &s.mid), ("purple", &s.hi), ("bright-purple", &s.hi),
        ("cyan", &s.mid), ("white", &s.fg), ("bright-white", &s.url),
    ];
    for (name, c) in map { out.push_str(&format!("palettes.matrix.\"{name}\" = \"#{c}\"\n")); }
    out
}

fn write_starship_toml(accent: Option<Accent>) -> Result<(), String> {
    let path = std::env::var_os("STARSHIP_CONFIG").map(PathBuf::from)
        .or_else(|| config_dir().map(|d| d.join("starship.toml"))).ok_or("no se pudo resolver starship.toml")?;
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let new = replace_block(&src, &starship_block(accent))
        .ok_or_else(|| format!("{}: faltan las lineas {STARSHIP_BEGIN} y {STARSHIP_END} (ver el README)", path.display()))?;
    if new == src { return Ok(()); }
    atomic_write(&path, &new)
}

// ---------- lazygit (--lazygit) ----------
// Igual que starship: bloque entre los marcadores en `~/.config/lazygit/config.yml` con `gui.theme`
// (no debe haber otra clave `gui:` en el fichero). lazygit lo lee al arrancar.
fn lazygit_block(accent: Option<Accent>) -> String {
    let Some(a) = accent else { return "# inactivo: se usa el tema por defecto de lazygit\n".to_string() };
    let s = scheme(a);
    let (fg, hi, url, dim, mid, faint) = (&s.fg, &s.hi, &s.url, &s.dim, &s.mid, &s.faint);
    format!(
        "gui:\n  theme:\n    activeBorderColor: ['#{hi}', bold]\n    inactiveBorderColor: ['#{mid}']\n\
         \x20   searchingActiveBorderColor: ['#{url}', bold]\n    optionsTextColor: ['#{hi}']\n\
         \x20   selectedLineBgColor: ['#{dim}']\n    inactiveViewSelectedLineBgColor: ['#{faint}']\n\
         \x20   cherryPickedCommitFgColor: ['#{hi}']\n    cherryPickedCommitBgColor: ['#{dim}']\n\
         \x20   markedBaseCommitFgColor: ['#{url}']\n    markedBaseCommitBgColor: ['#{dim}']\n\
         \x20   unstagedChangesColor: ['#{url}']\n    defaultFgColor: ['#{fg}']\n")
}

fn write_lazygit_yml(accent: Option<Accent>) -> Result<(), String> {
    let path = config_dir().ok_or("no se pudo resolver ~/.config")?.join("lazygit").join("config.yml");
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let new = replace_block(&src, &lazygit_block(accent))
        .ok_or_else(|| format!("{}: faltan las lineas {STARSHIP_BEGIN} y {STARSHIP_END} (ver el README)", path.display()))?;
    if new == src { return Ok(()); }
    atomic_write(&path, &new)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zellij_active_theme_has_name_and_green_accent() {
        let body = zellij_theme_body(Some(Accent::One([0, 255, 65])), Path::new("/nonexistent")).unwrap();
        assert!(body.contains("themes {") && body.contains("    matrix {"));
        assert!(body.contains("base 0 255 65"));
        assert_eq!(body.matches('{').count(), body.matches('}').count());
        if let Ok(p) = std::env::var("MATRIX_ZELLIJ_DUMP") { std::fs::write(p, &body).unwrap(); }
    }

    #[test]
    fn zellij_off_copies_user_theme_renamed() {
        let dir = std::env::temp_dir().join(format!("mw-zellij-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("matrix-off.kdl"), "themes {\n    matrix-off { }\n}\n").unwrap();
        let body = zellij_theme_body(None, &dir).unwrap();
        assert!(body.contains("    matrix {") && !body.contains("matrix-off"));
        assert!(zellij_theme_body(None, Path::new("/nonexistent")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod nvim_tests {
    use super::*;

    #[test]
    fn nvim_lua_active_and_off() {
        let on = nvim_lua_body(Some(Accent::One([0, 255, 65])));
        assert!(on.contains("\"#00ff41\"") && on.contains("nvim_set_hl") && on.contains("nvim_get_hl"));
        assert_eq!(on.matches('{').count(), on.matches('}').count());
        assert!(nvim_lua_body(None).starts_with("-- generado"));
        if let Ok(p) = std::env::var("MATRIX_NVIM_DUMP") { std::fs::write(p, &on).unwrap(); }
    }
}

#[cfg(test)]
mod starship_tests {
    use super::*;

    #[test]
    fn block_is_replaced_and_idempotent() {
        let src = format!("a = 1\n{STARSHIP_BEGIN}\nviejo\n{STARSHIP_END}\n[x]\nb = 2\n");
        let on = replace_block(&src, &starship_block(Some(Accent::One([0, 255, 65])))).unwrap();
        assert!(on.starts_with("a = 1\n") && on.ends_with("[x]\nb = 2\n") && on.contains("palette = \"matrix\"") && !on.contains("viejo"));
        assert!(on.contains("palettes.matrix.\"green\" = \"#00ff41\""));
        assert_eq!(replace_block(&on, &starship_block(Some(Accent::One([0, 255, 65])))).unwrap(), on);
        let off = replace_block(&on, &starship_block(None)).unwrap();
        assert!(!off.contains("palette =") && off.contains("[x]"));
        assert!(replace_block("sin marcadores", "x").is_none());
    }
}

#[cfg(test)]
mod lazygit_tests {
    use super::*;

    #[test]
    fn lazygit_block_is_valid_shape() {
        let on = lazygit_block(Some(Accent::One([0, 255, 65])));
        assert!(on.starts_with("gui:\n  theme:\n") && on.contains("activeBorderColor: ['#00ff41', bold]"));
        assert!(lazygit_block(None).starts_with('#'));
        if let Ok(p) = std::env::var("MATRIX_LG_DUMP") { std::fs::write(p, &on).unwrap(); }
    }
}
