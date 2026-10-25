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

Los colores salen de wallust (`~/.config/kitty/kitty-themes/01-Wallust.conf`)
y se recargan solos al cambiar de wallpaper.

## Teclas

`/` buscar · `Enter` reproducir/abrir · `espacio` pausa · `n`/`p` sig./ant. ·
`<`/`>` ±5s · `+`/`-` volumen · `s` aleatorio · `r` repetir · `a` autoplay
(radio al acabar la cola) · `z` a la cola · `R` radio · `L` me gusta ·
`h`/`l`/`Tab` panel · `j`/`k`/`g`/`G` moverse · `Backspace` atrás · `q` salir

## Tests

`cargo test -- --ignored --nocapture` prueba búsqueda, álbum, artista,
playlist, radio e inicio contra YouTube de verdad (útil cuando cambian algo).
