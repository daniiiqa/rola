use crate::api::Kind;
use crate::{App, Focus, Repeat, Side};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Cell, List, ListItem, Paragraph, Row, Table};
use ratatui::Frame;

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
    fn block(&self, focused: bool) -> Block<'static> {
        let c = if focused { self.theme.primary } else { self.theme.border };
        Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(c))
    }

    pub fn draw(&mut self, f: &mut Frame) {
        let [search, body, bar, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(4),
            Constraint::Length(1),
        ])
        .areas(f.area());
        let [side, main] = Layout::horizontal([Constraint::Length(28), Constraint::Fill(1)]).areas(body);
        self.draw_search(f, search);
        self.draw_side(f, side);
        self.draw_main(f, main);
        self.draw_bar(f, bar);
        self.draw_footer(f, footer);
    }

    fn draw_search(&self, f: &mut Frame, area: Rect) {
        let t = self.theme;
        let focused = self.focus == Focus::Search;
        let block = self.block(focused).padding(ratatui::widgets::Padding::horizontal(1));
        let inner = block.inner(area);
        let text = if self.input.is_empty() && !focused {
            Line::styled("buscar en YouTube Music  ( / )", Style::new().fg(t.muted).italic())
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
                _ => ListItem::new(Line::styled(label.clone(), Style::new().fg(t.fg))),
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
            let msg = if self.loading.is_some() { "cargando…" } else { "nada por aquí" };
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
                    Span::styled(I_PLAY, Style::new().fg(t.primary))
                } else if !it.playable() {
                    let icon = match it.kind {
                        Kind::Album => "\u{f0025}",
                        Kind::Artist => "\u{f0004}",
                        Kind::Playlist => "\u{f0cb8}",
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
        let header = Row::new(["", "título", "artista", "álbum", ""]).style(muted);
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
            Repeat::Off => "\u{f0457}",
            Repeat::All => "\u{f0456}",
            Repeat::One => "\u{f0458}",
        };
        let flags = Line::from(vec![
            Span::styled(format!("{I_SHUFFLE} "), on(self.shuffle)),
            Span::styled(format!("{rep} "), on(self.repeat != Repeat::Off)),
            Span::styled(format!("{I_RADIO} "), on(self.autoplay)),
            Span::styled(format!("{I_VOL} {}%", self.volume), Style::new().fg(t.fg)),
        ]);
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(flags.width() as u16 + 1)]).areas(top);
        f.render_widget(Paragraph::new(flags).right_aligned(), right);

        let it = self.current();
        let now = match it {
            Some(it) => {
                let mut v = vec![
                    Span::styled(format!("{}  ", if self.paused { I_PLAY } else { I_PAUSE }), Style::new().fg(t.primary)),
                    Span::styled(it.title.clone(), Style::new().fg(t.fg).bold()),
                ];
                if !it.artist.is_empty() {
                    v.push(Span::styled(format!("  ·  {}", it.artist), Style::new().fg(t.muted)));
                }
                Line::from(v)
            }
            None => Line::styled("nada sonando — Enter sobre una canción", Style::new().fg(t.muted).italic()),
        };
        f.render_widget(Paragraph::new(now), left);

        let (l, r) = if it.is_some() { (fmt(self.pos), fmt(self.dur)) } else { (fmt(None), fmt(None)) };
        let bw = (prog.width as usize).saturating_sub(l.len() + r.len() + 2);
        let frac = match (self.pos, self.dur) {
            (Some(p), Some(d)) if d > 0.0 => (p / d).clamp(0.0, 1.0),
            _ => 0.0,
        };
        let done = (bw as f64 * frac).round() as usize;
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{l} "), Style::new().fg(t.muted)),
                Span::styled("━".repeat(done), Style::new().fg(t.primary)),
                Span::styled("─".repeat(bw - done), Style::new().fg(t.border)),
                Span::styled(format!(" {r}"), Style::new().fg(t.muted)),
            ])),
            prog,
        );
    }

    fn draw_footer(&self, f: &mut Frame, area: Rect) {
        let t = self.theme;
        let keys: &[(&str, &str)] = if self.focus == Focus::Search {
            &[("enter", "buscar"), ("esc", "volver"), ("^u", "borrar")]
        } else {
            &[
                ("/", "buscar"),
                ("spc", "pausa"),
                ("n/p", "sig/ant"),
                ("<>", "±5s"),
                ("+-", "vol"),
                ("s", "aleatorio"),
                ("r", "repetir"),
                ("z", "a la cola"),
                ("R", "radio"),
                ("L", "me gusta"),
                ("⌫", "atrás"),
                ("q", "salir"),
            ]
        };
        let mut spans = vec![Span::raw(" ")];
        for (k, d) in keys {
            spans.push(Span::styled(*k, Style::new().fg(t.primary).bold()));
            spans.push(Span::styled(format!(" {d}  "), Style::new().fg(t.muted)));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}
