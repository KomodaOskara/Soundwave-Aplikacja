use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Focus, LoginField, Modal, NavItem, RepeatMode, Screen, StudioTab, PLAYLIST_COLORS};
use crate::player::{PlaybackStatus, PlayerSnapshot, SPECTRUM_BANDS};
use crate::theme::{LayoutMode, Theme, ThemeId};

pub fn draw(frame: &mut Frame, app: &App, snap: &PlayerSnapshot) {
    let t = app.theme();
    frame.render_widget(Block::default().style(Style::default().bg(t.bg)), frame.area());

    match app.screen {
        Screen::Login => draw_login(frame, app, t),
        Screen::Main => draw_main(frame, app, snap, t),
    }
}

fn draw_login(frame: &mut Frame, app: &App, t: Theme) {
    let area = frame.area();
    let card_w = area.width.min(64).max(36.min(area.width));
    let card_h = area.height.min(20);
    let x = area.x + area.width.saturating_sub(card_w) / 2;
    let y = area.y + area.height.saturating_sub(card_h) / 2;
    let card = Rect::new(x, y, card_w, card_h);

    let block = Block::default()
        .title(Span::styled(
            " ♪ SOUNDWAVE CLI ",
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.accent))
        .style(Style::default().bg(t.surface));
    let inner = block.inner(card);
    frame.render_widget(block, card);

    let compact_inputs = inner.height < 15;
    let input_h = if compact_inputs { 1 } else { 3 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(if compact_inputs { 0 } else { 1 })
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(input_h),
            Constraint::Length(input_h),
            Constraint::Length(input_h),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Zaloguj się do swojego konta ", Style::default().fg(t.text2)),
            Span::styled("SoundWave", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        ])),
        chunks[0],
    );

    draw_input(
        frame,
        chunks[2],
        "Serwer",
        &app.login_server,
        app.login_field == LoginField::Server,
        false,
        compact_inputs,
        t,
    );
    draw_input(
        frame,
        chunks[3],
        "Login",
        &app.login_user,
        app.login_field == LoginField::Username,
        false,
        compact_inputs,
        t,
    );
    draw_input(
        frame,
        chunks[4],
        "Hasło",
        &app.login_pass,
        app.login_field == LoginField::Password,
        true,
        compact_inputs,
        t,
    );

    let msg = if app.login_busy {
        Line::from(Span::styled("Łączenie z serwerem...", Style::default().fg(t.accent2)))
    } else if let Some(err) = &app.login_error {
        Line::from(Span::styled(err.as_str(), Style::default().fg(t.error)))
    } else {
        Line::from(Span::styled(
            "Sesja zostanie zapamiętana na tym komputerze.",
            Style::default().fg(t.text3),
        ))
    };
    frame.render_widget(Paragraph::new(msg).wrap(Wrap { trim: true }), chunks[6]);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("[Tab/↑↓]", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::styled(" pole   ", Style::default().fg(t.text2)),
            Span::styled("[Enter]", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::styled(" zaloguj   ", Style::default().fg(t.text2)),
            Span::styled("[Esc]", Style::default().fg(t.accent3).add_modifier(Modifier::BOLD)),
            Span::styled(" wyjdź", Style::default().fg(t.text2)),
        ])),
        chunks[7],
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_input(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    secret: bool,
    compact: bool,
    t: Theme,
) {
    let shown = if secret {
        "•".repeat(value.chars().count())
    } else {
        value.to_string()
    };
    let cursor = if focused { "▌" } else { "" };
    let field_bg = if focused { t.select_bg } else { t.bg };

    if compact {
        let line = Line::from(vec![
            Span::styled(
                format!("{label:<7}: "),
                Style::default()
                    .fg(if focused { t.accent } else { t.text2 })
                    .add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() }),
            ),
            Span::styled(shown, Style::default().fg(t.text).bg(field_bg)),
            Span::styled(cursor, Style::default().fg(t.accent).bg(field_bg)),
        ]);
        frame.render_widget(Paragraph::new(line).style(Style::default().bg(field_bg)), area);
        return;
    }

    let border_col = if focused { t.accent } else { t.border };
    let block = Block::default()
        .title(Span::styled(
            format!(" {label} "),
            Style::default()
                .fg(if focused { t.accent } else { t.text2 })
                .add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() }),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_col))
        .style(Style::default().bg(field_bg));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default().bg(field_bg)),
            Span::styled(shown, Style::default().fg(t.text).bg(field_bg)),
            Span::styled(cursor, Style::default().fg(t.accent).bg(field_bg)),
        ]))
        .block(block),
        area,
    );
}

fn draw_main(frame: &mut Frame, app: &App, snap: &PlayerSnapshot, t: Theme) {
    let studio_active = app.layout == LayoutMode::Studio && frame.area().width >= 76;
    let footer_h = if studio_active { 3 } else { 5 };

    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),        // header
            Constraint::Min(10),          // body
            Constraint::Length(footer_h), // player bar / slim keybar
        ])
        .split(frame.area());

    draw_header(frame, root[0], app, t);

    let sidebar_w = if studio_active { 24 } else { 26 };
    let body_constraints = if studio_active {
        let studio_w = (frame.area().width / 3).clamp(34, 44);
        vec![
            Constraint::Length(sidebar_w),
            Constraint::Min(24),
            Constraint::Length(studio_w),
        ]
    } else if app.show_lyrics && frame.area().width >= 90 {
        vec![
            Constraint::Length(sidebar_w),
            Constraint::Min(28),
            Constraint::Length(36),
        ]
    } else {
        vec![Constraint::Length(sidebar_w), Constraint::Min(28)]
    };

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(body_constraints)
        .split(root[1]);

    draw_sidebar(frame, body[0], app, t);

    if app.is_settings_view() {
        draw_settings(frame, body[1], app, t);
    } else {
        draw_tracks(frame, body[1], app, snap, t);
    }

    if studio_active && body.len() > 2 {
        draw_studio_panel(frame, body[2], app, snap, t);
    } else if app.show_lyrics && body.len() > 2 {
        draw_default_lyrics_panel(frame, body[2], app, snap, t);
    }

    if studio_active {
        draw_studio_footer(frame, root[2], app, snap, t);
    } else {
        draw_player_bar(frame, root[2], app, snap, t);
    }

    if let Some(modal) = &app.modal {
        draw_modal(frame, modal, app, t);
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App, t: Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.border))
        .style(Style::default().bg(t.surface));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(inner);

    let user = app
        .session
        .as_ref()
        .map(|s| s.username.as_str())
        .unwrap_or("");

    let sync_badge = if app.sync_connected {
        if app.sync_devices > 1 {
            Span::styled(
                format!("  ◉ SYNC({})", app.sync_devices),
                Style::default().fg(t.accent),
            )
        } else {
            Span::styled("  ◉ SYNC", Style::default().fg(t.accent))
        }
    } else {
        Span::styled("  ○ SYNC", Style::default().fg(t.text3))
    };

    let left = Line::from(vec![
        Span::styled(" ♪ ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        Span::styled("SOUNDWAVE", Style::default().fg(t.text).add_modifier(Modifier::BOLD)),
        Span::styled(" CLI", Style::default().fg(t.accent2).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  @{user}"), Style::default().fg(t.text2)),
        sync_badge,
    ]);

    let right_text = if let Some(msg) = app.active_status() {
        Line::from(Span::styled(msg, Style::default().fg(t.accent)))
    } else if app.loading {
        Line::from(Span::styled("⏳ Ładowanie...", Style::default().fg(t.accent2)))
    } else {
        let srv = app
            .session
            .as_ref()
            .map(|s| s.server_url.trim_start_matches("https://").trim_start_matches("http://"))
            .unwrap_or("");
        Line::from(Span::styled(srv, Style::default().fg(t.text3)))
    };

    frame.render_widget(Paragraph::new(left), cols[0]);
    frame.render_widget(Paragraph::new(right_text).alignment(Alignment::Right), cols[1]);
}

fn draw_sidebar(frame: &mut Frame, area: Rect, app: &App, t: Theme) {
    let focused = app.focus == Focus::Sidebar;
    let block = Block::default()
        .title(Span::styled(
            " Biblioteka ",
            Style::default()
                .fg(if focused { t.accent } else { t.text2 })
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { t.border_focus } else { t.border }))
        .style(Style::default().bg(t.surface));

    let items: Vec<ListItem> = app
        .nav_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let label = match item {
                NavItem::Queue => format!("≡  Kolejka ({})", app.queue.len()),
                _ => item.label(),
            };
            let color = match item {
                NavItem::Liked => t.accent3,
                NavItem::Search => t.accent,
                NavItem::Local | NavItem::Queue => t.accent2,
                NavItem::Playlist { .. } => t.text,
                NavItem::Settings => t.accent,
            };
            let style = if i == app.nav_index {
                Style::default()
                    .fg(color)
                    .bg(t.select_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color)
            };
            ListItem::new(Line::from(Span::styled(format!(" {label}"), style)))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.nav_index));

    let list = List::new(items).block(block);
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_settings(frame: &mut Frame, area: Rect, app: &App, t: Theme) {
    let focused = app.focus == Focus::Settings;
    let block = Block::default()
        .title(Span::styled(
            " ⚙ Ustawienia ",
            Style::default()
                .fg(if focused { t.accent } else { t.text })
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { t.border_focus } else { t.border }))
        .style(Style::default().bg(t.surface));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        " MOTYW (KOLORY)",
        Style::default().fg(t.accent2).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    for (i, &tid) in ThemeId::ALL.iter().enumerate() {
        let preview = tid.theme();
        let active = app.theme_id == tid;
        let selected = focused && app.settings_index == i;
        let bg = if selected { t.select_bg } else { t.surface };
        let bullet = if active { "●" } else { "○" };

        lines.push(Line::from(vec![
            Span::styled(
                format!(" {bullet} "),
                Style::default()
                    .fg(if active { t.accent } else { t.text3 })
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("●", Style::default().fg(preview.accent).bg(bg)),
            Span::styled("●", Style::default().fg(preview.accent2).bg(bg)),
            Span::styled("● ", Style::default().fg(preview.accent3).bg(bg)),
            Span::styled(
                format!("{:<12} ", tid.name()),
                Style::default()
                    .fg(if active || selected { t.text } else { t.text2 })
                    .bg(bg)
                    .add_modifier(if active || selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            Span::styled(tid.desc(), Style::default().fg(t.text3).bg(bg)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " UKŁAD (LAYOUT)",
        Style::default().fg(t.accent2).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let base_idx = ThemeId::ALL.len();
    for (j, &mode) in LayoutMode::ALL.iter().enumerate() {
        let idx = base_idx + j;
        let active = app.layout == mode;
        let selected = focused && app.settings_index == idx;
        let bg = if selected { t.select_bg } else { t.surface };
        let bullet = if active { "●" } else { "○" };

        lines.push(Line::from(vec![
            Span::styled(
                format!(" {bullet} "),
                Style::default()
                    .fg(if active { t.accent } else { t.text3 })
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<10} ", mode.name()),
                Style::default()
                    .fg(if active || selected { t.text } else { t.text2 })
                    .bg(bg)
                    .add_modifier(if active || selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            Span::styled(mode.desc(), Style::default().fg(t.text3).bg(bg)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" [↑/↓]", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" wybierz   ", Style::default().fg(t.text3)),
        Span::styled("[Enter]", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" zastosuj (zapisuje się automatycznie)   ", Style::default().fg(t.text3)),
        Span::styled("[Tab]", Style::default().fg(t.accent2).add_modifier(Modifier::BOLD)),
        Span::styled(" biblioteka", Style::default().fg(t.text3)),
    ]));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_tracks(frame: &mut Frame, area: Rect, app: &App, snap: &PlayerSnapshot, t: Theme) {
    let show_search =
        app.focus == Focus::SearchInput || matches!(app.nav_items.get(app.nav_index), Some(NavItem::Search));

    let (search_area, list_area) = if show_search {
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(5)])
            .split(area);
        (Some(split[0]), split[1])
    } else {
        (None, area)
    };

    if let Some(sa) = search_area {
        let s_focused = app.focus == Focus::SearchInput;
        let cursor = if s_focused { "▌" } else { "" };
        let placeholder = if app.search_query.is_empty() && !s_focused {
            "Wciśnij [/] aby szukać na YouTube Music lub wkleić link..."
        } else {
            ""
        };
        let s_block = Block::default()
            .title(Span::styled(
                " Szukaj (Enter = szukaj, Esc = lista) ",
                Style::default().fg(if s_focused { t.accent } else { t.text2 }),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(if s_focused { t.accent } else { t.border }))
            .style(Style::default().bg(t.surface));
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ⌕ ", Style::default().fg(t.accent)),
                Span::styled(&app.search_query, Style::default().fg(t.text)),
                Span::styled(cursor, Style::default().fg(t.accent)),
                Span::styled(placeholder, Style::default().fg(t.text3)),
            ]))
            .block(s_block),
            sa,
        );
    }

    let focused = app.focus == Focus::Tracks;
    let block = Block::default()
        .title(Span::styled(
            format!(" {} ", app.view_title),
            Style::default()
                .fg(if focused { t.accent } else { t.text })
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { t.border_focus } else { t.border }))
        .style(Style::default().bg(t.surface));

    if app.tracks.is_empty() {
        let empty_msg = if app.loading {
            "Ładowanie utworów..."
        } else {
            "Brak utworów do wyświetlenia. Wciśnij [/], żeby wyszukać muzykę."
        };
        frame.render_widget(
            Paragraph::new(Span::styled(empty_msg, Style::default().fg(t.text3)))
                .alignment(Alignment::Center)
                .block(block),
            list_area,
        );
        return;
    }

    let inner_w = list_area.width.saturating_sub(2) as usize;
    let dur_w = 6usize;
    let heart_w = 2usize;
    let num_w = 4usize;
    let remain = inner_w.saturating_sub(num_w + heart_w + dur_w + 5);
    let title_w = (remain * 58 / 100).max(10);
    let artist_w = remain.saturating_sub(title_w).max(6);

    let playing_id = snap.track.as_ref().map(|tr| tr.id.as_str()).unwrap_or("");

    let items: Vec<ListItem> = app
        .tracks
        .iter()
        .enumerate()
        .map(|(i, tr)| {
            let is_playing = tr.id == playing_id && snap.status != PlaybackStatus::Stopped;
            let is_selected = i == app.track_index;
            let liked = app.liked_ids.contains(&tr.id);

            let num_str = if is_playing {
                " ▶ ".to_string()
            } else {
                format!("{:>2}. ", i + 1)
            };

            let title_cut = truncate(&tr.title, title_w);
            let artist_cut = truncate(&tr.artist, artist_w);
            let heart = if liked { "♥" } else { " " };
            let dur = if tr.duration.is_empty() {
                " --:--"
            } else {
                tr.duration.as_str()
            };

            let bg = if is_selected { t.select_bg } else { t.surface };
            let title_col = if is_playing { t.accent } else { t.text };

            let line = Line::from(vec![
                Span::styled(
                    format!("{num_str:<num_w$}"),
                    Style::default()
                        .fg(if is_playing { t.accent } else { t.text3 })
                        .bg(bg),
                ),
                Span::styled(
                    format!("{title_cut:<title_w$} "),
                    Style::default()
                        .fg(title_col)
                        .bg(bg)
                        .add_modifier(if is_playing || is_selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(
                    format!("{artist_cut:<artist_w$} "),
                    Style::default().fg(t.text2).bg(bg),
                ),
                Span::styled(
                    format!("{heart} "),
                    Style::default().fg(t.accent3).bg(bg),
                ),
                Span::styled(
                    format!("{dur:>dur_w$}"),
                    Style::default().fg(t.text3).bg(bg),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.track_index));

    let list = List::new(items).block(block);
    frame.render_stateful_widget(list, list_area, &mut state);
}

// ============================================================================
// STUDIO PANEL (zawsze po prawej w trybie Studio):
// 1. Góra: 16-pasmowy Audio Visualizer (w miejscu okładki albumu)
// 2. Środek: Tytuł, Wykonawca, Pasek postępu, Kontrolki
// 3. Dół: Zakładki Kolejka / Tekst (przełączane przez [t])
// ============================================================================
fn draw_studio_panel(frame: &mut Frame, area: Rect, app: &App, snap: &PlayerSnapshot, t: Theme) {
    let outer = Block::default()
        .title(Span::styled(
            " Teraz gra · Studio ",
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.border))
        .style(Style::default().bg(t.surface));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let viz_h = if inner.height >= 24 { 8 } else { 6 };

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(1)
        .constraints([
            Constraint::Length(viz_h), // 0: Audio Visualizer
            Constraint::Length(1),     // 1: spacer
            Constraint::Length(5),     // 2: title + artist + progress + controls
            Constraint::Length(1),     // 3: spacer
            Constraint::Length(1),     // 4: tabs [Następne] [Tekst]
            Constraint::Min(4),        // 5: queue or synced lyrics
        ])
        .split(inner);

    // 1. Audio Visualizer w miejscu okładki albumu
    draw_visualizer_box(frame, sections[0], snap, t);

    // 2. Tytuł, wykonawca, pasek postępu i kontrolki Studio
    let ctrl_w = sections[2].width as usize;
    let is_playing = snap.status == PlaybackStatus::Playing;
    let liked = snap
        .track
        .as_ref()
        .is_some_and(|tr| app.liked_ids.contains(&tr.id));

    let (title_line, artist_line) = if let Some(tr) = &snap.track {
        let mut t_spans = vec![Span::styled(
            truncate(&tr.title, ctrl_w.saturating_sub(3)),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        )];
        if liked {
            t_spans.push(Span::styled(" ♥", Style::default().fg(t.accent3)));
        }
        (
            Line::from(t_spans),
            Line::from(Span::styled(
                truncate(&tr.artist, ctrl_w),
                Style::default().fg(t.text2),
            )),
        )
    } else {
        (
            Line::from(Span::styled(
                "Nic nie gra",
                Style::default().fg(t.text2).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Wybierz utwór z listy",
                Style::default().fg(t.text3),
            )),
        )
    };

    let pos_str = fmt_secs(snap.position_secs);
    let dur_str = if snap.duration_secs > 0.5 {
        fmt_secs(snap.duration_secs)
    } else {
        "--:--".to_string()
    };
    let bar_w = ctrl_w.saturating_sub(13).max(6);
    let ratio = if snap.duration_secs > 0.5 {
        (snap.position_secs / snap.duration_secs).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = ((bar_w as f64) * ratio).round() as usize;
    let empty = bar_w.saturating_sub(filled.min(bar_w));

    let prog_line = Line::from(vec![
        Span::styled(format!("{pos_str} "), Style::default().fg(t.text3)),
        Span::styled("━".repeat(filled.min(bar_w)), Style::default().fg(t.accent)),
        Span::styled("●", Style::default().fg(t.text)),
        Span::styled("─".repeat(empty.saturating_sub(1)), Style::default().fg(t.text3)),
        Span::styled(format!(" {dur_str}"), Style::default().fg(t.text3)),
    ]);

    let play_badge = match snap.status {
        PlaybackStatus::Playing => "⏸",
        PlaybackStatus::Buffering => "◌",
        _ => "▶",
    };

    let controls1 = Line::from(vec![
        Span::styled("⏮ ", Style::default().fg(t.text)),
        Span::styled("[p]  ", Style::default().fg(t.text3)),
        Span::styled(
            format!(" {play_badge} "),
            Style::default()
                .fg(t.bg)
                .bg(if is_playing { t.accent } else { t.accent2 })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" [Spc]  ", Style::default().fg(t.text3)),
        Span::styled("⏭ ", Style::default().fg(t.text)),
        Span::styled("[n]  ", Style::default().fg(t.text3)),
        Span::styled(
            if liked { "♥" } else { "♡" },
            Style::default().fg(if liked { t.accent3 } else { t.text2 }),
        ),
        Span::styled(" [l]", Style::default().fg(t.text3)),
    ]);

    let controls2 = Line::from(vec![
        Span::styled("SHF:", Style::default().fg(t.text3)),
        Span::styled(
            if app.shuffle { "ON" } else { "OFF" },
            Style::default().fg(if app.shuffle { t.accent } else { t.text3 }),
        ),
        Span::styled(" [z]  RPT:", Style::default().fg(t.text3)),
        Span::styled(
            app.repeat.label(),
            Style::default().fg(if app.repeat != RepeatMode::Off {
                t.accent2
            } else {
                t.text3
            }),
        ),
        Span::styled(" [o]  ", Style::default().fg(t.text3)),
        Span::styled("📻 [r]", Style::default().fg(t.accent2)),
    ]);

    frame.render_widget(
        Paragraph::new(vec![title_line, artist_line, prog_line, controls1, controls2]),
        sections[2],
    );

    // 3. Zakładki [Następne] / [Tekst] (przełączane przez [t])
    let q_active = app.studio_tab == StudioTab::Queue;
    let l_active = app.studio_tab == StudioTab::Lyrics;
    let upcoming_count = app
        .queue_index
        .map(|idx| app.queue.len().saturating_sub(idx + 1))
        .unwrap_or(app.queue.len());

    let tabs_line = Line::from(vec![
        Span::styled(
            format!(" Następne ({upcoming_count}) "),
            Style::default()
                .fg(if q_active { t.accent } else { t.text3 })
                .bg(if q_active { t.select_bg } else { t.surface })
                .add_modifier(if q_active { Modifier::BOLD } else { Modifier::empty() }),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(
            " Tekst ",
            Style::default()
                .fg(if l_active { t.accent } else { t.text3 })
                .bg(if l_active { t.select_bg } else { t.surface })
                .add_modifier(if l_active { Modifier::BOLD } else { Modifier::empty() }),
        ),
        Span::styled("   [t]", Style::default().fg(t.text3)),
    ]);
    frame.render_widget(Paragraph::new(tabs_line), sections[4]);

    // 4. Zawartość dolnej zakładki: Kolejka lub zsynchronizowany Tekst
    match app.studio_tab {
        StudioTab::Queue => draw_studio_queue(frame, sections[5], app, t),
        StudioTab::Lyrics => draw_lyrics_content(frame, sections[5], app, snap, t),
    }
}

fn draw_visualizer_box(frame: &mut Frame, area: Rect, snap: &PlayerSnapshot, t: Theme) {
    let status_lbl = match snap.status {
        PlaybackStatus::Playing => " ● LIVE ",
        PlaybackStatus::Paused => " ⏸ PAUZA ",
        PlaybackStatus::Buffering => " ◌ BUFOR ",
        PlaybackStatus::Stopped => " ○ AUDIO ",
    };
    let box_block = Block::default()
        .title(Span::styled(
            status_lbl,
            Style::default()
                .fg(if snap.status == PlaybackStatus::Playing {
                    t.accent
                } else {
                    t.text3
                })
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.border))
        .style(Style::default().bg(t.bg));
    let inner = box_block.inner(area);
    frame.render_widget(box_block, area);

    let rows = inner.height as usize;
    let cols = inner.width as usize;
    if rows == 0 || cols == 0 {
        return;
    }

    const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let is_playing = snap.status == PlaybackStatus::Playing;

    // Dopasuj 16 pasm do szerokości ramki (po 2 znaki na pasmo, jeśli jest miejsce)
    let bar_w = if cols >= SPECTRUM_BANDS * 2 { 2 } else { 1 };
    let used_w = (SPECTRUM_BANDS * bar_w).min(cols);
    let pad_left = cols.saturating_sub(used_w) / 2;
    let shown_bands = used_w / bar_w;

    let mut viz_lines = Vec::with_capacity(rows);
    for r in 0..rows {
        let row_from_bottom = rows - 1 - r;
        let mut spans = Vec::new();
        if pad_left > 0 {
            spans.push(Span::raw(" ".repeat(pad_left)));
        }

        // Gradient wysokościowy: dół = accent, środek = accent2, szczyty = accent3
        let row_color = if row_from_bottom + 1 >= rows {
            t.accent3
        } else if row_from_bottom * 2 >= rows {
            t.accent2
        } else {
            t.accent
        };

        for b in 0..shown_bands {
            let level = if is_playing { snap.bands[b] } else { 0.02 };
            let total_units = level * (rows as f32) * 8.0;
            let row_start = (row_from_bottom as f32) * 8.0;
            let fill = (total_units - row_start).clamp(0.0, 8.0).round() as usize;
            let ch = if !is_playing && row_from_bottom == 0 {
                '·'
            } else {
                BLOCKS[fill]
            };
            let cell = if bar_w == 2 {
                format!("{ch} ")
            } else {
                ch.to_string()
            };
            spans.push(Span::styled(
                cell,
                Style::default().fg(if is_playing { row_color } else { t.text3 }),
            ));
        }
        viz_lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(viz_lines), inner);
}

fn draw_studio_queue(frame: &mut Frame, area: Rect, app: &App, t: Theme) {
    let start = app.queue_index.map(|i| i + 1).unwrap_or(0);
    if start >= app.queue.len() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled("Koniec kolejki.", Style::default().fg(t.text3))),
                Line::from(Span::styled(
                    "Wciśnij [r], żeby odpalić Radio.",
                    Style::default().fg(t.text3),
                )),
            ])
            .alignment(Alignment::Center),
            area,
        );
        return;
    }

    let max_w = area.width.saturating_sub(4) as usize;
    let max_rows = area.height as usize;
    let mut lines = Vec::new();
    for (offset, tr) in app.queue[start..].iter().take(max_rows).enumerate() {
        let idx = offset + 1;
        let combined = format!("{} — {}", tr.title, tr.artist);
        lines.push(Line::from(vec![
            Span::styled(format!("{idx:>2}. "), Style::default().fg(t.text3)),
            Span::styled(truncate(&combined, max_w), Style::default().fg(t.text2)),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_default_lyrics_panel(frame: &mut Frame, area: Rect, app: &App, snap: &PlayerSnapshot, t: Theme) {
    let badge = if !app.lyrics.synced.is_empty() {
        " Tekst · SYNC (t) "
    } else {
        " Tekst (t) "
    };
    let block = Block::default()
        .title(Span::styled(
            badge,
            Style::default().fg(t.accent2).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.border))
        .style(Style::default().bg(t.surface));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    draw_lyrics_content(frame, inner, app, snap, t);
}

/// Wspólny renderer tekstu piosenki: pełne zawijanie wierszy, pogrubiona aktywna linia
/// oraz płynne auto-scrollowanie (wyśrodkowane na aktualnie śpiewanym wersie).
fn draw_lyrics_content(frame: &mut Frame, area: Rect, app: &App, snap: &PlayerSnapshot, t: Theme) {
    if area.height == 0 || area.width < 4 {
        return;
    }

    if app.lyrics.loading {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled("⏳ Szukam tekstu...", Style::default().fg(t.accent2))),
            ])
            .alignment(Alignment::Center),
            area,
        );
        return;
    }

    if !app.lyrics.found {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Brak tekstu dla bieżącego utworu.",
                    Style::default().fg(t.text3),
                )),
            ])
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let visible_rows = area.height as usize;
    let wrap_w = area.width.saturating_sub(3).max(8) as usize;

    if !app.lyrics.synced.is_empty() {
        let pos = snap.position_secs;
        let active_idx = app
            .lyrics
            .synced
            .iter()
            .rposition(|(ts, _)| *ts <= pos + 0.35);

        let mut rendered: Vec<Line> = Vec::new();
        let mut active_visual_row: usize = 0;

        for (idx, (_, raw_text)) in app.lyrics.synced.iter().enumerate() {
            let is_active = active_idx == Some(idx);
            let is_past = active_idx.is_some_and(|a| idx < a);
            let is_next = active_idx.is_some_and(|a| idx == a + 1);

            if is_active {
                active_visual_row = rendered.len();
            }

            let display = if raw_text.is_empty() { "♪" } else { raw_text.as_str() };
            let wrapped = wrap_text(display, wrap_w);

            for (w_i, part) in wrapped.into_iter().enumerate() {
                if is_active {
                    let prefix = if w_i == 0 { "▸ " } else { "  " };
                    rendered.push(Line::from(vec![
                        Span::styled(
                            prefix,
                            Style::default()
                                .fg(t.accent)
                                .bg(t.select_bg)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            part,
                            Style::default()
                                .fg(t.accent)
                                .bg(t.select_bg)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]));
                } else {
                    let color = if is_past {
                        t.text3
                    } else if is_next {
                        t.text
                    } else {
                        t.text2
                    };
                    rendered.push(Line::from(Span::styled(
                        format!("  {part}"),
                        Style::default().fg(color),
                    )));
                }
            }
        }

        // Wyśrodkowanie aktualnie śpiewanej linii i płynny scroll wraz z upływem utworu
        let half = visible_rows / 2;
        let max_scroll = rendered.len().saturating_sub(visible_rows);
        let scroll_y = active_visual_row.saturating_sub(half).min(max_scroll);

        frame.render_widget(
            Paragraph::new(rendered).scroll((scroll_y as u16, 0)),
            area,
        );
    } else {
        // Zwykły tekst bez timestampów — delikatne przewijanie proporcjonalne do postępu utworu
        let mut rendered: Vec<Line> = Vec::new();
        for raw in &app.lyrics.plain {
            if raw.trim().is_empty() {
                rendered.push(Line::from(""));
                continue;
            }
            for part in wrap_text(raw, wrap_w) {
                rendered.push(Line::from(Span::styled(
                    format!(" {part}"),
                    Style::default().fg(t.text2),
                )));
            }
        }
        let max_scroll = rendered.len().saturating_sub(visible_rows);
        let ratio = if snap.duration_secs > 1.0 {
            (snap.position_secs / snap.duration_secs).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let scroll_y = ((max_scroll as f64) * ratio).round() as usize;
        frame.render_widget(
            Paragraph::new(rendered).scroll((scroll_y as u16, 0)),
            area,
        );
    }
}

fn draw_studio_footer(frame: &mut Frame, area: Rect, _app: &App, snap: &PlayerSnapshot, t: Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.border))
        .style(Style::default().bg(t.surface));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(14)])
        .split(inner);

    let keys = Line::from(vec![
        Span::styled(" [/]", Style::default().fg(t.accent)),
        Span::styled(" Szukaj ", Style::default().fg(t.text3)),
        Span::styled("[Tab]", Style::default().fg(t.accent)),
        Span::styled(" Panel ", Style::default().fg(t.text3)),
        Span::styled("[t]", Style::default().fg(t.accent2)),
        Span::styled(" Kol/Tekst ", Style::default().fg(t.text3)),
        Span::styled("[d]", Style::default().fg(t.accent2)),
        Span::styled(" +Playl ", Style::default().fg(t.text3)),
        Span::styled("[c]", Style::default().fg(t.accent2)),
        Span::styled(" Nowa PL ", Style::default().fg(t.text3)),
        Span::styled("[a]", Style::default().fg(t.accent2)),
        Span::styled(" +Kol ", Style::default().fg(t.text3)),
        Span::styled("[←/→]", Style::default().fg(t.accent)),
        Span::styled(" ±5s ", Style::default().fg(t.text3)),
        Span::styled("[q]", Style::default().fg(t.error)),
        Span::styled(" Wyjdź", Style::default().fg(t.text3)),
    ]);

    let vol = Line::from(Span::styled(
        format!("VOL {:>3}% ", snap.volume),
        Style::default().fg(t.text).add_modifier(Modifier::BOLD),
    ));

    frame.render_widget(Paragraph::new(keys), cols[0]);
    frame.render_widget(Paragraph::new(vol).alignment(Alignment::Right), cols[1]);
}

fn draw_player_bar(frame: &mut Frame, area: Rect, app: &App, snap: &PlayerSnapshot, t: Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.border))
        .style(Style::default().bg(t.surface));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let status_icon = match snap.status {
        PlaybackStatus::Playing => ("▶", t.accent),
        PlaybackStatus::Paused => ("⏸", t.accent2),
        PlaybackStatus::Buffering => ("◌", t.accent2),
        PlaybackStatus::Stopped => ("■", t.text3),
    };

    let mut left_spans = vec![Span::styled(
        format!(" {} ", status_icon.0),
        Style::default()
            .fg(status_icon.1)
            .add_modifier(Modifier::BOLD),
    )];

    if let Some(tr) = &snap.track {
        left_spans.push(Span::styled(
            tr.title.clone(),
            Style::default().fg(t.text).add_modifier(Modifier::BOLD),
        ));
        left_spans.push(Span::styled("  —  ", Style::default().fg(t.text3)));
        left_spans.push(Span::styled(tr.artist.clone(), Style::default().fg(t.text2)));
        if app.liked_ids.contains(&tr.id) {
            left_spans.push(Span::styled("  ♥", Style::default().fg(t.accent3)));
        }
    } else {
        left_spans.push(Span::styled(
            "Nic nie gra — wybierz utwór i wciśnij Enter",
            Style::default().fg(t.text3),
        ));
    }

    let wave = mini_wave(snap.level, snap.status == PlaybackStatus::Playing);
    let right_line = Line::from(vec![
        Span::styled(wave, Style::default().fg(t.accent)),
        Span::styled("   SHF:", Style::default().fg(t.text3)),
        Span::styled(
            if app.shuffle { "ON" } else { "OFF" },
            Style::default().fg(if app.shuffle { t.accent } else { t.text3 }),
        ),
        Span::styled("  RPT:", Style::default().fg(t.text3)),
        Span::styled(
            app.repeat.label(),
            Style::default().fg(if app.repeat != RepeatMode::Off {
                t.accent2
            } else {
                t.text3
            }),
        ),
        Span::styled(format!("   VOL {:>3}% ", snap.volume), Style::default().fg(t.text)),
    ]);

    let top_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(34)])
        .split(rows[0]);
    frame.render_widget(Paragraph::new(Line::from(left_spans)), top_cols[0]);
    frame.render_widget(Paragraph::new(right_line).alignment(Alignment::Right), top_cols[1]);

    let pos_str = fmt_secs(snap.position_secs);
    let dur_str = if snap.duration_secs > 0.5 {
        fmt_secs(snap.duration_secs)
    } else {
        "--:--".to_string()
    };
    let bar_w = inner.width.saturating_sub(16) as usize;
    let ratio = if snap.duration_secs > 0.5 {
        (snap.position_secs / snap.duration_secs).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = ((bar_w as f64) * ratio).round() as usize;
    let empty = bar_w.saturating_sub(filled.min(bar_w));

    let bar_line = Line::from(vec![
        Span::styled(format!(" {pos_str} "), Style::default().fg(t.text2)),
        Span::styled("━".repeat(filled.min(bar_w)), Style::default().fg(t.accent)),
        Span::styled("●", Style::default().fg(t.text)),
        Span::styled("─".repeat(empty.saturating_sub(1)), Style::default().fg(t.text3)),
        Span::styled(format!(" {dur_str}"), Style::default().fg(t.text2)),
    ]);
    frame.render_widget(Paragraph::new(bar_line), rows[1]);

    let keys = Line::from(vec![
        Span::styled(" [Spc]", Style::default().fg(t.accent)),
        Span::styled(" Pauza ", Style::default().fg(t.text3)),
        Span::styled("[/]", Style::default().fg(t.accent)),
        Span::styled(" Szukaj ", Style::default().fg(t.text3)),
        Span::styled("[Tab]", Style::default().fg(t.accent)),
        Span::styled(" Panel ", Style::default().fg(t.text3)),
        Span::styled("[n/p]", Style::default().fg(t.accent)),
        Span::styled(" Nast/Poprz ", Style::default().fg(t.text3)),
        Span::styled("[l]", Style::default().fg(t.accent3)),
        Span::styled(" Polub ", Style::default().fg(t.text3)),
        Span::styled("[d]", Style::default().fg(t.accent2)),
        Span::styled(" +Playl ", Style::default().fg(t.text3)),
        Span::styled("[c]", Style::default().fg(t.accent2)),
        Span::styled(" Nowa PL ", Style::default().fg(t.text3)),
        Span::styled("[r]", Style::default().fg(t.accent2)),
        Span::styled(" Radio ", Style::default().fg(t.text3)),
        Span::styled("[t]", Style::default().fg(t.accent2)),
        Span::styled(" Tekst ", Style::default().fg(t.text3)),
        Span::styled("[q]", Style::default().fg(t.error)),
        Span::styled(" Wyjdź", Style::default().fg(t.text3)),
    ]);
    frame.render_widget(Paragraph::new(keys), rows[2]);
}

fn draw_modal(frame: &mut Frame, modal: &Modal, app: &App, t: Theme) {
    let area = frame.area();
    match modal {
        Modal::CreatePlaylist {
            name,
            color_idx,
            pending_track,
        } => {
            let w = area.width.min(54).max(34.min(area.width));
            let h = 11u16.min(area.height);
            let rect = Rect::new(
                area.x + area.width.saturating_sub(w) / 2,
                area.y + area.height.saturating_sub(h) / 2,
                w,
                h,
            );
            frame.render_widget(Clear, rect);
            let block = Block::default()
                .title(Span::styled(
                    " + Nowa playlista ",
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(t.accent))
                .style(Style::default().bg(t.surface));
            let inner = block.inner(rect);
            frame.render_widget(block, rect);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .horizontal_margin(2)
                .vertical_margin(1)
                .constraints([
                    Constraint::Length(3), // nazwa
                    Constraint::Length(2), // kolor
                    Constraint::Length(1), // info / pending track
                    Constraint::Length(1), // help
                ])
                .split(inner);

            draw_input(frame, chunks[0], "Nazwa playlisty", name, true, false, false, t);

            let mut color_spans = vec![Span::styled("Kolor: ", Style::default().fg(t.text2))];
            for (i, (cname, hex)) in PLAYLIST_COLORS.iter().enumerate() {
                let col = parse_hex_color(hex);
                let sel = i == *color_idx;
                if sel {
                    color_spans.push(Span::styled(
                        format!("[● {cname}] "),
                        Style::default().fg(col).add_modifier(Modifier::BOLD),
                    ));
                } else {
                    color_spans.push(Span::styled("● ", Style::default().fg(col)));
                }
            }
            frame.render_widget(Paragraph::new(Line::from(color_spans)), chunks[1]);

            let sub = if let Some(tr) = pending_track {
                Line::from(Span::styled(
                    format!("Dodam też: {}", truncate(&tr.title, 36)),
                    Style::default().fg(t.accent2),
                ))
            } else {
                Line::from(Span::styled("", Style::default()))
            };
            frame.render_widget(Paragraph::new(sub), chunks[2]);

            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("[←/→]", Style::default().fg(t.accent)),
                    Span::styled(" kolor  ", Style::default().fg(t.text3)),
                    Span::styled("[Enter]", Style::default().fg(t.accent)),
                    Span::styled(" utwórz  ", Style::default().fg(t.text3)),
                    Span::styled("[Esc]", Style::default().fg(t.accent3)),
                    Span::styled(" anuluj", Style::default().fg(t.text3)),
                ])),
                chunks[3],
            );
        }
        Modal::AddToPlaylist { track, selected } => {
            let total_rows = app.playlists.len() + 1;
            let w = area.width.min(54).max(34.min(area.width));
            let h = (total_rows as u16 + 6).clamp(8, area.height.min(20));
            let rect = Rect::new(
                area.x + area.width.saturating_sub(w) / 2,
                area.y + area.height.saturating_sub(h) / 2,
                w,
                h,
            );
            frame.render_widget(Clear, rect);
            let block = Block::default()
                .title(Span::styled(
                    " + Dodaj do playlisty ",
                    Style::default().fg(t.accent2).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(t.accent2))
                .style(Style::default().bg(t.surface));
            let inner = block.inner(rect);
            frame.render_widget(block, rect);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .horizontal_margin(1)
                .constraints([
                    Constraint::Length(2),
                    Constraint::Min(3),
                    Constraint::Length(1),
                ])
                .split(inner);

            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("Utwór: ", Style::default().fg(t.text3)),
                    Span::styled(
                        truncate(&format!("{} — {}", track.title, track.artist), w as usize - 12),
                        Style::default().fg(t.text).add_modifier(Modifier::BOLD),
                    ),
                ])),
                chunks[0],
            );

            let mut items: Vec<ListItem> = app
                .playlists
                .iter()
                .enumerate()
                .map(|(i, pl)| {
                    let is_sel = i == *selected;
                    let bg = if is_sel { t.select_bg } else { t.surface };
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            if is_sel { " ▸ ♪ " } else { "   ♪ " },
                            Style::default().fg(t.accent).bg(bg),
                        ),
                        Span::styled(
                            format!("{} ", truncate(&pl.name, w as usize - 16)),
                            Style::default()
                                .fg(if is_sel { t.text } else { t.text2 })
                                .bg(bg)
                                .add_modifier(if is_sel { Modifier::BOLD } else { Modifier::empty() }),
                        ),
                        Span::styled(
                            format!("({})", pl.track_count),
                            Style::default().fg(t.text3).bg(bg),
                        ),
                    ]))
                })
                .collect();

            let new_sel = *selected >= app.playlists.len();
            let new_bg = if new_sel { t.select_bg } else { t.surface };
            items.push(ListItem::new(Line::from(Span::styled(
                if new_sel {
                    " ▸ + Utwórz nową playlistę..."
                } else {
                    "   + Utwórz nową playlistę..."
                },
                Style::default()
                    .fg(t.accent)
                    .bg(new_bg)
                    .add_modifier(Modifier::BOLD),
            ))));

            let mut st = ListState::default();
            st.select(Some((*selected).min(items.len().saturating_sub(1))));
            frame.render_stateful_widget(List::new(items), chunks[1], &mut st);

            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("[↑/↓]", Style::default().fg(t.accent)),
                    Span::styled(" wybierz  ", Style::default().fg(t.text3)),
                    Span::styled("[Enter]", Style::default().fg(t.accent)),
                    Span::styled(" dodaj  ", Style::default().fg(t.text3)),
                    Span::styled("[Esc]", Style::default().fg(t.accent3)),
                    Span::styled(" anuluj", Style::default().fg(t.text3)),
                ])),
                chunks[2],
            );
        }
    }
}

fn parse_hex_color(hex: &str) -> Color {
    let h = hex.trim_start_matches('#');
    if h.len() == 6
        && let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&h[0..2], 16),
            u8::from_str_radix(&h[2..4], 16),
            u8::from_str_radix(&h[4..6], 16),
        )
    {
        return Color::Rgb(r, g, b);
    }
    Color::White
}

fn mini_wave(level: f32, playing: bool) -> String {
    if !playing {
        return "▁▁▁▁▁".to_string();
    }
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let weights = [0.55f32, 0.9, 1.0, 0.8, 0.5];
    weights
        .iter()
        .map(|w| {
            let v = (level * *w).clamp(0.0, 1.0);
            BARS[((v * 7.0).round() as usize).min(7)]
        })
        .collect()
}

fn fmt_secs(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn truncate(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else if max_chars > 1 {
        let cut: String = s.chars().take(max_chars - 1).collect();
        format!("{cut}…")
    } else {
        "…".to_string()
    }
}

fn wrap_text(s: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return vec![s.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;

    for word in s.split_whitespace() {
        let wlen = word.chars().count();
        if current_len == 0 {
            if wlen <= max_chars {
                current.push_str(word);
                current_len = wlen;
            } else {
                // Długie słowo bez spacji tniemy po znakach
                let chars: Vec<char> = word.chars().collect();
                for chunk in chars.chunks(max_chars) {
                    lines.push(chunk.iter().collect());
                }
            }
        } else if current_len + 1 + wlen <= max_chars {
            current.push(' ');
            current.push_str(word);
            current_len += 1 + wlen;
        } else {
            lines.push(std::mem::take(&mut current));
            if wlen <= max_chars {
                current.push_str(word);
                current_len = wlen;
            } else {
                let chars: Vec<char> = word.chars().collect();
                for chunk in chars.chunks(max_chars) {
                    lines.push(chunk.iter().collect());
                }
                current_len = 0;
            }
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}
