# matrix-wayland

Efecto Matrix como **fondo de pantalla** en Wayland. Cliente nativo en Rust que usa
`wlr-layer-shell` (capa `background`) y pinta por CPU con `wl_shm`: sin GPU, sin
terminal y sin XWayland.

## Uso

```
matrix-wayland start -c cyan --fps 15   # arranca en segundo plano (start es opcional)
matrix-wayland status
matrix-wayland restart -c rainbow
matrix-wayland stop                     # parada limpia (SIGTERM), sin kill
matrix-wayland start --foreground       # primer plano, para depurar
```

Opciones: `-c` color (`green cyan magenta pink blue purple orange yellow red`,
`#rrggbb` o `rainbow`), `-s` caracteres (`mixed katakana binary digits hex`),
`--fps N`, `--size PX`, `--font RUTA`, `--waybar` y `--fuzzel` (ver abajo).

## Color de waybar (`--waybar`, opcional)

Con `--waybar`, matrix-wayland escribe en `~/.config/waybar/matrix-colors.css` las variables
`matrix_fg` (texto e iconos = el color elegido), `matrix_bar_bg` y `matrix_mod_bg` (negro),
`matrix_mod_hover` (el color al 25 %) y `matrix_accent`, y manda `SIGUSR2` a waybar para
recargar el estilo. Al parar, el fichero queda vacío y vuelven tus colores por defecto.
Con `-c rainbow` se ignora (no hay un color único).

Pon esto al principio de tu `style.css` y usa las variables en lugar de colores fijos:

```css
/* valores por defecto cuando matrix-wayland no corre */
@define-color matrix_fg #eceff4;
@define-color matrix_bar_bg rgba(46,52,64,0.6);
@define-color matrix_mod_bg rgba(129,161,193,0.4);
@define-color matrix_mod_hover rgba(129,161,193,0.8);
@import "matrix-colors.css";           /* el fichero debe existir; puede estar vacío */

#waybar { color: @matrix_fg; background-color: @matrix_bar_bg; }
#clock, #cpu, #memory { background: @matrix_mod_bg; }
#clock:hover, #cpu:hover, #memory:hover { background: @matrix_mod_hover; }
```

Crea antes el fichero vacío: `touch ~/.config/waybar/matrix-colors.css`. Después:
`matrix-wayland start -c yellow --waybar`. La recarga reinicia los módulos de waybar un
instante, así que solo ocurre al iniciar, reiniciar y parar.

## Color de fuzzel (`--fuzzel`, opcional)

Con `--fuzzel` escribe `~/.config/fuzzel/matrix-colors.ini` (`[colors]`: texto, borde y selección en
el color elegido, fondo negro). Fuzzel lee su configuración al abrirse, así que no hay que recargar
nada. Al parar, el fichero queda vacío y vuelven tus colores.

Fuzzel solo admite `include` en `[main]` y gana el último valor definido, así que tus colores deben
ir en un fichero aparte, incluido antes que el de matrix-wayland (el fichero debe existir):

```ini
# ~/.config/fuzzel/fuzzel.ini (arriba del todo, antes de cualquier [seccion])
include=~/.config/fuzzel/colors-default.ini   # tu bloque [colors] de siempre
include=~/.config/fuzzel/matrix-colors.ini
```

Se pueden combinar: `matrix-wayland start -c yellow --waybar --fuzzel`.

## Compilar e instalar

```
cargo build --release
install -m755 target/release/matrix-wayland ~/.local/bin/
```

Necesita un compositor con `wlr-layer-shell` (sway, Hyprland, river...) y una fuente con
katakana de media anchura. Por defecto `/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc`
(cámbiala con `--font`).

## Consumo medido

Misma lógica en cuatro implementaciones, pantalla 1366x768 (sway). CPU en régimen estable:

| Implementación | CPU |
|---|---|
| Python en `kitten panel` (25 FPS) | ~13 % solo el script, más kitty pintando |
| Rust en terminal con `kitten panel` (15 FPS) | ~17 % (kitty ~16 % + script ~1 %) |
| **matrix-wayland (wl_shm, 15 FPS)** | **~4,3 %**, ~13 MB de RAM |

Las cifras de Python/Go/Rust en terminal se midieron con un pty de 200x50, y la de
matrix-wayland con `/proc/<pid>/stat` a 15 FPS; no son comparables al 100 %.

## Licencia

GPL-3.0-or-later. Ver [LICENSE](LICENSE).

## Limitaciones

- Una sola superficie: no gestiona varios monitores a propósito ni escalado HiDPI.
- El arranque tarda unos segundos de CPU al rasterizar la fuente CJK (después se libera).
- Si falla tras los primeros 1,5 s, `start` ya habrá dicho «iniciado»; usa `--foreground`.
