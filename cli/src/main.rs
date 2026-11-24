mod api;
mod app;
mod config;
mod player;
mod sync;
mod theme;
mod ui;

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::api::ApiClient;
use crate::app::{App, Focus, LoginField, Modal, Screen, PLAYLIST_COLORS};
use crate::config::{clear_session, load_session};

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut override_server: Option<String> = None;
    let mut do_logout = false;
    let mut do_whoami = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--logout" | "logout" => do_logout = true,
            "--whoami" | "whoami" => do_whoami = true,
            "--server" if i + 1 < args.len() => {
                override_server = Some(args[i + 1].clone());
                i += 1;
            }
            "--help" | "-h" => {
                println!("SoundWave CLI (swcli / soundwave-cli)");
                println!("Użycie:");
                println!("  swcli                 Uruchom odtwarzacz (automatyczne logowanie jeśli sesja zapisana)");
                println!("  swcli --logout        Wyloguj i usuń zapisaną sesję z tego komputera");
                println!("  swcli --whoami        Sprawdź aktualnie zapisane konto");
                println!("  swcli --server <URL>  Użyj innego adresu serwera SoundWave");
                return Ok(());
            }
            _ => {}
        }
        i += 1;
    }

    if do_logout {
        if let Some(sess) = load_session() {
            let client = ApiClient::new(&sess.server_url, &sess.token);
            client.logout();
        }
        clear_session();
        println!("Wylogowano z SoundWave CLI i usunięto lokalną sesję.");
        return Ok(());
    }

    // Sprawdź zapisaną sesję (jeśli token nie działa na serwerze, pokaż ekran logowania)
    let mut valid_session = load_session();
    if let Some(ref sess) = valid_session {
        let client = ApiClient::new(&sess.server_url, &sess.token);
        match client.me() {
            Ok((uname, is_admin)) => {
                if do_whoami {
                    println!("Zalogowano jako: @{uname} (admin: {is_admin}) @ {}", sess.server_url);
                    return Ok(());
                }
            }
            Err(err) => {
                if do_whoami {
                    println!("Zapisana sesja nieważna lub brak połączenia: {err}");
                    return Ok(());
                }
                if err.contains("HTTP 401") {
                    clear_session();
                    valid_session = None;
                }
            }
        }
    } else if do_whoami {
        println!("Brak zapisanej sesji. Uruchom `swcli`, aby się zalogować.");
        return Ok(());
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(valid_session, override_server);

    let result = (|| -> io::Result<()> {
        loop {
            let snap = app.tick();
            terminal.draw(|frame| ui::draw(frame, &app, &snap))?;

            if event::poll(Duration::from_millis(60))?
                && let Event::Key(key) = event::read()?
            {
                if matches!(key.kind, KeyEventKind::Release) {
                    continue;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    break;
                }
                if handle_key(&mut app, key) {
                    break;
                }
            }
        }
        Ok(())
    })();

    app.shutdown();
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// Zwraca `true`, jeśli aplikacja ma się zamknąć.
fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    match app.screen {
        Screen::Login => handle_login_key(app, key),
        Screen::Main => handle_main_key(app, key),
    }
}

fn handle_login_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => return true,
        KeyCode::Tab | KeyCode::Down => {
            app.login_field = match app.login_field {
                LoginField::Server => LoginField::Username,
                LoginField::Username => LoginField::Password,
                LoginField::Password => LoginField::Server,
            };
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.login_field = match app.login_field {
                LoginField::Server => LoginField::Password,
                LoginField::Username => LoginField::Server,
                LoginField::Password => LoginField::Username,
            };
        }
        KeyCode::Enter => {
            if app.login_field == LoginField::Username && app.login_pass.is_empty() {
                app.login_field = LoginField::Password;
            } else {
                app.submit_login();
            }
        }
        KeyCode::Backspace => match app.login_field {
            LoginField::Server => {
                app.login_server.pop();
            }
            LoginField::Username => {
                app.login_user.pop();
            }
            LoginField::Password => {
                app.login_pass.pop();
            }
        },
        KeyCode::Char(c)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            match app.login_field {
                LoginField::Server => app.login_server.push(c),
                LoginField::Username => app.login_user.push(c),
                LoginField::Password => app.login_pass.push(c),
            }
        }
        _ => {}
    }
    false
}

fn handle_main_key(app: &mut App, key: KeyEvent) -> bool {
    // Najpierw obsługa aktywnego modala (Nowa playlista / Dodaj do playlisty)
    if app.modal.is_some() {
        handle_modal_key(app, key);
        return false;
    }

    // Tryb wpisywania w wyszukiwarkę
    if app.focus == Focus::SearchInput {
        match key.code {
            KeyCode::Esc => app.focus = Focus::Tracks,
            KeyCode::Enter => app.run_search(),
            KeyCode::Backspace => {
                app.search_query.pop();
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                app.search_query.push(c);
            }
            _ => {}
        }
        return false;
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => return true,
        KeyCode::Char('/') => {
            app.nav_index = 0; // Search
            app.focus = Focus::SearchInput;
        }
        KeyCode::Tab => {
            app.focus = match app.focus {
                Focus::Sidebar => {
                    if app.is_settings_view() {
                        Focus::Settings
                    } else {
                        Focus::Tracks
                    }
                }
                Focus::Tracks | Focus::SearchInput | Focus::Settings => Focus::Sidebar,
            };
        }
        KeyCode::Char(' ') => app.toggle_pause(),
        KeyCode::Char('n') | KeyCode::Char('N') => app.next_track(),
        KeyCode::Char('p') | KeyCode::Char('P') => app.prev_track(),
        KeyCode::Left => app.seek_relative(-5.0),
        KeyCode::Right => app.seek_relative(5.0),
        KeyCode::Char('+') | KeyCode::Char('=') => app.adjust_volume(5),
        KeyCode::Char('-') | KeyCode::Char('_') => app.adjust_volume(-5),
        KeyCode::Char('l') | KeyCode::Char('L') => app.toggle_like_selected(),
        KeyCode::Char('a') | KeyCode::Char('A') => app.enqueue_selected(),
        KeyCode::Char('c') | KeyCode::Char('C') => app.open_create_playlist_modal(None),
        KeyCode::Char('d') | KeyCode::Char('D') => app.open_add_to_playlist_modal(),
        KeyCode::Char('r') | KeyCode::Char('R') => app.start_radio(),
        KeyCode::Char('t') | KeyCode::Char('T') => app.toggle_lyrics_or_studio_tab(),
        KeyCode::Char('z') | KeyCode::Char('Z') => {
            app.shuffle = !app.shuffle;
            app.set_status(if app.shuffle {
                "Losowanie: WŁ"
            } else {
                "Losowanie: WYŁ"
            });
        }
        KeyCode::Char('o') | KeyCode::Char('O') => {
            app.repeat = app.repeat.next();
            app.set_status(format!("Powtarzanie: {}", app.repeat.label()));
        }
        KeyCode::Up | KeyCode::Char('k') => match app.focus {
            Focus::Sidebar => {
                if app.nav_index > 0 {
                    app.nav_index -= 1;
                }
            }
            Focus::Tracks => {
                if app.track_index > 0 {
                    app.track_index -= 1;
                }
            }
            Focus::Settings => {
                if app.settings_index > 0 {
                    app.settings_index -= 1;
                }
            }
            Focus::SearchInput => {}
        },
        KeyCode::Down | KeyCode::Char('j') => match app.focus {
            Focus::Sidebar => {
                if app.nav_index + 1 < app.nav_items.len() {
                    app.nav_index += 1;
                }
            }
            Focus::Tracks => {
                if app.track_index + 1 < app.tracks.len() {
                    app.track_index += 1;
                }
            }
            Focus::Settings => {
                if app.settings_index + 1 < app.settings_count() {
                    app.settings_index += 1;
                }
            }
            Focus::SearchInput => {}
        },
        KeyCode::Enter => match app.focus {
            Focus::Sidebar => app.activate_nav(),
            Focus::Tracks => app.play_selected_track(),
            Focus::Settings => app.activate_setting(),
            Focus::SearchInput => app.run_search(),
        },
        _ => {}
    }
    false
}

fn handle_modal_key(app: &mut App, key: KeyEvent) {
    match app.modal.as_mut() {
        Some(Modal::CreatePlaylist {
            name, color_idx, ..
        }) => match key.code {
            KeyCode::Esc => app.modal = None,
            KeyCode::Enter => app.submit_create_playlist(),
            KeyCode::Left => {
                *color_idx = (*color_idx + PLAYLIST_COLORS.len() - 1) % PLAYLIST_COLORS.len();
            }
            KeyCode::Right | KeyCode::Tab => {
                *color_idx = (*color_idx + 1) % PLAYLIST_COLORS.len();
            }
            KeyCode::Backspace => {
                name.pop();
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if name.chars().count() < 80 {
                    name.push(c);
                }
            }
            _ => {}
        },
        Some(Modal::AddToPlaylist { selected, .. }) => {
            let max_idx = app.playlists.len(); // ostatnia pozycja = "+ Utwórz nową playlistę"
            match key.code {
                KeyCode::Esc => app.modal = None,
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected > 0 {
                        *selected -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected < max_idx {
                        *selected += 1;
                    }
                }
                KeyCode::Enter => app.confirm_add_to_playlist(),
                _ => {}
            }
        }
        None => {}
    }
}
