use crate::api::Kind;
use crate::config::{Bar, Labels, Select, Tint};
use crate::i18n::fill;
use crate::mouse::{Button, Hits};
use crate::settings::{self, SECTIONS};
use crate::{App, Focus, Lyrics, Popup, Repeat, Side, Sleep, SPLASH};
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap};
use ratatui::Frame;
use ratatui_image::StatefulImage;

const I_PLAY: &str = "\u{f040a}";
const I_PAUSE: &str = "\u{f03e4}";
const I_SHUFFLE: &str = "\u{f049d}";
const I_RADIO: &str = "\u{f0439}";
const I_VOL: &str = "\u{f057e}";
const I_SPOTIFY: &str = "\u{f04c7}";
const I_YOUTUBE: &str = "\u{f05c3}";

const LOGO: [&str; 3] = ["┏━┓┏━┓╻  ┏━┓", "┣┳┛┃ ┃┃  ┣━┫", "╹┗╸┗━┛┗━╸╹ ╹"];
/// Media rocola (la otra mitad es su espejo). o luces · B tubos · L «rola» ·
/// R ventana del disco · T/A título/artista · k botonera · g rejilla.
const ROCOLA: [&str; 22] = [
    "            ╭────",
    "        ╭───╯ ooo",
    "     ╭──╯o       ",
    "   ╭─╯o        LL",
    "  ╭╯o   ╭────────",
    "  │o  B │RRRRRRRR",
    "  │o  B │RRRRRRRR",
    "  │o  B │RRRRRRRR",
    "  │o  B │RRRRRRRR",
    "  │o  B │RRRRRRRR",
    "  │o  B │RRRRRRRR",
    "  │o  B ╰────────",
    "  │o  B  TTTTTTTT",
    "  │o  B  AAAAAAAA",
    "  │o  B  ╭───────",
    "  │o  B  │kkkkkkk",
    "  │o  B  ╰───────",
    "  │   B  gggggggg",
    "  │   B  gggggggg",
    "  │   B  gggggggg",
    "  ╰───┴──────────",
    "     ▀▀▀         ",
];
const LOGO_ASCII: [&str; 4] = [" _ __ ___ | | __ _ ", "| '__/ _ \\| |/ _` |", "| | | (_) | | (_| |", "|_|  \\___/|_|\\__,_|"];

/// Parte `s` en filas de hasta `w` columnas, por palabras (y a la fuerza
/// si una sola no cabe).
fn wrap(s: &str, w: usize) -> Vec<String> {
    let width = |s: &str| Span::raw(s).width();
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in s.split(' ') {
        if !cur.is_empty() && width(&cur) + 1 + width(word) > w {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
        while width(&cur) > w {
            let mut head = String::new();
            for ch in cur.chars() {
                if width(&head) + width(&ch.to_string()) > w {
                    break;
                }
                head.push(ch);
            }
            if head.is_empty() {
                break;
            }
            cur = cur[head.len()..].to_string();
            out.push(head);
        }
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

/// De `a` a `b` (0..=1); si alguno no es RGB (colores de la terminal), `a`.
pub fn mix(a: Color, b: Color, k: f64) -> Color {
    match (a, b) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let m = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * k.clamp(0.0, 1.0)).round() as u8;
            Color::Rgb(m(r1, r2), m(g1, g2), m(b1, b2))
        }
        _ => a,
    }
}

fn fmt(sec: Option<f64>) -> String {
    let Some(s) = sec else { return "-:--".into() };
    let s = s.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

impl App {
    /// Rectángulo centrado y limpio para un popup.
    fn popup_area(&self, f: &mut Frame, w: u16, h: u16) -> Rect {
        let [a] = Layout::horizontal([Constraint::Length(w)]).flex(Flex::Center).areas(f.area());
        let [a] = Layout::vertical([Constraint::Length(h)]).flex(Flex::Center).areas(a);
        f.render_widget(Clear, a);
        f.render_widget(Block::new().bg(self.theme.bg), a);
        a
    }

    /// El icono de Nerd Font, o su versión en texto si los botones van con palabras.
    fn ic(&self, nerd: &'static str, plain: &'static str) -> &'static str {
        if self.cfg.labels == Labels::Icons {
            nerd
        } else {
            plain
        }
    }

    /// Como `ic`, pero con la palabra entera (traducida) para «palabras enteras».
    fn ic3(&self, nerd: &'static str, short: &'static str, word: &'static str) -> String {
        match self.cfg.labels {
            Labels::Icons => nerd.into(),
            Labels::Short => self.tx(short),
            Labels::Words => self.tx(word),
        }
    }

    /// De dónde viene algo: el logo de Spotify o el de YouTube Music, en su
    /// color si el tema va en color. `long` deja la palabra entera (si toca).
    fn source(&self, sp: bool, long: bool) -> Span<'static> {
        let txt = match (self.cfg.labels, long) {
            (Labels::Icons, _) if sp => I_SPOTIFY.into(),
            (Labels::Icons, _) => I_YOUTUBE.into(),
            (Labels::Words, true) if sp => self.tx("Spotify"),
            (Labels::Words, true) => self.tx("YouTube"),
            _ if sp => self.tx("sp"),
            _ => self.tx("yt"),
        };
        let c = match self.cfg.tint {
            Tint::Color if sp => Color::Rgb(30, 215, 96),
            Tint::Color => Color::Rgb(255, 0, 51),
            _ => self.theme.muted,
        };
        Span::styled(txt, Style::new().fg(c))
    }

    /// Los logos solo dicen algo cuando hay dos bibliotecas.
    fn two_sources(&self) -> bool {
        self.spotify.is_some()
    }

    // ── adornos ──

    /// Los colores van en degradado (con tinta o fósforo, un solo color).
    fn gradient(&self, k: f64) -> Color {
        let t = self.theme;
        if self.cfg.tint == Tint::Color {
            mix(t.primary, t.accent, k)
        } else {
            t.primary
        }
    }

    pub fn playing(&self) -> bool {
        self.loaded && !self.paused
    }

    /// Un ecualizador de `n` barras que bailan mientras suena.
    fn eq(&self, n: usize) -> String {
        let lv: &[char] = if self.cfg.bar == Bar::Ascii {
            &['_', '.', '-', '=', '#']
        } else {
            &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█']
        };
        let t = self.epoch.elapsed().as_secs_f64();
        (0..n)
            .map(|k| {
                if !self.playing() || !self.cfg.motion {
                    return lv[lv.len() / 3];
                }
                let k = k as f64;
                let v = ((t * (5.3 + k * 1.9)).sin() + 0.6 * (t * (3.1 + k * 2.7) + k).sin() + 1.6) / 3.2;
                lv[(v.clamp(0.0, 0.999) * lv.len() as f64) as usize]
            })
            .collect()
    }

    /// Una línea que, si no cabe en `w`, se desplaza como en la radio del coche
    /// (quieta dos segundos al empezar cada vuelta).
    fn marquee(&self, spans: Vec<Span<'static>>, w: u16) -> Line<'static> {
        let line = Line::from(spans);
        if !self.cfg.motion || line.width() <= w as usize {
            return line;
        }
        let gap = "     ";
        let mut cells: Vec<(char, Style)> = Vec::new();
        for s in line.spans.iter() {
            cells.extend(s.content.chars().map(|c| (c, s.style)));
        }
        cells.extend(gap.chars().map(|c| (c, Style::new())));
        let n = cells.len();
        let (hold, step) = (2000u128, 180u128);
        let e = self.song_start.elapsed().as_millis() % (hold + step * n as u128);
        let off = if e < hold { 0 } else { ((e - hold) / step) as usize };
        // justo `w` columnas: así puede ir algo detrás
        let mut out: Vec<Span<'static>> = Vec::new();
        let mut used = 0;
        for (c, st) in cells.iter().cycle().skip(off).take(n + w as usize) {
            let cw = Span::raw(c.to_string()).width();
            if used + cw > w as usize {
                break;
            }
            used += cw;
            match out.last_mut() {
                Some(l) if l.style == *st => l.content.to_mut().push(*c),
                _ => out.push(Span::styled(c.to_string(), *st)),
            }
        }
        Line::from(out)
    }

    /// El logo, en degradado, con un mensaje debajo.
    fn draw_logo(&self, f: &mut Frame, area: Rect, msg: &str) {
        let rows: &[&str] = if self.cfg.bar == Bar::Ascii { &LOGO_ASCII } else { &LOGO };
        let w = rows[0].chars().count();
        let mut lines: Vec<Line> = rows
            .iter()
            .map(|r| {
                let spans: Vec<Span> = r
                    .chars()
                    .enumerate()
                    .map(|(i, c)| Span::styled(c.to_string(), Style::new().fg(self.gradient(i as f64 / w as f64)).bold()))
                    .collect();
                Line::from(spans).centered()
            })
            .collect();
        lines.push(Line::raw(""));
        lines.push(Line::styled(self.tx(msg), Style::new().fg(self.theme.muted).italic()).centered());
        let h = lines.len() as u16;
        let [mid] = Layout::vertical([Constraint::Length(h)]).flex(Flex::Center).areas(area);
        f.render_widget(Paragraph::new(lines), mid);
    }

    fn block(&self, focused: bool) -> Block<'static> {
        let c = if focused { self.theme.primary } else { self.theme.border };
        Block::bordered().border_style(Style::new().fg(c)).border_set(self.cfg.borders.set(self.rounded))
    }

    /// Un rótulo de la interfaz en mayúsculas/minúsculas según el look.
    fn tx(&self, s: &str) -> String {
        self.cfg.case.apply(s)
    }

    /// Cómo se marca la fila elegida: estilo y símbolo a su izquierda.
    fn sel(&self, focused: bool) -> (Style, &'static str) {
        let t = self.theme;
        match (self.cfg.select, focused) {
            (Select::Fill, true) => (Style::new().bg(t.hl).fg(t.fg).bold(), ""),
            (Select::Bar, true) => (Style::new().fg(t.primary).bold(), "▌"),
            (Select::Bar, false) => (Style::new().fg(t.primary), "▌"),
            (Select::Inverse, true) => (Style::new().bg(t.primary).fg(t.base).bold(), ""),
            (_, false) => (Style::new().bg(t.border), ""),
        }
    }

    pub fn draw(&mut self, f: &mut Frame) {
        *self.hits.borrow_mut() = Hits::default();
        self.apply_tint();
        if self.theme.bg != Color::Reset {
            f.render_widget(Block::new().style(Style::new().bg(self.theme.bg).fg(self.theme.fg)), f.area());
        }
        let [search, body, bar, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(4),
            Constraint::Length(self.cfg.footer as u16),
        ])
        .areas(f.area());
        let sw = self.cfg.side_width.min(body.width / 2);
        let [side, main] = Layout::horizontal([Constraint::Length(sw), Constraint::Fill(1)]).areas(body);
        self.draw_search(f, search);
        // portada cuadrada al pie de la barra lateral (las celdas son más altas que anchas)
        let (fw, fh) = self.picker.font_size();
        let cover_h = (side.width.saturating_sub(2) as u32 * fw as u32 / fh.max(1) as u32) as u16 + 2;
        let show_cover = self.cover_on && self.current().is_some() && side.height >= cover_h + 8;
        let side = if show_cover {
            let [list, cover] = Layout::vertical([Constraint::Fill(1), Constraint::Length(cover_h)]).areas(side);
            self.draw_cover(f, cover);
            list
        } else {
            side
        };
        self.draw_side(f, side);
        if self.lyrics_on {
            let w = (main.width * self.cfg.lyrics_width / 100).max(24);
            let [table, lyrics] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(w)]).areas(main);
            self.draw_main(f, table);
            self.draw_lyrics(f, lyrics);
        } else {
            self.draw_main(f, main);
        }
        self.draw_bar(f, bar);
        if self.cfg.footer {
            self.draw_footer(f, footer);
        }
        match &self.popup {
            Some(Popup::Help) => self.draw_help(f),
            Some(Popup::Settings { sel }) => self.draw_settings(f, *sel),
            Some(Popup::AddTo { .. }) => self.draw_add_to(f),
            Some(Popup::Login { .. }) => self.draw_login(f),
            Some(Popup::Spotify { .. }) => self.draw_spotify(f),
            Some(Popup::Wrapped { period, plays }) => self.draw_wrapped(f, *period, plays),
            Some(Popup::Scrobble { .. }) => self.draw_scrobble(f),
            Some(Popup::Party) => self.draw_party(f),
            None => {}
        }
    }

    fn draw_cover(&mut self, f: &mut Frame, area: Rect) {
        let block = self.block(false);
        let inner = block.inner(area);
        f.render_widget(block, area);
        match &mut self.cover {
            Some((_, Some(img))) => f.render_stateful_widget(StatefulImage::default(), inner, img),
            _ => {
                let p = Paragraph::new(Line::styled(self.ic("\u{f0025}", "♪"), Style::new().fg(self.theme.muted))).centered();
                let [mid] = Layout::vertical([Constraint::Length(1)]).flex(Flex::Center).areas(inner);
                f.render_widget(p, mid);
            }
        }
    }

    fn draw_lyrics(&self, f: &mut Frame, area: Rect) {
        let (t, s) = (self.theme, self.s());
        let muted = Style::new().fg(t.muted).italic();
        let mut block = self
            .block(self.focus == Focus::Lyrics)
            .title(Line::styled(format!(" {} ", self.tx(s.lyrics)), Style::new().fg(t.primary).bold()))
            .padding(ratatui::widgets::Padding::horizontal(1));
        // sin letra: una rocola
        if let (Some((_, Lyrics::Missing)), Some(it)) = (&self.lyrics, self.current()) {
            self.hits.borrow_mut().lyrics = area;
            let block = block.title_bottom(Line::styled(format!(" {} ", self.tx(s.no_lyrics)), Style::new().fg(t.muted).italic()).centered());
            let inner = block.inner(area);
            f.render_widget(block, area);
            return self.draw_rocola(f, inner, it);
        }
        let lines: Vec<Line> = match &self.lyrics {
            _ if self.current().is_none() => vec![Line::styled(self.tx(s.nothing_playing), muted)],
            Some((_, Lyrics::Text(text, source))) => {
                if !source.is_empty() {
                    block = block.title_bottom(Line::styled(format!(" {source} "), Style::new().fg(t.muted)).right_aligned());
                }
                text.lines().map(|l| Line::styled(l.to_string(), Style::new().fg(t.fg))).collect()
            }
            Some((_, Lyrics::Synced(lines, source))) => {
                let block = block.title_bottom(Line::styled(format!(" {source} "), Style::new().fg(t.muted)).right_aligned());
                return self.draw_synced(f, area, block, lines);
            }
            Some((_, Lyrics::Missing)) => vec![Line::styled(self.tx(s.no_lyrics), muted)],
            Some((_, Lyrics::Err(e))) => vec![Line::styled(fill(s.load_failed, &[e]), Style::new().fg(t.accent))],
            Some((_, Lyrics::Loading)) | None => vec![Line::styled(self.tx(s.loading), muted)],
        };
        self.hits.borrow_mut().lyrics = area;
        let scroll = if self.following_lyrics() { self.lyrics_auto.get() } else { self.lyrics_scroll };
        let p = Paragraph::new(lines).centered().wrap(Wrap { trim: false }).scroll((scroll, 0));
        f.render_widget(p.block(block), area);
    }

    /// La letra con tiempos: lo cantado apagado, la que suena llenándose
    /// como en el karaoke y lo que viene normal. Cada fila guarda su segundo,
    /// así un clic en ella salta ahí.
    fn draw_synced(&self, f: &mut Frame, area: Rect, block: Block, lines: &[(f64, String)]) {
        let t = self.theme;
        let inner = block.inner(area);
        f.render_widget(block, area);
        self.hits.borrow_mut().lyrics = area;
        // la que suena: la última que ya empezó (un pelín antes, que se lee tarde)
        let pos = self.pos.unwrap_or(0.0) + 0.2;
        let cur = lines.iter().rposition(|(at, _)| *at <= pos);
        // cuánto se ha cantado: la línea dura hasta la siguiente, pero sin
        // estirarse en los silencios (lo que se tarda en decirla, más o menos)
        let sung = cur.map_or(0.0, |c| {
            let (at, l) = &lines[c];
            let next = lines.get(c + 1).map_or(at + 4.0, |n| n.0);
            let dur = (next - at).min(l.chars().count() as f64 * 0.11 + 0.6).max(0.3);
            ((pos - at) / dur).clamp(0.0, 1.0)
        });
        // el ajuste de líneas, hecho aquí: así se sabe qué fila es de qué línea
        let w = inner.width.max(1) as usize;
        let mut rows: Vec<(usize, String, usize)> = Vec::new(); // (línea, texto, letras antes en su línea)
        let mut len = vec![0; lines.len()];
        for (i, (_, l)) in lines.iter().enumerate() {
            // los huecos sin letra (solos instrumentales) con una nota
            let l = if l.is_empty() { "♪" } else { l };
            for r in wrap(l, w) {
                let n = r.chars().count();
                rows.push((i, r, len[i]));
                len[i] += n;
            }
        }
        self.lyrics_total.set(rows.len() as u16);
        let first = cur.and_then(|c| rows.iter().position(|r| r.0 == c)).unwrap_or(0);
        // que la que suena quede a un tercio de arriba
        let auto = first.saturating_sub(inner.height as usize / 3);
        self.lyrics_auto.set(auto as u16);
        let scroll = if self.following_lyrics() { auto } else { self.lyrics_scroll as usize };
        let mut at = Vec::new();
        let out: Vec<Line> = rows
            .iter()
            .skip(scroll)
            .take(inner.height as usize)
            .enumerate()
            .map(|(y, (i, r, off))| {
                at.push((inner.y + y as u16, lines[*i].0));
                match cur {
                    Some(c) if c == *i && self.cfg.motion => {
                        let done = ((sung * len[c] as f64).round() as usize).saturating_sub(*off);
                        let mut spans: Vec<Span> = r
                            .chars()
                            .take(done)
                            .enumerate()
                            .map(|(k, ch)| Span::styled(ch.to_string(), Style::new().fg(self.gradient((off + k) as f64 / len[c].max(1) as f64)).bold()))
                            .collect();
                        spans.push(Span::styled(r.chars().skip(done).collect::<String>(), Style::new().fg(t.fg).bold()));
                        Line::from(spans)
                    }
                    Some(c) if c == *i => Line::styled(r.clone(), Style::new().fg(t.primary).bold()),
                    Some(c) if *i < c => Line::styled(r.clone(), Style::new().fg(t.muted)),
                    _ => Line::styled(r.clone(), Style::new().fg(t.fg)),
                }
            })
            .collect();
        self.hits.borrow_mut().lyric_rows = at;
        f.render_widget(Paragraph::new(out).centered(), inner);
    }

    /// Sin letra, una rocola: luces que se persiguen por el arco, burbujas que
    /// suben por los tubos, el disco girando en su ventana y el botón de la
    /// que suena encendido. Todo va con la posición de la canción, así que en
    /// pausa se queda quieta.
    fn draw_rocola(&self, f: &mut Frame, area: Rect, it: &crate::api::Item) {
        let t = self.theme;
        // si no cabe entera, fuera primero rejilla de sobra y patas
        let mut rows: Vec<&str> = ROCOLA.to_vec();
        for drop in [21, 19, 18] {
            if rows.len() as u16 > area.height {
                rows.remove(drop);
            }
        }
        let w = ROCOLA[0].chars().count() as u16 * 2;
        if rows.len() as u16 > area.height || w > area.width {
            let text = vec![
                Line::styled(it.title.clone(), Style::new().fg(t.fg).bold()).centered(),
                Line::styled(it.artist.clone(), Style::new().fg(t.muted)).centered(),
            ];
            let [mid] = Layout::vertical([Constraint::Length(2)]).flex(Flex::Center).areas(area);
            return f.render_widget(Paragraph::new(text), mid);
        }
        let [col] = Layout::horizontal([Constraint::Length(w)]).flex(Flex::Center).areas(area);
        let [body] = Layout::vertical([Constraint::Length(rows.len() as u16)]).flex(Flex::Center).areas(col);
        let mirror = |c: char| match c {
            '╭' => '╮',
            '╮' => '╭',
            '╯' => '╰',
            '╰' => '╯',
            c => c,
        };
        let grid: Vec<Vec<char>> = rows.iter().map(|l| l.chars().chain(l.chars().rev().map(mirror)).collect()).collect();
        // con la canción: quieta en pausa, y sin animaciones todo encendido
        let pos = if self.cfg.motion { self.pos.unwrap_or(0.0) } else { 0.0 };
        let step = (pos * 4.0) as usize;
        // las luces en orden alrededor del centro, para que la persecución dé la vuelta
        let (cx, cy) = (w as f64 / 2.0, grid.len() as f64 / 2.0);
        let mut lights: Vec<(usize, usize)> =
            grid.iter().enumerate().flat_map(|(y, r)| r.iter().enumerate().filter(|(_, c)| **c == 'o').map(move |(x, _)| (x, y))).collect();
        let ang = |&(x, y): &(usize, usize)| ((y as f64 - cy) * 2.0).atan2(x as f64 - cx);
        lights.sort_by(|a, b| ang(a).partial_cmp(&ang(b)).unwrap_or(std::cmp::Ordering::Equal));
        let tube = grid.iter().filter(|r| r.contains(&'B')).count().max(1) as f64;
        let (mut window, mut title, mut artist, mut keys) = (None::<Rect>, None::<Rect>, None::<Rect>, None::<Rect>);
        let grow = |r: &mut Option<Rect>, x: u16, y: u16| {
            let c = Rect::new(x, y, 1, 1);
            *r = Some(r.map_or(c, |r| r.union(c)));
        };
        let logo: Vec<char> = "rola".chars().collect();
        let mut ty = 0.0;
        let buf = f.buffer_mut();
        for (y, row) in grid.iter().enumerate() {
            if row.contains(&'B') {
                ty += 1.0;
            }
            for (x, &ch) in row.iter().enumerate() {
                let (px, py) = (body.x + x as u16, body.y + y as u16);
                let edge = self.gradient(x as f64 / w as f64);
                let (c, st) = match ch {
                    ' ' => continue,
                    'R' => {
                        grow(&mut window, px, py);
                        continue;
                    }
                    'T' => {
                        grow(&mut title, px, py);
                        continue;
                    }
                    'A' => {
                        grow(&mut artist, px, py);
                        continue;
                    }
                    'k' => {
                        grow(&mut keys, px, py);
                        continue;
                    }
                    'o' => {
                        let i = lights.iter().position(|&l| l == (x, y)).unwrap_or(0);
                        let lit = !self.cfg.motion || (i + lights.len() - step % lights.len()).is_multiple_of(3);
                        let c = self.gradient(i as f64 / lights.len() as f64);
                        if lit {
                            ('●', Style::new().fg(c).bold())
                        } else {
                            ('•', Style::new().fg(mix(t.border, c, 0.35)))
                        }
                    }
                    'B' => {
                        // los colores suben por el tubo, y alguna burbuja con ellos
                        let k = (ty / tube - pos * 0.25).rem_euclid(1.0);
                        let c = self.gradient(1.0 - (2.0 * k - 1.0).abs());
                        let bubble = self.cfg.motion && (y + step / 2 + if x < 17 { 0 } else { 2 }).is_multiple_of(5);
                        (if bubble { '°' } else { '┃' }, Style::new().fg(c))
                    }
                    'L' => {
                        let i = row[..x].iter().filter(|c| **c == 'L').count();
                        (logo[i % logo.len()], Style::new().fg(self.gradient(i as f64 / 3.0)).bold())
                    }
                    'g' => (if x % 2 == 0 { '┊' } else { ' ' }, Style::new().fg(t.border)),
                    '▀' => ('▀', Style::new().fg(t.border)),
                    c => (c, Style::new().fg(edge)),
                };
                if let Some(cell) = buf.cell_mut((px, py)) {
                    cell.set_char(c).set_style(st);
                }
            }
        }
        if let Some(r) = window {
            self.disc(f, r, pos * std::f64::consts::TAU / 6.0);
        }
        let line = |r: Option<Rect>, s: Span<'static>, f: &mut Frame| {
            if let Some(r) = r {
                let l = self.marquee(vec![s], r.width);
                f.render_widget(Paragraph::new(l).centered(), r);
            }
        };
        line(title, Span::styled(it.title.clone(), Style::new().fg(t.fg).bold()), f);
        line(artist, Span::styled(it.artist.clone(), Style::new().fg(t.muted)), f);
        if let Some(r) = keys {
            // la botonera, con la de esta canción encendida
            let n = (r.width as usize).div_ceil(2);
            let on = self.qi.unwrap_or(0) % n;
            let spans: Vec<Span> = (0..n)
                .map(|i| {
                    if i == on {
                        Span::styled("▪ ", Style::new().fg(t.accent).bold())
                    } else {
                        Span::styled("▪ ", Style::new().fg(t.muted))
                    }
                })
                .collect();
            f.render_widget(Paragraph::new(Line::from(spans)), r);
        }
    }

    /// Un vinilo en `area`, girado `turn` radianes: surcos, un brillo doble que
    /// da la vuelta y la etiqueta en degradado.
    fn disc(&self, f: &mut Frame, area: Rect, turn: f64) {
        let t = self.theme;
        // las celdas son el doble de altas que anchas: el radio va en filas
        let rf = (area.height as f64 / 2.0).min(area.width as f64 / 4.0);
        let (cx, cy) = (area.x as f64 + area.width as f64 / 2.0, area.y as f64 + area.height as f64 / 2.0);
        let vinyl = mix(t.border, t.base, 0.35);
        let buf = f.buffer_mut();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let (dx, dy) = ((x as f64 + 0.5 - cx) / 2.0, y as f64 + 0.5 - cy);
                let d = (dx * dx + dy * dy).sqrt() / rf;
                let Some(c) = buf.cell_mut((x, y)) else { continue };
                if d > 1.0 {
                    continue;
                }
                let a = dy.atan2(dx) - turn;
                if d < 0.15 {
                    c.set_char('•').set_style(Style::new().fg(t.base).bg(self.gradient(0.5)));
                } else if d < 0.42 {
                    c.set_char(' ').set_style(Style::new().bg(self.gradient(0.5 + 0.5 * a.cos())));
                } else {
                    let shine = (2.0 * a).cos().max(0.0).powi(6);
                    let groove = ((d * rf * 2.2) as i64) % 2 == 0;
                    c.set_char(if groove { '·' } else { ' ' })
                        .set_style(Style::new().fg(mix(t.muted, t.fg, shine)).bg(mix(vinyl, t.muted, shine * 0.45)));
                }
            }
        }
    }

    /// rola wrapped: cuánto, a quién, qué y a qué horas, del periodo elegido.
    fn draw_wrapped(&self, f: &mut Frame, period: usize, plays: &[crate::stats::Play]) {
        use crate::stats::{summary, Period};
        let (t, s) = (self.theme, self.s());
        let w = 92.min(f.area().width.saturating_sub(4));
        let h = 20.min(f.area().height.saturating_sub(2));
        let area = self.popup_area(f, w, h);
        self.hits.borrow_mut().popup = area;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let sum = summary(plays, Period::ALL[period], now);
        // las pestañas de periodo en el título, la elegida resaltada
        let mut title = vec![Span::styled(" rola wrapped ", Style::new().fg(t.primary).bold())];
        for (i, p) in s.wr_periods.iter().enumerate() {
            let st = if i == period { Style::new().fg(t.base).bg(t.primary).bold() } else { Style::new().fg(t.muted) };
            title.push(Span::raw(" "));
            title.push(Span::styled(format!(" {} ", self.tx(p)), st));
        }
        title.push(Span::raw(" "));
        let block = self.block(true).title(Line::from(title)).padding(ratatui::widgets::Padding::new(2, 2, 1, 0));
        let inner = block.inner(area);
        f.render_widget(block, area);
        if sum.plays == 0 {
            let p = Paragraph::new(Line::styled(s.wr_empty, Style::new().fg(t.muted).italic())).centered().wrap(Wrap { trim: true });
            let [mid] = Layout::vertical([Constraint::Length(3)]).flex(Flex::Center).areas(inner);
            return f.render_widget(p, mid);
        }
        let dur = |secs: f64| if secs < 3600.0 { format!("{} min", (secs / 60.0).round()) } else { format!("{:.1} h", secs / 3600.0) };
        let [head, _, cols, _, hours] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(6),
            Constraint::Length(1),
            Constraint::Length(7),
        ])
        .areas(inner);
        // el total, grande y en degradado
        let big = dur(sum.secs);
        let n = big.chars().count().max(1);
        let mut l1: Vec<Span> =
            big.chars().enumerate().map(|(i, c)| Span::styled(c.to_string(), Style::new().fg(self.gradient(i as f64 / n as f64)).bold())).collect();
        l1.push(Span::styled(format!(" {}", s.wr_time), Style::new().fg(t.fg)));
        let mut l2 = vec![Span::styled(fill(s.wr_plays, &[&sum.plays.to_string(), &sum.songs.to_string()]), Style::new().fg(t.muted))];
        if sum.streak > 1 {
            l2.push(Span::styled("  ·  ", Style::new().fg(t.muted)));
            l2.push(Span::styled(format!("{} {}", self.ic("\u{f0238}", "*"), fill(s.wr_streak, &[&sum.streak.to_string()])), Style::new().fg(t.accent)));
        }
        f.render_widget(Paragraph::new(vec![Line::from(l1), Line::from(l2)]), head);

        let [left, _, right] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(3), Constraint::Fill(1)]).areas(cols);
        let header = |x: &str| Line::styled(self.tx(x), Style::new().fg(t.primary).bold());
        // artistas, con su barra
        let mut lines = vec![header(s.wr_artists)];
        let top = sum.artists.first().map_or(1.0, |a| a.1);
        let name_w = 16.min(left.width.saturating_sub(16) as usize).max(6);
        let bar_w = (left.width as usize).saturating_sub(name_w + 12).max(3);
        for (i, (a, secs)) in sum.artists.iter().enumerate() {
            let cells = ((secs / top) * bar_w as f64).round().max(1.0) as usize;
            let name: String = a.chars().take(name_w).collect();
            let mut spans = vec![
                Span::styled(format!("{} ", i + 1), Style::new().fg(t.muted)),
                Span::styled(format!("{name:<name_w$} "), Style::new().fg(t.fg)),
            ];
            spans.extend((0..cells).map(|k| Span::styled("█", Style::new().fg(self.gradient(k as f64 / bar_w as f64)))));
            spans.push(Span::styled(format!(" {}", dur(*secs)), Style::new().fg(t.muted)));
            lines.push(Line::from(spans));
        }
        f.render_widget(Paragraph::new(lines), left);
        // canciones, con las veces
        let mut lines = vec![header(s.wr_songs)];
        for (i, (title, artist, n)) in sum.top.iter().enumerate() {
            // el número y las veces quietos; lo que se desplaza, si no cabe, es la canción
            let times = format!(" ×{n}");
            let room = right.width.saturating_sub(2 + times.chars().count() as u16);
            let song = vec![Span::styled(title.clone(), Style::new().fg(t.fg)), Span::styled(format!(" · {artist}"), Style::new().fg(t.muted))];
            let mut spans = vec![Span::styled(format!("{} ", i + 1), Style::new().fg(t.muted))];
            spans.extend(self.marquee(song, room).spans);
            spans.push(Span::styled(times, Style::new().fg(t.accent).bold()));
            lines.push(Line::from(spans));
        }
        f.render_widget(Paragraph::new(lines), right);

        // a qué horas: un histograma de 4 filas, una columna (doble) por hora
        let peak = sum.hours.iter().cloned().fold(0.0, f64::max).max(1.0);
        let best = sum.hours.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map_or(0, |x| x.0);
        let lv = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        let rows = 4;
        let mut lines = vec![Line::from(vec![
            Span::styled(self.tx(s.wr_hours), Style::new().fg(t.primary).bold()),
            Span::styled(format!("   {}", fill(s.wr_peak, &[&best.to_string()])), Style::new().fg(t.muted)),
        ])];
        for r in (0..rows).rev() {
            let spans: Vec<Span> = (0..24)
                .map(|hr| {
                    let v = sum.hours[hr] / peak * (rows * 8) as f64;
                    let c = match v - (r * 8) as f64 {
                        x if x >= 8.0 => '█',
                        x if x > 0.0 => lv[(x as usize).min(7)],
                        _ if r == 0 => '▁',
                        _ => ' ',
                    };
                    let col = if sum.hours[hr] > 0.0 { self.gradient(hr as f64 / 23.0) } else { t.border };
                    Span::styled(format!("{c}{c} "), Style::new().fg(col))
                })
                .collect();
            lines.push(Line::from(spans));
        }
        let marks: String = (0..24).map(|hr| if hr % 6 == 0 { format!("{hr:<3}") } else { "   ".into() }).collect();
        lines.push(Line::styled(marks, Style::new().fg(t.muted)));
        f.render_widget(Paragraph::new(lines), hours);
    }

    fn draw_help(&self, f: &mut Frame) {
        let (t, help) = (self.theme, self.s().help);
        let kw = help.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);
        let lines: Vec<Line> = help
            .iter()
            .map(|(k, d)| {
                Line::from(vec![
                    Span::styled(format!("{k:>kw$}  "), Style::new().fg(t.primary).bold()),
                    Span::styled(self.tx(d), Style::new().fg(t.fg)),
                ])
            })
            .collect();
        let w = lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 4;
        let area = self.popup_area(f, w, help.len() as u16 + 2);
        self.hits.borrow_mut().popup = area;
        let block = self
            .block(true)
            .title(Line::styled(format!(" {} ", self.tx(self.s().keys_title)), Style::new().fg(t.primary).bold()))
            .padding(ratatui::widgets::Padding::horizontal(1));
        f.render_widget(Paragraph::new(lines).block(block), area);
    }

    fn draw_settings(&self, f: &mut Frame, sel: usize) {
        let (t, s) = (self.theme, self.s());
        let w = 66.min(f.area().width.saturating_sub(4));
        let inner_w = w.saturating_sub(4) as usize;
        let mut lines: Vec<Line> = Vec::new();
        let mut sel_line = 0;
        let mut i = 0;
        let mut at = Vec::new(); // línea de cada ajuste
        for (n, (title, keys)) in SECTIONS.iter().enumerate() {
            if n > 0 {
                lines.push(Line::raw(""));
            }
            lines.push(Line::styled(self.tx(title(s)), Style::new().fg(t.primary).bold()));
            for &k in keys.iter() {
                let on = i == sel;
                let value = self.setting_value(k);
                // la fila elegida toma los colores de la marca de selección
                let mut right = vec![if on {
                    Span::raw(format!("‹ {value} ›"))
                } else {
                    Span::styled(format!("  {value}  "), Style::new().fg(t.fg))
                }];
                if k == settings::Setting::Theme {
                    right.push(Span::raw(" "));
                    for c in [t.bg, t.primary, t.accent, t.fg, t.muted] {
                        if c != Color::Reset {
                            right.push(Span::styled("█", Style::new().fg(c)));
                        }
                    }
                }
                let label = format!("{} {}", if on { "▌" } else { " " }, self.tx(k.label(s)));
                let left = if on { Span::raw(label) } else { Span::styled(label, Style::new().fg(t.muted)) };
                let mut spans = vec![left];
                if k.key().is_some_and(|k| self.cfg.in_look(k)) {
                    spans.push(Span::styled(format!(" {}", self.tx(s.from_look)), Style::new().fg(t.muted).italic()));
                }
                let used = spans.iter().chain(&right).map(Span::width).sum::<usize>();
                spans.push(Span::raw(" ".repeat(inner_w.saturating_sub(used))));
                spans.extend(right);
                let line = Line::from(spans);
                at.push((lines.len(), i));
                if on {
                    sel_line = lines.len();
                    lines.push(line.style(self.sel(true).0));
                } else {
                    lines.push(line);
                }
                i += 1;
            }
        }
        let h = (lines.len() as u16 + 2).min(f.area().height.saturating_sub(2));
        let area = self.popup_area(f, w, h);
        let scroll = (sel_line + 1).saturating_sub(h.saturating_sub(2) as usize) as u16;
        {
            let mut hits = self.hits.borrow_mut();
            hits.popup = area;
            let (top, rows) = (area.y + 1, h.saturating_sub(2) as usize);
            hits.popup_rows = at
                .into_iter()
                .filter(|&(l, _)| l >= scroll as usize && l < scroll as usize + rows)
                .map(|(l, i)| (top + (l - scroll as usize) as u16, i))
                .collect();
        }
        let block = self
            .block(true)
            .title(Line::styled(format!(" {} ", self.tx(s.settings)), Style::new().fg(t.primary).bold()))
            .title_bottom(Line::styled(format!(" {} ", self.tx(s.custom_themes)), Style::new().fg(t.muted)).right_aligned())
            .padding(ratatui::widgets::Padding::horizontal(1));
        f.render_widget(Paragraph::new(lines).scroll((scroll, 0)).block(block), area);
    }

    fn draw_add_to(&mut self, f: &mut Frame) {
        let t = self.theme;
        let pls = self.own_playlists();
        let Some(Popup::AddTo { item, .. }) = &self.popup else { return };
        let title = format!(" {} ", self.tx(&fill(self.s().add_to, &[&item.title])));
        let w = pls.iter().map(|p| p.title.chars().count()).max().unwrap_or(0).max(title.chars().count()) as u16 + 6;
        let h = (pls.len() as u16 + 2).min(f.area().height.saturating_sub(4));
        let area = self.popup_area(f, w.min(f.area().width.saturating_sub(4)), h);
        let items: Vec<ListItem> =
            pls.iter().map(|p| ListItem::new(Line::styled(p.title.clone(), Style::new().fg(t.fg)))).collect();
        let block = self
            .block(true)
            .title(Line::styled(title, Style::new().fg(t.primary).bold()))
            .padding(ratatui::widgets::Padding::horizontal(1));
        let (hl, mark) = self.sel(true);
        let list = List::new(items).block(block).highlight_style(hl).highlight_symbol(mark);
        if let Some(Popup::AddTo { state, .. }) = &mut self.popup {
            f.render_stateful_widget(list, area, state);
            let mut hits = self.hits.borrow_mut();
            hits.popup = area;
            let off = state.offset();
            hits.popup_rows = (0..h.saturating_sub(2)).map(|k| (area.y + 1 + k, off + k as usize)).filter(|&(_, i)| i < pls.len()).collect();
        }
    }

    fn draw_login(&self, f: &mut Frame) {
        let (t, s) = (self.theme, self.s());
        let Some(Popup::Login { browsers, state, busy, err }) = &self.popup else { return };
        let sel = state.selected().unwrap_or(0);
        let (hl, mark) = self.sel(true);
        let mut names: Vec<String> = browsers.iter().map(|b| b.name.to_string()).collect();
        if self.api.authed() {
            names.push(self.tx(s.logout));
        }
        let mut lines = vec![Line::styled(self.tx(s.login_pick), Style::new().fg(t.muted)), Line::raw("")];
        let first = lines.len();
        for (i, n) in names.iter().enumerate() {
            lines.push(if i == sel {
                Line::styled(format!("{mark}{n}"), hl)
            } else {
                Line::styled(format!("{}{n}", " ".repeat(mark.chars().count())), Style::new().fg(t.fg))
            });
        }
        if let Some(b) = busy {
            lines.push(Line::raw(""));
            lines.push(Line::styled(fill(s.login_busy, &[b]), Style::new().fg(t.primary).italic()));
        } else if let Some(e) = err {
            lines.push(Line::raw(""));
            lines.push(Line::styled(e.clone(), Style::new().fg(t.accent)));
        }
        let w = 64.min(f.area().width.saturating_sub(4));
        let wrapped: u16 = lines.iter().map(|l| (l.width() as u16).div_ceil(w.saturating_sub(4).max(1)).max(1)).sum();
        let area = self.popup_area(f, w, wrapped + 2);
        {
            let mut hits = self.hits.borrow_mut();
            hits.popup = area;
            hits.popup_rows = (0..names.len()).map(|i| (area.y + 1 + (first + i) as u16, i)).collect();
        }
        let block = self
            .block(true)
            .title(Line::styled(format!(" {} ", self.tx(s.login_title)), Style::new().fg(t.primary).bold()))
            .padding(ratatui::widgets::Padding::horizontal(1));
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
    }

    fn draw_spotify(&self, f: &mut Frame) {
        let (t, s) = (self.theme, self.s());
        let Some(Popup::Spotify { input, busy, err }) = &self.popup else { return };
        let mut lines: Vec<Line> = Vec::new();
        if let Some(sp) = &self.spotify {
            lines.push(Line::styled(fill(s.sp_connected, &[&sp.user()]), Style::new().fg(t.primary)));
            lines.push(Line::styled(self.tx(s.sp_disconnect_hint), Style::new().fg(t.muted)));
            lines.push(Line::raw(""));
        }
        for (i, l) in s.sp_steps.iter().enumerate() {
            lines.push(Line::styled(*l, Style::new().fg(if i == 0 { t.muted } else { t.fg })));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled(format!("{}: ", self.tx(s.sp_input)), Style::new().fg(t.primary).bold()),
            Span::styled(input.clone(), Style::new().fg(t.fg)),
            Span::styled(if *busy { "" } else { "▏" }, Style::new().fg(t.primary)),
        ]));
        if *busy {
            lines.push(Line::raw(""));
            lines.push(Line::styled(self.tx(s.sp_busy), Style::new().fg(t.primary).italic()));
        } else if let Some(e) = err {
            lines.push(Line::raw(""));
            lines.push(Line::styled(e.clone(), Style::new().fg(t.accent)));
        }
        let w = 78.min(f.area().width.saturating_sub(4));
        let h: u16 = lines.iter().map(|l| (l.width() as u16).div_ceil(w.saturating_sub(4).max(1)).max(1)).sum();
        let area = self.popup_area(f, w, h + 2);
        self.hits.borrow_mut().popup = area;
        let block = self
            .block(true)
            .title(Line::styled(format!(" {} ", self.tx(s.sp_title)), Style::new().fg(t.primary).bold()))
            .padding(ratatui::widgets::Padding::horizontal(1));
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
    }

    /// El QR (en blanco y negro de verdad, para que el móvil lo lea en
    /// cualquier tema) y el enlace.
    fn draw_party(&self, f: &mut Frame) {
        let (t, s) = (self.theme, self.s());
        let Some(p) = &self.party else { return };
        let code = crate::party::qr(&p.url);
        let qr_st = Style::new().fg(Color::Rgb(255, 255, 255)).bg(Color::Rgb(0, 0, 0));
        let mut lines = vec![Line::styled(self.tx(s.party_scan), Style::new().fg(t.fg)), Line::raw("")];
        lines.extend(code.iter().map(|r| Line::styled(r.clone(), qr_st).centered()));
        lines.push(Line::raw(""));
        lines.push(Line::styled(p.url.clone(), Style::new().fg(t.primary).bold()).centered());
        lines.push(Line::raw(""));
        lines.push(Line::styled(self.tx(s.party_hint), Style::new().fg(t.muted)).centered());
        let qw = code.first().map_or(0, |r| r.chars().count()) as u16;
        let w = (qw.max(p.url.len() as u16).max(Line::raw(s.party_scan).width() as u16) + 6).min(f.area().width.saturating_sub(2));
        let area = self.popup_area(f, w, lines.len() as u16 + 2);
        self.hits.borrow_mut().popup = area;
        let block = self
            .block(true)
            .title(Line::styled(format!(" {} ", self.tx(s.party_title)), Style::new().fg(t.primary).bold()))
            .padding(ratatui::widgets::Padding::horizontal(1));
        f.render_widget(Paragraph::new(lines).block(block), area);
    }

    fn draw_scrobble(&self, f: &mut Frame) {
        let (t, s) = (self.theme, self.s());
        let Some(Popup::Scrobble { input, busy, err }) = &self.popup else { return };
        let mut lines: Vec<Line> = Vec::new();
        if let Some((user, _)) = &self.lb {
            lines.push(Line::styled(fill(s.lb_connected, &[user]), Style::new().fg(t.primary)));
            lines.push(Line::styled(self.tx(s.sp_disconnect_hint), Style::new().fg(t.muted)));
            lines.push(Line::raw(""));
        }
        for (i, l) in s.lb_steps.iter().enumerate() {
            lines.push(Line::styled(*l, Style::new().fg(if i == 0 { t.muted } else { t.fg })));
        }
        lines.push(Line::raw(""));
        // el token no se enseña entero: solo sus 4 últimas
        let n = input.chars().count();
        let shown: String = "•".repeat(n.saturating_sub(4)) + &input.chars().skip(n.saturating_sub(4)).collect::<String>();
        lines.push(Line::from(vec![
            Span::styled("Token: ", Style::new().fg(t.primary).bold()),
            Span::styled(shown, Style::new().fg(t.fg)),
            Span::styled(if *busy { "" } else { "▏" }, Style::new().fg(t.primary)),
        ]));
        if *busy {
            lines.push(Line::raw(""));
            lines.push(Line::styled(self.tx(s.lb_busy), Style::new().fg(t.primary).italic()));
        } else if let Some(e) = err {
            lines.push(Line::raw(""));
            lines.push(Line::styled(e.clone(), Style::new().fg(t.accent)));
        }
        let w = 78.min(f.area().width.saturating_sub(4));
        let h: u16 = lines.iter().map(|l| (l.width() as u16).div_ceil(w.saturating_sub(4).max(1)).max(1)).sum();
        let area = self.popup_area(f, w, h + 2);
        self.hits.borrow_mut().popup = area;
        let block = self
            .block(true)
            .title(Line::styled(" ListenBrainz ", Style::new().fg(t.primary).bold()))
            .padding(ratatui::widgets::Padding::horizontal(1));
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
    }

    fn draw_search(&self, f: &mut Frame, area: Rect) {
        let t = self.theme;
        let focused = self.focus == Focus::Search;
        let block = self.block(focused).padding(ratatui::widgets::Padding::horizontal(1));
        let inner = block.inner(area);
        self.hits.borrow_mut().search = area;
        let text = if self.input.is_empty() && !focused {
            Line::styled(self.tx(self.s().search_placeholder), Style::new().fg(t.muted).italic())
        } else {
            Line::styled(self.input.as_str(), Style::new().fg(t.fg))
        };
        let w = Line::from(self.input.as_str()).width() as u16;
        // si no cabe, se ve el final de lo escrito
        let scroll = w.saturating_sub(inner.width.saturating_sub(1));
        f.render_widget(Paragraph::new(text).scroll((0, scroll)).block(block), area);
        if focused {
            f.set_cursor_position((inner.x + w - scroll, inner.y));
        }
    }

    fn draw_side(&mut self, f: &mut Frame, area: Rect) {
        let t = self.theme;
        let focused = self.focus == Focus::Side;
        let line = match self.cfg.borders.set(self.rounded).horizontal_top {
            " " => "─",
            h => h,
        };
        let sep = line.repeat(area.width.saturating_sub(4) as usize);
        let items: Vec<ListItem> = self
            .side
            .iter()
            .map(|(label, id)| match id {
                Side::Sep => ListItem::new(Line::styled(sep.clone(), Style::new().fg(t.border))),
                Side::Hint => ListItem::new(Line::styled(self.tx(label), Style::new().fg(t.muted).italic())),
                // con iconos, el logo delante; si no, el nombre ya lo dice (en su color)
                Side::Head(sp) if self.cfg.labels == Labels::Icons => ListItem::new(Line::from(vec![
                    self.source(*sp, false),
                    Span::styled(format!(" {}", self.tx(label)), Style::new().fg(t.muted).bold()),
                ])),
                Side::Head(sp) => ListItem::new(Line::styled(self.tx(label), self.source(*sp, false).style.bold())),
                _ => {
                    let icon = match id {
                        Side::Home => self.ic("\u{f02dc}  ", ""),
                        Side::Liked => self.ic("\u{f02d1}  ", ""),
                        Side::History => self.ic("\u{f02da}  ", ""),
                        Side::Albums => self.ic("\u{f0025}  ", ""),
                        Side::Artists => self.ic("\u{f0004}  ", ""),
                        Side::Queue => self.ic("\u{f0cb8}  ", ""),
                        _ => "",
                    };
                    ListItem::new(Line::styled(format!("{icon}{}", self.tx(label)), Style::new().fg(t.fg)))
                }
            })
            .collect();
        let (hl, mark) = match self.sel(focused) {
            // sin foco, solo se ve en color cuál es
            (_, "") if !focused => (Style::new().fg(t.primary), ""),
            x => x,
        };
        let block = self.block(focused).padding(ratatui::widgets::Padding::horizontal(1));
        self.hits.borrow_mut().side = block.inner(area);
        let list = List::new(items)
            .block(block)
            .highlight_style(hl)
            .highlight_symbol(mark);
        f.render_stateful_widget(list, area, &mut self.side_state);
    }

    fn draw_main(&mut self, f: &mut Frame, area: Rect) {
        let t = self.theme;
        let focused = self.focus == Focus::Main;
        let n = self.items().len();
        let mut title = vec![Span::raw(" ")];
        let crumbs: Vec<String> = self.views.iter().rev().take(3).rev().map(|v| self.tx(&v.title)).collect();
        title.push(Span::styled(crumbs.join("  ›  "), Style::new().fg(t.primary).bold()));
        if !self.views.is_empty() {
            title.push(Span::styled(format!("  ·  {n}"), Style::new().fg(t.muted)));
        }
        if let Some(l) = &self.loading {
            title.push(Span::styled(format!("   … {l}"), Style::new().fg(t.muted).italic()));
        }
        // canciones de Spotify que aún se están buscando en YouTube Music
        let pending = self.items().iter().filter(|x| x.video_id.is_none() && x.sp_track().is_some_and(|sp| self.matching.contains(sp))).count();
        if pending > 0 {
            let msg = fill(self.s().sp_matching, &[&pending.to_string()]);
            title.push(Span::styled(format!("   … {}", self.tx(&msg)), Style::new().fg(t.muted).italic()));
        }
        title.push(Span::raw(" "));
        let mut block = self.block(focused).title(Line::from(title));
        // canción nueva: su nombre arriba a la derecha un momento, con un destello al entrar
        if let (Some(at), Some(it)) = (self.splash, self.current()) {
            let e = at.elapsed();
            if self.cfg.motion && e < SPLASH {
                let flash = e.as_millis() < 450;
                let st = if flash {
                    Style::new().fg(t.bg).bg(t.accent).bold()
                } else {
                    Style::new().fg(t.accent).bold()
                };
                let txt = format!(" {}  {} ", self.ic("\u{f0f74}", "♪"), it.title);
                block = block.title_top(Line::styled(txt, st).right_aligned());
            }
        }

        if n == 0 {
            let msg = if self.loading.is_some() { self.s().loading } else { self.s().empty };
            let inner = block.inner(area);
            f.render_widget(block, area);
            return self.draw_logo(f, inner, msg);
        }

        let inner = block.inner(area);
        // la primera fila es la cabecera
        self.hits.borrow_mut().rows = Rect { y: inner.y + 1, height: inner.height.saturating_sub(1), ..inner };
        let queue_view = self.views.last().is_some_and(|v| v.queue);
        let cur = self.current().and_then(|x| x.video_id.clone());
        let qi = self.qi;
        let muted = Style::new().fg(t.muted);
        let badges = self.two_sources();
        let rows: Vec<Row> = self
            .items()
            .iter()
            .enumerate()
            .map(|(i, it)| {
                let playing = if queue_view { qi == Some(i) } else { it.playable() && it.video_id == cur };
                let played = queue_view && qi.is_some_and(|q| i < q);
                let mark = if playing && self.playing() && self.cfg.motion {
                    Span::styled(self.eq(2), Style::new().fg(t.primary))
                } else if playing {
                    Span::styled(self.ic(I_PLAY, "▶"), Style::new().fg(t.primary))
                } else if !it.playable() {
                    let icon = match it.kind {
                        Kind::Album => self.ic("\u{f0025}", "◉"),
                        Kind::Artist => self.ic("\u{f0004}", "☺"),
                        Kind::Playlist => self.ic("\u{f0cb8}", "≡"),
                        _ => "",
                    };
                    Span::styled(icon, muted)
                } else {
                    Span::raw("")
                };
                let title_style = if playing {
                    Style::new().fg(t.primary).bold()
                } else if played || (it.sp_track().is_some() && it.video_id.is_none()) {
                    muted
                } else {
                    Style::new().fg(t.fg)
                };
                let mut cells = vec![Cell::from(mark)];
                if badges {
                    cells.push(Cell::from(self.source(it.is_spotify(), false)));
                }
                cells.extend([
                    Cell::from(Span::styled(it.title.clone(), title_style)),
                    Cell::from(Span::styled(it.artist.clone(), muted)),
                    Cell::from(Span::styled(it.album.clone(), muted)),
                    Cell::from(Line::styled(it.duration.clone(), muted).right_aligned()),
                ]);
                Row::new(cells)
            })
            .collect();
        let s = self.s();
        // con dos bibliotecas, una columna para el logo de cada fila
        let mut head = vec![""];
        let mut cols = vec![Constraint::Length(2)];
        if badges {
            head.push("");
            cols.push(Constraint::Length(2));
        }
        head.extend([s.col_title, s.col_artist, s.col_album, ""]);
        cols.extend([Constraint::Fill(9), Constraint::Fill(6), Constraint::Fill(5), Constraint::Length(7)]);
        let header = Row::new(head.into_iter().map(|h| self.tx(h))).style(muted);
        let table = Table::new(rows, cols)
            .header(header)
            .block(block)
            .column_spacing(1)
            .row_highlight_style(self.sel(focused).0)
            .highlight_symbol(self.sel(focused).1);
        if let Some(v) = self.views.last_mut() {
            f.render_stateful_widget(table, area, &mut v.state);
        }
    }

    fn draw_bar(&self, f: &mut Frame, area: Rect) {
        let t = self.theme;
        let mut block = self.block(false);
        if let Some((msg, err, _)) = &self.note {
            let c = if *err { t.accent } else { t.primary };
            block = block.title_top(Line::styled(format!(" {msg} "), Style::new().fg(c)).right_aligned());
        }
        let inner = block.inner(area).inner(ratatui::layout::Margin::new(1, 0));
        f.render_widget(block, area);
        self.hits.borrow_mut().bar = area;
        let [top, prog] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);

        let on = |b: bool| if b { Style::new().fg(t.primary).bold() } else { Style::new().fg(t.muted) };
        let w = self.s().words;
        let rep = match self.repeat {
            Repeat::Off => self.ic3("\u{f0457}", "rep", w[1]),
            Repeat::All => self.ic3("\u{f0456}", "rep", w[1]),
            Repeat::One => self.ic3("\u{f0458}", "rep1", w[2]),
        };
        // las palabras enteras necesitan más aire entre sí
        let gap = if self.cfg.labels == Labels::Words { "  " } else { " " };
        let flags = Line::from(vec![
            Span::styled(format!("{}{gap}", self.ic3(I_SHUFFLE, "shuf", w[0])), on(self.shuffle)),
            Span::styled(format!("{rep}{gap}"), on(self.repeat != Repeat::Off)),
            Span::styled(format!("{}{gap}", self.ic3(I_RADIO, "auto", w[3])), on(self.autoplay)),
            Span::styled(format!("{} {}%", self.ic3(I_VOL, "vol", w[4]), self.volume), Style::new().fg(t.fg)),
        ]);
        let mut flags = flags;
        // con temporizador, lo que queda al final (minutos, o «fin» de esta canción)
        let left = match (self.sleep, self.sleep_left()) {
            (Sleep::Off, _) => None,
            (_, Some(m)) => Some(format!("{m}m")),
            _ => Some(self.tx(self.s().sleep_end)),
        };
        if let Some(left) = left {
            flags.spans.push(Span::styled(format!("{gap}{} {left}", self.ic("\u{f04b2}", "zz")), Style::new().fg(t.accent)));
        }
        // la fiesta encendida
        if self.party.is_some() {
            flags.spans.push(Span::styled(format!("{gap}{}", self.ic3("\u{f0849}", "party", self.s().party_word)), Style::new().fg(t.accent).bold()));
        }
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(flags.width() as u16 + 1)]).areas(top);
        {
            // va alineado a la derecha: cada botón donde cae su trozo
            let mut x = right.right().saturating_sub(flags.width() as u16);
            let mut hits = self.hits.borrow_mut();
            for (s, b) in flags.spans.iter().zip([Button::Shuffle, Button::Repeat, Button::Autoplay]) {
                let w = s.width() as u16;
                hits.buttons.push((Rect::new(x, top.y, w, 1), b));
                x += w;
            }
            if self.current().is_some() {
                hits.buttons.push((Rect::new(left.x, top.y, 2, 1), Button::Play));
                hits.buttons.push((Rect::new(left.x + 2, top.y, left.width.saturating_sub(2), 1), Button::Title));
            }
        }
        f.render_widget(Paragraph::new(flags).right_aligned(), right);

        let it = self.current();
        let now = match it {
            Some(it) => {
                let icon = if self.playing() { self.ic(I_PAUSE, "⏸") } else { self.ic(I_PLAY, "▶") };
                let mut lead = vec![Span::styled(format!("{icon} "), Style::new().fg(t.primary))];
                if self.cfg.motion {
                    lead.push(Span::styled(format!("{} ", self.eq(4)), Style::new().fg(self.gradient(0.5))));
                }
                lead.push(Span::raw(" "));
                // de qué biblioteca viene la que suena
                if self.two_sources() {
                    lead.push(self.source(it.is_spotify(), true));
                    lead.push(Span::raw("  "));
                }
                let mut v = vec![Span::styled(it.title.clone(), Style::new().fg(t.fg).bold())];
                if !it.artist.is_empty() {
                    v.push(Span::styled(format!("  ·  {}", it.artist), Style::new().fg(t.muted)));
                }
                // el icono y el ecualizador quietos; lo que se desplaza es el título
                let lw = Line::from(lead.clone()).width() as u16;
                lead.extend(self.marquee(v, left.width.saturating_sub(lw)).spans);
                Line::from(lead)
            }
            None => Line::styled(self.tx(self.s().idle_hint), Style::new().fg(t.muted).italic()),
        };
        f.render_widget(Paragraph::new(now), left);

        let (l, r) = if it.is_some() { (fmt(self.pos), fmt(self.dur)) } else { (fmt(None), fmt(None)) };
        let bw = (prog.width as usize).saturating_sub(l.len() + r.len() + 2);
        let frac = match (self.pos, self.dur) {
            (Some(p), Some(d)) if d > 0.0 => (p / d).clamp(0.0, 1.0),
            _ => 0.0,
        };
        let done = (bw as f64 * frac).round() as usize;
        self.hits.borrow_mut().prog = Rect::new(prog.x + l.len() as u16 + 1, prog.y, bw as u16, 1);
        let (full, head, rest) = self.cfg.bar.glyphs();
        let (body, head) = if done > 0 && it.is_some() {
            (full.repeat(done - 1), head)
        } else {
            (String::new(), "")
        };
        // lo hecho en degradado del color principal al de acento, a lo ancho de toda la barra
        let mut spans = vec![Span::styled(format!("{l} "), Style::new().fg(t.muted))];
        let fg = |i: usize| self.gradient(i as f64 / bw.max(1) as f64);
        if self.cfg.tint == Tint::Color && matches!((t.primary, t.accent), (Color::Rgb(..), Color::Rgb(..))) {
            spans.extend(body.chars().enumerate().map(|(i, c)| Span::styled(c.to_string(), Style::new().fg(fg(i)))));
        } else {
            spans.push(Span::styled(body, Style::new().fg(t.primary)));
        }
        spans.push(Span::styled(head, Style::new().fg(fg(done)).bold()));
        spans.push(Span::styled(rest.repeat(bw - done), Style::new().fg(t.border)));
        spans.push(Span::styled(format!(" {r}"), Style::new().fg(t.muted)));
        f.render_widget(Paragraph::new(Line::from(spans)), prog);
    }

    fn draw_footer(&self, f: &mut Frame, area: Rect) {
        let t = self.theme;
        let s = self.s();
        let keys = match (&self.popup, self.focus) {
            (Some(Popup::AddTo { .. }), _) => s.foot_add,
            (Some(Popup::Help), _) => s.foot_help,
            (Some(Popup::Settings { .. }), _) => s.foot_settings,
            (Some(Popup::Login { .. }), _) => s.foot_login,
            (Some(Popup::Spotify { .. }), _) => s.foot_spotify,
            (Some(Popup::Wrapped { .. }), _) => s.foot_wrapped,
            (Some(Popup::Scrobble { .. }), _) => s.foot_spotify,
            (Some(Popup::Party), _) => s.foot_party,
            (None, Focus::Search) => s.foot_search,
            (None, Focus::Lyrics) => s.foot_lyrics,
            _ if self.queue_view() => s.foot_queue,
            _ => s.foot_main,
        };
        let mut spans = vec![Span::raw(" ")];
        for (k, d) in keys {
            spans.push(Span::styled(*k, Style::new().fg(t.primary).bold()));
            spans.push(Span::styled(format!(" {}  ", self.tx(d)), Style::new().fg(t.muted)));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_by_words() {
        assert_eq!(wrap("uno dos tres", 7), ["uno dos", "tres"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("", 5), [""]);
        assert_eq!(wrap("año más", 3), ["año", "más"]);
    }

    #[test]
    fn mix_colors() {
        let (a, b) = (Color::Rgb(0, 100, 200), Color::Rgb(200, 100, 0));
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, 0.5), Color::Rgb(100, 100, 100));
        // colores de la terminal: sin degradado
        assert_eq!(mix(Color::Blue, b, 0.5), Color::Blue);
    }
}
