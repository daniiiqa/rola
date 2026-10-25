# ytm

YouTube Music en la terminal, al estilo spotify-tui. Rust + ratatui; el audio
lo pone mpv (con mpv-mpris sale en waybar/playerctl). Habla directo con la API
interna de YouTube Music, sin runtime ni librerías de Python.

## Instalar

```bash
sudo pacman -S --needed mpv yt-dlp mpv-mpris
cargo install --path . --root ~/.local --force   # deja ~/.local/bin/ytm
```

`ytm --login` conecta tu cuenta (me gusta, historial, playlists, álbumes,
artistas); sin login funcionan búsqueda e inicio. La cookie queda en
`~/.local/share/ytm/auth.json` (0600).

La portada de lo que suena sale al pie de la barra lateral: en kitty (o
cualquier terminal con kitty graphics, sixel o iTerm2) como imagen de verdad,
en las demás con medios bloques.

## Ajustes y temas

`o` abre los ajustes; los cambios se ven al momento y se guardan al cerrar en
`~/.config/ytm/config.json` (se puede editar a mano):

- **General**: idioma (español / English), recuperar la sesión al abrir,
  duración de los avisos.
- **Apariencia**: tema, fondo (el de la terminal o el del tema), bordes
  (como Hyprland, redondeados, rectos, dobles, gruesos, sin bordes), estilo de la barra de
  progreso, iconos Nerd Font o en texto, ancho de la barra lateral y de la
  letra, atajos al pie, portada.
- **Reproducción**: calidad del audio (la mejor, ≤128k, baja), paso del
  volumen, salto de `<`/`>`, autoplay.

Temas: `kitty` (por defecto) usa el theme que incluya `~/.config/kitty/kitty.conf`
en ese momento, así que sigue a los looks de Hyprland que cambian el theme de
kitty, y al wallpaper cuando ese theme es el de wallust; `wallust` lee siempre
`~/.config/kitty/kitty-themes/01-Wallust.conf` (otra ruta con `"wallust"` en
el config); `terminal` (los 16 colores de tu terminal), catppuccin-mocha,
catppuccin-latte, tokyo-night, gruvbox, nord, dracula, rose-pine, everforest,
kanagawa y youtube. Para uno propio, deja un `.conf` en `~/.config/ytm/themes/`
con el formato de un theme de kitty (`background`, `foreground`, `color0`…
`color15`); `primary` y `accent` eligen los colores a mano y si no se usan
`color12` y `color11`. Todos los que salen de un archivo se recargan solos.

Los bordes «como Hyprland» le preguntan a Hyprland su `decoration:rounding`
(por su socket, cada par de segundos): esquinas rectas con 0, redondeadas si
no. Fuera de Hyprland quedan redondeados.

## Teclas

`/` buscar · `Enter` reproducir/abrir · `espacio` pausa · `n`/`p` sig./ant. ·
`<`/`>` ±5s · `+`/`-` volumen · `s` aleatorio · `r` repetir · `a` autoplay
(radio al acabar la cola) · `z` a la cola · `R` radio · `L` me gusta ·
`P` añadir a una playlist · `y` letra · `c` portada · `o` ajustes · `?` ayuda ·
`h`/`l`/`Tab` panel · `j`/`k`/`g`/`G` moverse · `Backspace` atrás · `q` salir

En **Cola**: `Enter` salta a esa canción, `d`/`Supr` la quita, `J`/`K` la mueven.

Al salir se guardan volumen, modos, la cola y por dónde ibas
(`~/.local/state/ytm/state.json`); al volver, `espacio` sigue desde ahí.

## Tests

`cargo test -- --ignored --nocapture` prueba búsqueda, álbum, artista,
playlist, radio e inicio contra YouTube de verdad (útil cuando cambian algo).
