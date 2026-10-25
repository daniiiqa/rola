# rola

*Rola*: canción, en México. (Antes se llamaba ytm; la primera vez que
abre, rola mueve solo las carpetas de `ytm` a las suyas.)

YouTube Music en la terminal, al estilo spotify-tui. Rust + ratatui; el audio
lo pone mpv. Sale por MPRIS como «rola», así que las teclas multimedia, playerctl,
waybar y los auriculares bluetooth manejan su cola (siguiente, anterior, pausa,
posición, volumen, aleatorio, repetir). Habla directo con la API
interna de YouTube Music, sin runtime ni librerías de Python.

## Instalar

```bash
sudo pacman -S --needed mpv yt-dlp
cargo install --path . --root ~/.local --force   # deja ~/.local/bin/rola
```

`rola --login` conecta tu cuenta (me gusta, historial, playlists, álbumes,
artistas); sin login funcionan búsqueda e inicio. La cookie queda en
`~/.local/share/rola/auth.json` (0600).

La portada de lo que suena sale al pie de la barra lateral: en kitty (o
cualquier terminal con kitty graphics, sixel o iTerm2) como imagen de verdad,
en las demás con medios bloques.

## Ajustes y temas

`o` abre los ajustes; los cambios se ven al momento y se guardan al cerrar en
`~/.config/rola/config.json` (se puede editar a mano):

- **General**: idioma (español / English), recuperar la sesión al abrir,
  duración de los avisos.
- **Apariencia**: tema, fondo (el de la terminal o el del tema), bordes
  (como Hyprland, redondeados, rectos, dobles, gruesos, punteados, sin bordes),
  estilo de la barra de progreso, botones con iconos Nerd Font, palabras cortas o enteras, rótulos en
  mayúsculas o minúsculas, cómo se marca la fila elegida (relleno, barra al
  lado, invertida), color (normal, fósforo, tinta), ancho de la barra lateral y de la
  letra, atajos al pie, portada, animaciones (ecualizador, títulos largos que se
  desplazan, aviso de canción nueva; la barra va en degradado si el color es «a color»).
- **Reproducción**: calidad del audio (la mejor, ≤128k, baja), paso del
  volumen, salto de `<`/`>`, autoplay.

Temas: `kitty` (por defecto) usa el theme que incluya `~/.config/kitty/kitty.conf`
en ese momento, así que sigue a los looks de Hyprland que cambian el theme de
kitty, y al wallpaper cuando ese theme es el de wallust; `wallust` lee siempre
`~/.config/kitty/kitty-themes/01-Wallust.conf` (otra ruta con `"wallust"` en
el config); `terminal` (los 16 colores de tu terminal), catppuccin-mocha,
catppuccin-latte, tokyo-night, gruvbox, nord, dracula, rose-pine, everforest,
kanagawa y youtube. Para uno propio, deja un `.conf` en `~/.config/rola/themes/`
con el formato de un theme de kitty (`background`, `foreground`, `color0`…
`color15`); `primary` y `accent` eligen los colores a mano y si no se usan
`color12` y `color11`. Todos los que salen de un archivo se recargan solos.

Los bordes «como Hyprland» le preguntan a Hyprland su `decoration:rounding`
por su socket: esquinas rectas con 0, redondeadas si no. Fuera de Hyprland
quedan redondeados.

### Looks

`~/.config/rola/look.json`, si existe, pisa las claves que traiga del config
(p. ej. `{"borders": "thick", "case": "upper", "select": "bar"}`). Pensado
para un selector de looks que copie archivos a `~/.config`: cada look trae su
`rola/look.json` y rola lo aplica al momento (mira el archivo y escucha cuando
Hyprland recarga su config). Lo que cambies en los ajustes sobre una de esas
claves se guarda en `look.json` (marcado «del look» en el menú), así vuelve
con ese look.

## Arranque rápido de canciones

Pedirle a mpv que resuelva la canción con yt-dlp tarda ~3 s; con el enlace
directo, ~0.2 s. Así que rola resuelve por adelantado la siguiente de la cola
(y la canción en la que dejas el cursor) y le pasa a mpv el enlace ya hecho.
Si un enlace caduca o falla, vuelve al camino normal solo.

## Radio

Como en YT Music: `Enter` en una canción del inicio o de una búsqueda pone esa
canción y detrás su radio. En álbumes y playlists suena la lista en orden y,
con autoplay, al final sigue la radio de la última. Cuando quedan un par de
canciones pide más, así que no se corta.

## Ratón

Clic en la barra lateral abre; en una canción la elige, doble clic la pone y
clic derecho la manda a la cola. Clic (o arrastrar) en la barra de progreso
salta; en aleatorio / repetir / autoplay los cambia; en ▶ pausa y en el título
abre la cola. La rueda mueve lo que tengas debajo y, sobre la barra de abajo,
el volumen. En los ajustes: clic elige, otro clic cambia (derecho, al revés).
Para seleccionar texto, Mayús + arrastrar; o se apaga en ajustes → Ratón.

## Teclas

`/` buscar · `Enter` reproducir/abrir · `espacio` pausa · `n`/`p` sig./ant. ·
`<`/`>` ±5s · `+`/`-` volumen · `s` aleatorio · `r` repetir · `a` autoplay
(radio al acabar la cola) · `z` a la cola · `R` radio · `L` me gusta ·
`P` añadir a una playlist · `y` letra · `c` portada · `o` ajustes · `?` ayuda ·
`h`/`l`/`Tab` panel · `j`/`k`/`g`/`G` moverse · `Backspace` atrás · `q` salir

En **Cola**: `Enter` salta a esa canción, `d`/`Supr` la quita, `J`/`K` la mueven.

Al salir se guardan volumen, modos, la cola y por dónde ibas
(`~/.local/state/rola/state.json`); al volver, `espacio` sigue desde ahí.

## Tests

`cargo test -- --ignored --nocapture` prueba búsqueda, álbum, artista,
playlist, radio e inicio contra YouTube de verdad (útil cuando cambian algo).
