# matrix-wayland

Fondos animados para Wayland (lluvia Matrix, un Galaga o un Pac-Man que se juegan solos), en negro y con el
color que elijas. Cliente nativo en Rust que usa
`wlr-layer-shell` (capa `background`) y pinta por CPU con `wl_shm`: sin GPU, sin
terminal y sin XWayland.

![pacman](docs/pacman.gif)

## Escenas

El fondo es una **escena** (`--scene`), todas en negro y con el color que elijas (`-c`):

- **`matrix`** (por defecto): lluvia de caracteres.

  ![matrix](docs/matrix.png)

- **`galaga`**: un shoot'em up inspirado en Galaga que **se juega solo**. Sprites propios (pixel art),
  formación de enemigos con entradas en curva y picados, y una IA que apunta, dispara y esquiva
  (con tiempo de reacción, así que a veces pierde y la partida se reinicia sola). Tres tipos de
  enemigo y la nave en el color elegido; con `-c rainbow`, un color distinto por tipo.
  `--hud` muestra puntuación, nivel y vidas abajo del todo.

  ![galaga](docs/galaga.gif)

- **`pacman`**: un Pac-Man inspirado que **se juega solo** en un laberinto **generado
  proceduralmente** (simétrico, conexo y sin callejones sin salida), adaptado al tamaño de la
  pantalla y distinto en cada nivel. Cuatro fantasmas con personalidades (persecución, emboscada
  y dos que se retiran a su esquina), pastillas de poder y una IA que busca puntos, huye de los
  fantasmas, va a por las pastillas de poder cuando se ve acorralada y persigue fantasmas
  asustados. Con `-c rainbow` usa los colores clásicos (laberinto azul, Pac-Man amarillo).
  `--hud` muestra puntuación, nivel y vidas; `--scale N` fija el tamaño de casilla en píxeles.

  ![pacman con rainbow](docs/pacman-rainbow.png)
- **`snake`**: cinco serpientes que **se juegan solas**, cada una en su carril y con su propia comida,
  siguiendo un ciclo hamiltoniano (nunca mueren) y tomando atajos seguros. Cada una crece hasta llenar el
  75 % de su carril y reinicia por separado, así la pantalla se va llenando por completo. Con `rainbow`,
  cada serpiente tiene su color.

  ![snake con rainbow](docs/snake.png)

- **`tron`**: motos de luz que se pelean solas. Cada una deja una estela de neón y esquiva eligiendo el
  giro con más espacio libre; gana la última en pie (con `rainbow`, cuatro motos de colores).

  ![tron con rainbow](docs/tron.png)

- **`sleet`**: una noche con aguanieve sobre un puente de piedra con ocho farolas, dibujada con caracteres.
  Las farolas respiran, los reflejos tiemblan en el río, cae nieve en tres profundidades y algo de lluvia,
  y cada 45 s cruza un coche con los faros encendidos y detrás pasa una ráfaga de viento. `-c amber` da la
  luz cálida de farola; con `rainbow`, cada farola tiene su color. Las capas quietas se calculan al
  arrancar (~0,6 s a 1366x768); el agua se rehace ~10 veces por segundo y el coche, 15 como mucho.

- **Paisajes** en caracteres, con sus propios colores (`-c` solo afecta a los programas con tema):
  - **`sunset`**: el sol se esconde en el mar, con nubes, oleaje, su reflejo y un velero. El ciclo dura
    150 s: atardecer, crepúsculo con las primeras estrellas, fundido y vuelta a empezar.
  - **`stars`**: cielo estrellado con la vía láctea sobre unas lomas; cada 3 a 9 s cruza una estrella fugaz
    por un sitio al azar (a veces dos seguidas).
  - **`aurora`**: cortinas de aurora boreal sobre montañas nevadas, un pinar y un lago que las refleja.
  - **`ocean`**: bajo el mar, con cinco peces grandes (payaso, cirujano y ángel) hechos de
    caracteres: nadan tranquilos moviendo la cola, a veces se giran y sueltan burbujas.
    Rayos de luz, medusas, algas y corales.

- **`equalizer`**: analizador de espectro con barras de LED rectangulares, como los ecualizadores de las
  cadenas de música, con caída suave y marcador de pico. **Reacciona a lo que suena**: `parec` capta el
  monitor de la salida por defecto (PipeWire o PulseAudio), una FFT propia reparte el espectro en barras
  logarítmicas (45 Hz – 14 kHz) y la ganancia se ajusta sola. Sin audio, las barras caen a cero. No añade
  dependencias de compilación, pero necesita `parec` (paquete `pipewire-pulse` / `pulseaudio-utils`);
  `--simulate` (o su ausencia) usa una pista simulada con bombo, caja y hi-hats. La captura se marca
  como monitor, así que el módulo `privacy` de waybar no muestra el icono de micrófono. Con `rainbow`,
  verde → amarillo → rojo según la altura. `--scale N` fija el alto base del LED en píxeles (el ancho es
  el doble). `--bar-width PX` fija el ancho de cada barra por separado (p. ej. `--bar-width 6` para barras
  finas conservando el alto del LED).

- **`spectrum`**: el mismo analizador, pero **en espejo**: las barras salen de una línea central y crecen
  hacia arriba y hacia abajo a la vez, como el visor de espectro de Sonic Pi. A diferencia de
  `equalizer`, **cubre toda la pantalla sin márgenes** (los LED se estiran para llenarla justo). Mismas
  opciones que `equalizer` (audio real, `--simulate`, `--scale`, `--bar-width`, `rainbow`).

  ![spectrum](docs/spectrum.gif)

- **`scope`**: osciloscopio mono como el Scope de Sonic Pi: dibuja la forma de onda de lo que suena
  (misma captura con `parec`), con disparo en cruce por cero para que la onda quede quieta y ganancia
  automática. Con `rainbow`, un degradado recorre los neones de izquierda a derecha. `--scale N` fija
  el grosor de la línea; `--simulate` dibuja una onda sintética.

## Uso

```
matrix-wayland start -c cyan                 # arranca en segundo plano (start es opcional)
matrix-wayland start --scene galaga -c yellow --hud
matrix-wayland restart -c purple             # cambia de color y mantiene la escena
matrix-wayland status
matrix-wayland stop                          # parada limpia (SIGTERM), sin kill
matrix-wayland start --foreground            # primer plano, para depurar
```

Opciones: `-c` color (`green cyan magenta pink blue purple orange yellow red`, `#rrggbb` o
`rainbow`), `--scene matrix|galaga|pacman|snake|tron|equalizer|spectrum|scope`, `--fps N` (matrix 15, el resto 30), `-s` caracteres
(`mixed katakana binary digits hex`, solo matrix), `--size PX` y `--font RUTA` (solo matrix),
`--scale N` (galaga: píxeles por píxel de sprite; pacman, snake y tron: píxeles por casilla; equalizer y spectrum: tamaño del LED; scope: grosor de la línea; por defecto según la pantalla), `--hud`,
`--bar-width PX` y `--simulate` (equalizer, spectrum y scope), `--waybar`, `--fuzzel`, `--kitty`, `--firedragon`, `--sway`, `--mako`, `--gtklock`, `--zellij`, `--nvim`, `--starship` y `--lazygit` (ver abajo).

**Las opciones se recuerdan entre arranques** (`~/.local/state/matrix-wayland/options`): lo que
escribas gana sobre lo recordado. `--no-waybar`, `--no-fuzzel`, `--no-kitty`, `--no-firedragon` y `--no-hud` apagan una bandera, y
`--reset` olvida todo y vuelve a los valores por defecto.

### Solo los colores, sin escena

`stop` restaura los colores de waybar y fuzzel. Para apagar la escena y **dejarlos puestos**:

```
matrix-wayland stop --keep-theme      # apaga la escena, waybar y fuzzel conservan el color
matrix-wayland theme                  # reaplica lo recordado (p. ej. tras reiniciar el equipo)
matrix-wayland theme -c cyan          # cambia el color sin ninguna escena
matrix-wayland theme --off            # restaura los colores originales
```

`theme` usa las banderas recordadas (`--waybar`, `--fuzzel`, `--kitty`, `--firedragon`); si hay una escena en marcha, se niega
(usa `restart -c COLOR`). Para que el color aparezca al iniciar sesión sin lanzar la escena, pon
`exec matrix-wayland theme` en tu configuración de sway.

## Color de waybar (`--waybar`, opcional)

En todas las capturas, la barra de arriba sigue el color del fondo (texto e iconos en el color
elegido sobre negro).

Con `--waybar`, matrix-wayland escribe en `~/.config/waybar/matrix-colors.css` las variables
`matrix_fg` (texto e iconos = el color elegido), `matrix_bar_bg` y `matrix_mod_bg` (negro),
`matrix_mod_hover` (el color al 25 %) y `matrix_accent`, y manda `SIGUSR2` a waybar para
recargar el estilo. Al parar, el fichero queda vacío y vuelven tus colores por defecto.
Con `-c rainbow`, waybar y fuzzel también van en modo rainbow: en waybar cada módulo y cada botón de
workspace lleva un neón distinto, alternados (cian, magenta, amarillo, verde...), con fondos negros y
fuzzel con un neón por elemento. Esto asume los ids de módulo del `style.css` de ejemplo de abajo
(`#cpu`, `#clock`, `#workspaces button`...).

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

![fuzzel con el tema de matrix-wayland](docs/fuzzel.png)

Fuzzel solo admite `include` en `[main]` y gana el último valor definido, así que tus colores deben
ir en un fichero aparte, incluido antes que el de matrix-wayland (el fichero debe existir):

```ini
# ~/.config/fuzzel/fuzzel.ini (arriba del todo, antes de cualquier [seccion])
include=~/.config/fuzzel/colors-default.ini   # tu bloque [colors] de siempre
include=~/.config/fuzzel/matrix-colors.ini
```

Se pueden combinar: `matrix-wayland start -c yellow --waybar --fuzzel`.

## Color de kitty (`--kitty`, opcional)

Con `--kitty` escribe `~/.config/kitty/matrix-colors.conf` (fondo negro; texto, cursor, selección, enlaces y
pestañas en el color elegido; la paleta ANSI no se toca) y manda `SIGUSR1` a kitty para que recargue en
caliente. Añade esta línea **al final** de `kitty.conf`, después de tus colores (gana el último), y crea
antes el fichero vacío (`touch ~/.config/kitty/matrix-colors.conf`):

```
include matrix-colors.conf
```

## Color de zellij (`--zellij`, opcional)

zellij no tiene `include`, así que `--zellij` escribe un tema completo en `~/.config/zellij/themes/matrix.kdl`
(fondo negro; texto, cinta, marcos y códigos de salida en el color elegido) y toca `config.kdl` para que zellij
recargue. En tu `config.kdl` pon `theme "matrix"`. Para que al apagar vuelva tu tema, copia el tuyo a
`~/.config/zellij/themes/matrix-off.kdl` y cambia el nombre interno del tema a `matrix-off`; al desactivar,
matrix-wayland copia ese fichero como `matrix.kdl` (con el nombre `matrix`). Si `matrix-off.kdl` no existe,
apagar da error en vez de dejar un tema roto.

```
theme "matrix"
```

## Color de nvim (`--nvim`, opcional)

Con `--nvim` escribe `~/.local/state/nvim/matrix-colors.lua`, que cambia los colores de texto, sintaxis,
selección y búsqueda **encima de tu colorscheme** (conserva su fondo, transparente o no), y le pide a cada
nvim en marcha (`$XDG_RUNTIME_DIR/nvim.PID.0`) que recargue el colorscheme. Inactivo, el fichero es solo un
comentario y vuelve tu colorscheme. Añade esto a tu config (en LazyVim, `lua/config/autocmds.lua`):

```lua
local function matrix_colors()
  local f = vim.fn.stdpath("state") .. "/matrix-colors.lua"
  if vim.uv.fs_stat(f) then pcall(dofile, f) end
end
vim.api.nvim_create_autocmd("ColorScheme", { callback = matrix_colors })
matrix_colors()
```

## Color de starship y lazygit (`--starship`, `--lazygit`, opcional)

Ninguno de los dos tiene `include`, así que matrix-wayland reescribe **solo el bloque entre dos marcadores**
(el resto del fichero no se toca). Añade estas dos líneas al principio de `~/.config/starship.toml` (antes de
cualquier `[tabla]`) y las mismas en `~/.config/lazygit/config.yml`:

```
# >>> matrix-wayland >>>
# <<< matrix-wayland <<<
```

- **starship:** el bloque define `palette = "matrix"` y sobrescribe los nombres `green`, `yellow`, `blue`,
  `purple`, `cyan` y `white` (y sus `bright-`) con tonos del color elegido. El rojo no se toca, para que los
  errores se vean. Starship relee el fichero en cada prompt.
- **lazygit:** el bloque es `gui.theme` (bordes, selección, cambios sin añadir). No debe haber otra clave `gui:`
  en el fichero. Se ve al abrir lazygit.

Si faltan los marcadores, el destino avisa y no modifica nada. Inactivo, el bloque queda como un comentario.

## Color de FireDragon (`--firedragon`, opcional)

Escribe `chrome/matrix-colors.css` (interfaz) y `chrome/matrix-content-colors.css` (`about:*` y nueva
pestaña) en el perfil `*.default-release` de `~/.firedragon`. Impórtalos al principio de tus
`userChrome.css` y `userContent.css`, justo después de `@charset`:

```css
@import "matrix-colors.css";            /* userChrome.css */
@import "matrix-content-colors.css";    /* userContent.css */
```

El navegador solo lee el CSS al arrancar: **hay que reiniciarlo** para ver el color nuevo.

## Color de sway, mako y gtklock (`--sway`, `--mako`, `--gtklock`, opcionales)

- **sway**: escribe `~/.config/sway/matrix-colors` (`client.*`: el color de los bordes de ventana) y lo aplica
  en caliente con `swaymsg`, sin recargar (un `reload` reinicia el `swaybar_command` y puede perder waybar).
  Añade al **final** de tu config `include $HOME/.config/sway/matrix-colors`. Al apagar el tema sí recarga, y
  si waybar no sobrevive, lo relanza.
- **mako**: escribe `~/.config/mako/matrix-colors` y ejecuta `makoctl reload`. Añade al final de tu config
  `include=/ruta/absoluta/.config/mako/matrix-colors` (mako exige ruta absoluta).
- **gtklock**: escribe `~/.config/gtklock/matrix-colors.css` (se lee al bloquear). Tu `style.css` define
  colores por defecto con `@define-color matrix_fg ...`, `matrix_hi`, `matrix_lock_bg`, `matrix_field_bg`,
  `matrix_field_border`, `matrix_field_text` y `matrix_hover`, hace `@import url("matrix-colors.css");` y
  usa esos colores (`@matrix_fg`...); con el tema activo el fondo se vuelve negro.

## Compilar e instalar

```
cargo build --release
install -m755 target/release/matrix-wayland ~/.local/bin/
```

Necesita un compositor con `wlr-layer-shell` (sway, Hyprland, river...) y una fuente con
katakana de media anchura. Por defecto `/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc`
(cámbiala con `--font`).

## Consumo medido

Escena matrix en distintas implementaciones (y galaga al final), pantalla 1366x768 (sway). CPU en régimen estable:

| Implementación | CPU |
|---|---|
| Python en `kitten panel` (25 FPS) | ~13 % solo el script, más kitty pintando |
| Rust en terminal con `kitten panel` (15 FPS) | ~17 % (kitty ~16 % + script ~1 %) |
| **matrix-wayland, escena matrix (wl_shm, 15 FPS)** | **~4,3 %**, ~13 MB de RAM |
| **matrix-wayland, escena galaga (wl_shm, 30 FPS)** | **~6,6 %**, ~11 MB de RAM |
| **matrix-wayland, escena pacman (wl_shm, 30 FPS)** | **~7,9 %**, ~16 MB de RAM |
| **matrix-wayland, escenas snake / tron (wl_shm, 30 FPS)** | **~6-7 %**, ~12 MB de RAM |

Las cifras de Python/Go/Rust en terminal se midieron con un pty de 200x50, y la de
matrix-wayland con `/proc/<pid>/stat`; no son comparables al 100 %.

## Licencia

GPL-3.0-or-later. Ver [LICENSE](LICENSE).

## Limitaciones

- Una sola superficie: no gestiona varios monitores a propósito ni escalado HiDPI.
- El arranque tarda unos segundos de CPU al rasterizar la fuente CJK (después se libera).
- Si falla tras los primeros 1,5 s, `start` ya habrá dicho «iniciado»; usa `--foreground`.
