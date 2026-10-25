use crate::api::Kind;
use crate::i18n::fill;
use crate::settings::{self, SECTIONS};
use crate::{App, Focus, Lyrics, Popup, Repeat, Side};
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap};
use ratatui::Frame;
use ratatui_image::StatefulImage;

const I_PLAY: &str = "\u{f040a}";
const I_PAUSE: &str = "\u{f03e4}";
const I_SHUFFLE: &str = "\u{f049d}";
const I_RADIO: &str = "\u{f0439}";
const I_VOL: &str = "\u{f057e}";

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

    /// El icono de Nerd Font, o su versión en texto si los iconos están apagados.
    fn ic(&self, nerd: &'static str, plain: &'static str) -> &'static str {
        if self.cfg.icons {
            nerd
        } else {
            plain
        }
    }

    fn block(&self, focused: bool) -> Block<'static> {
        let c = if focused { self.theme.primary } else { self.theme.border };
        let b = Block::bordered().border_style(Style::new().fg(c));
        match self.cfg.borders.border_type(self.rounded) {
            Some(t) => b.border_type(t),
            None => b.border_set(symbols::border::EMPTY),
        }
    }

    pub fn draw(&mut self, f: &mut Frame) {
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
            .title(Line::styled(format!(" {} ", s.lyrics), Style::new().fg(t.primary).bold()))
            .padding(ratatui::widgets::Padding::horizontal(1));
        let lines: Vec<Line> = match &self.lyrics {
            _ if self.current().is_none() => vec![Line::styled(s.nothing_playing, muted)],
            Some((_, Lyrics::Text(text, source))) => {
                if !source.is_empty() {
                    block = block.title_bottom(Line::styled(format!(" {source} "), Style::new().fg(t.muted)).right_aligned());
                }
                text.lines().map(|l| Line::styled(l.to_string(), Style::new().fg(t.fg))).collect()
            }
            Some((_, Lyrics::Missing)) => vec![Line::styled(s.no_lyrics, muted)],
            Some((_, Lyrics::Err(e))) => vec![Line::styled(fill(s.load_failed, &[e]), Style::new().fg(t.accent))],
            Some((_, Lyrics::Loading)) | None => vec![Line::styled(s.loading, muted)],
        };
        let p = Paragraph::new(lines).centered().wrap(Wrap { trim: false }).scroll((self.lyrics_scroll, 0));
        f.render_widget(p.block(block), area);
    }

    fn draw_help(&self, f: &mut Frame) {
        let (t, help) = (self.theme, self.s().help);
        let kw = help.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);
        let lines: Vec<Line> = help
            .iter()
            .map(|(k, d)| {
                Line::from(vec![
                    Span::styled(format!("{k:>kw$}  "), Style::new().fg(t.primary).bold()),
                    Span::styled(*d, Style::new().fg(t.fg)),
                ])
            })
            .collect();
        let w = lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 4;
        let area = self.popup_area(f, w, help.len() as u16 + 2);
        let block = self
            .block(true)
            .title(Line::styled(format!(" {} ", self.s().keys_title), Style::new().fg(t.primary).bold()))
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
        for (n, (title, keys)) in SECTIONS.iter().enumerate() {
            if n > 0 {
                lines.push(Line::raw(""));
            }
            lines.push(Line::styled(title(s), Style::new().fg(t.primary).bold()));
            for &k in keys.iter() {
                let on = i == sel;
                let value = self.setting_value(k);
                let mut right = vec![if on {
                    Span::styled(format!("‹ {value} ›"), Style::new().fg(t.primary).bold())
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
                let left = Span::styled(
                    format!("{} {}", if on { "▌" } else { " " }, k.label(s)),
                    if on { Style::new().fg(t.fg).bold() } else { Style::new().fg(t.muted) },
                );
                let used = left.width() + right.iter().map(Span::width).sum::<usize>();
                let mut spans = vec![left, Span::raw(" ".repeat(inner_w.saturating_sub(used)))];
                spans.extend(right);
                let line = Line::from(spans);
                if on {
                    sel_line = lines.len();
                    lines.push(line.style(Style::new().bg(t.hl)));
                } else {
                    lines.push(line);
                }
                i += 1;
            }
        }
        let h = (lines.len() as u16 + 2).min(f.area().height.saturating_sub(2));
        let area = self.popup_area(f, w, h);
        let scroll = (sel_line + 1).saturating_sub(h.saturating_sub(2) as usize) as u16;
        let block = self
            .block(true)
            .title(Line::styled(format!(" {} ", s.settings), Style::new().fg(t.primary).bold()))
            .title_bottom(Line::styled(format!(" {} ", s.custom_themes), Style::new().fg(t.muted)).right_aligned())
            .padding(ratatui::widgets::Padding::horizontal(1));
        f.render_widget(Paragraph::new(lines).scroll((scroll, 0)).block(block), area);
    }

    fn draw_add_to(&mut self, f: &mut Frame) {
        let t = self.theme;
        let pls = self.own_playlists();
        let Some(Popup::AddTo { item, .. }) = &self.popup else { return };
        let title = format!(" {} ", fill(self.s().add_to, &[&item.title]));
        let w = pls.iter().map(|p| p.title.chars().count()).max().unwrap_or(0).max(title.chars().count()) as u16 + 6;
        let h = (pls.len() as u16 + 2).min(f.area().height.saturating_sub(4));
        let area = self.popup_area(f, w.min(f.area().width.saturating_sub(4)), h);
        let items: Vec<ListItem> =
            pls.iter().map(|p| ListItem::new(Line::styled(p.title.clone(), Style::new().fg(t.fg)))).collect();
        let block = self
            .block(true)
            .title(Line::styled(title, Style::new().fg(t.primary).bold()))
            .padding(ratatui::widgets::Padding::horizontal(1));
        let list = List::new(items).block(block).highlight_style(Style::new().bg(t.hl).add_modifier(Modifier::BOLD));
        if let Some(Popup::AddTo { state, .. }) = &mut self.popup {
            f.render_stateful_widget(list, area, state);
        }
    }

    fn draw_search(&self, f: &mut Frame, area: Rect) {
        let t = self.theme;
        let focused = self.focus == Focus::Search;
        let block = self.block(focused).padding(ratatui::widgets::Padding::horizontal(1));
        let inner = block.inner(area);
        let text = if self.input.is_empty() && !focused {
            Line::styled(self.s().search_placeholder, Style::new().fg(t.muted).italic())
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
        let sep = "─".repeat(area.width.saturating_sub(4) as usize);
        let items: Vec<ListItem> = self
            .side
            .iter()
            .map(|(label, id)| match id {
                Side::Sep => ListItem::new(Line::styled(sep.clone(), Style::new().fg(t.border))),
                Side::Hint => ListItem::new(Line::styled(label.clone(), Style::new().fg(t.muted).italic())),
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
                    ListItem::new(Line::styled(format!("{icon}{label}"), Style::new().fg(t.fg)))
                }
            })
            .collect();
        let hl = if focused {
            Style::new().bg(t.hl).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(t.primary)
        };
        let list = List::new(items)
            .block(self.block(focused).padding(ratatui::widgets::Padding::horizontal(1)))
            .highlight_style(hl);
        f.render_stateful_widget(list, area, &mut self.side_state);
    }

    fn draw_main(&mut self, f: &mut Frame, area: Rect) {
        let t = self.theme;
        let focused = self.focus == Focus::Main;
        let n = self.items().len();
        let mut title = vec![Span::raw(" ")];
        let crumbs: Vec<&str> = self.views.iter().rev().take(3).rev().map(|v| v.title.as_str()).collect();
        title.push(Span::styled(crumbs.join("  ›  "), Style::new().fg(t.primary).bold()));
        if !self.views.is_empty() {
            title.push(Span::styled(format!("  ·  {n}"), Style::new().fg(t.muted)));
        }
        if let Some(l) = &self.loading {
            title.push(Span::styled(format!("   … {l}"), Style::new().fg(t.muted).italic()));
        }
        title.push(Span::raw(" "));
        let block = self.block(focused).title(Line::from(title));

        if n == 0 {
            let msg = if self.loading.is_some() { self.s().loading } else { self.s().empty };
            let p = Paragraph::new(Line::styled(msg, Style::new().fg(t.muted).italic())).block(block);
            return f.render_widget(p, area);
        }

        let queue_view = self.views.last().is_some_and(|v| v.queue);
        let cur = self.current().and_then(|x| x.video_id.clone());
        let qi = self.qi;
        let muted = Style::new().fg(t.muted);
        let rows: Vec<Row> = self
            .items()
            .iter()
            .enumerate()
            .map(|(i, it)| {
                let playing = if queue_view { qi == Some(i) } else { it.playable() && it.video_id == cur };
                let played = queue_view && qi.is_some_and(|q| i < q);
                let mark = if playing {
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
                } else if played {
                    muted
                } else {
                    Style::new().fg(t.fg)
                };
                Row::new([
                    Cell::from(mark),
                    Cell::from(Span::styled(it.title.clone(), title_style)),
                    Cell::from(Span::styled(it.artist.clone(), muted)),
                    Cell::from(Span::styled(it.album.clone(), muted)),
                    Cell::from(Line::styled(it.duration.clone(), muted).right_aligned()),
                ])
            })
            .collect();
        let s = self.s();
        let header = Row::new(["", s.col_title, s.col_artist, s.col_album, ""]).style(muted);
        let table = Table::new(
            rows,
            [
                Constraint::Length(2),
                Constraint::Fill(9),
                Constraint::Fill(6),
                Constraint::Fill(5),
                Constraint::Length(7),
            ],
        )
        .header(header)
        .block(block)
        .column_spacing(1)
        .row_highlight_style(if focused { Style::new().bg(t.hl) } else { Style::new().bg(t.border) });
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
        let [top, prog] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);

        let on = |b: bool| if b { Style::new().fg(t.primary).bold() } else { Style::new().fg(t.muted) };
        let rep = match self.repeat {
            Repeat::Off => self.ic("\u{f0457}", "rep"),
            Repeat::All => self.ic("\u{f0456}", "rep"),
            Repeat::One => self.ic("\u{f0458}", "rep1"),
        };
        let flags = Line::from(vec![
            Span::styled(format!("{} ", self.ic(I_SHUFFLE, "shuf")), on(self.shuffle)),
            Span::styled(format!("{rep} "), on(self.repeat != Repeat::Off)),
            Span::styled(format!("{} ", self.ic(I_RADIO, "auto")), on(self.autoplay)),
            Span::styled(format!("{} {}%", self.ic(I_VOL, "vol"), self.volume), Style::new().fg(t.fg)),
        ]);
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(flags.width() as u16 + 1)]).areas(top);
        f.render_widget(Paragraph::new(flags).right_aligned(), right);

        let it = self.current();
        let now = match it {
            Some(it) => {
                let mut v = vec![
                    Span::styled(
                        format!(
                            "{}  ",
                            if self.paused || !self.loaded { self.ic(I_PLAY, "▶") } else { self.ic(I_PAUSE, "⏸") }
                        ),
                        Style::new().fg(t.primary),
                    ),
                    Span::styled(it.title.clone(), Style::new().fg(t.fg).bold()),
                ];
                if !it.artist.is_empty() {
                    v.push(Span::styled(format!("  ·  {}", it.artist), Style::new().fg(t.muted)));
                }
                Line::from(v)
            }
            None => Line::styled(self.s().idle_hint, Style::new().fg(t.muted).italic()),
        };
        f.render_widget(Paragraph::new(now), left);

        let (l, r) = if it.is_some() { (fmt(self.pos), fmt(self.dur)) } else { (fmt(None), fmt(None)) };
        let bw = (prog.width as usize).saturating_sub(l.len() + r.len() + 2);
        let frac = match (self.pos, self.dur) {
            (Some(p), Some(d)) if d > 0.0 => (p / d).clamp(0.0, 1.0),
            _ => 0.0,
        };
        let done = (bw as f64 * frac).round() as usize;
        let (full, head, rest) = self.cfg.bar.glyphs();
        let (body, head) = if done > 0 && it.is_some() {
            (full.repeat(done - 1), head)
        } else {
            (String::new(), "")
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{l} "), Style::new().fg(t.muted)),
                Span::styled(body, Style::new().fg(t.primary)),
                Span::styled(head, Style::new().fg(t.primary).bold()),
                Span::styled(rest.repeat(bw - done), Style::new().fg(t.border)),
                Span::styled(format!(" {r}"), Style::new().fg(t.muted)),
            ])),
            prog,
        );
    }

    fn draw_footer(&self, f: &mut Frame, area: Rect) {
        let t = self.theme;
        let s = self.s();
        let keys = match (&self.popup, self.focus) {
            (Some(Popup::AddTo { .. }), _) => s.foot_add,
            (Some(Popup::Help), _) => s.foot_help,
            (Some(Popup::Settings { .. }), _) => s.foot_settings,
            (None, Focus::Search) => s.foot_search,
            (None, Focus::Lyrics) => s.foot_lyrics,
            _ if self.queue_view() => s.foot_queue,
            _ => s.foot_main,
        };
        let mut spans = vec![Span::raw(" ")];
        for (k, d) in keys {
            spans.push(Span::styled(*k, Style::new().fg(t.primary).bold()));
            spans.push(Span::styled(format!(" {d}  "), Style::new().fg(t.muted)));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}
