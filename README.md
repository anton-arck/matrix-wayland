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
`--fps N`, `--size PX`, `--font RUTA`.

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

## Limitaciones

- Una sola superficie: no gestiona varios monitores a propósito ni escalado HiDPI.
- El arranque tarda unos segundos de CPU al rasterizar la fuente CJK (después se libera).
- Si falla tras los primeros 1,5 s, `start` ya habrá dicho «iniciado»; usa `--foreground`.
