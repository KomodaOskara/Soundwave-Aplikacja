# SoundWave CLI (`swcli` / `soundwave-cli`)

Lekki terminalowy odtwarzacz dla **SoundWave** (`https://muzyka.komodaoskara.pl`) napisany w Ruście (`ratatui` + `crossterm` + `ureq` + `tungstenite`), działający na **Linuxie** (`ffmpeg` → PipeWire/PulseAudio) oraz **Windowsie** (`ffmpeg` → natywne WASAPI).

## Szybka instalacja

### Linux (x86_64)
```bash
curl -fsSL https://raw.githubusercontent.com/KomodaOskara/Soundwave-Aplikacja/main/cli/install.sh | bash
```
Wymaga `ffmpeg` oraz `paplay` (PipeWire/PulseAudio).

### Windows (PowerShell)
```powershell
irm https://raw.githubusercontent.com/KomodaOskara/Soundwave-Aplikacja/main/cli/install.ps1 | iex
```
Wymaga `ffmpeg` w PATH (`winget install Gyan.FFmpeg`).

## Funkcje
- Logowanie z zapamiętaniem sesji (Linux: `~/.config/soundwave-cli/session.json`, Windows: `%APPDATA%\soundwave-cli\session.json`).
- **Biblioteka:** Wyszukiwarka YouTube Music (+ linki YT), Polubione, Moja muzyka, Twoje playlisty, Kolejka, Ustawienia.
- **Motywy z SoundWave + Nothing OS:** Midnight, Midnight v2, Ember, Ocean, Daylight, Las, Nothing OS.
- **Układy:** `Default` oraz `Studio` (16-pasmowy Audio Visualizer na żywo, kontrolki, zakładki Następne / Tekst).
- **Zsynchronizowany tekst piosenki** (`/api/lyrics`) z pogrubieniem i auto-scrollowaniem aktualnego wersu.
- **Synchronizacja urządzeń (`/ws/sync`)** — CLI pojawia się na liście urządzeń i można nim zdalnie sterować z telefonu/przeglądarki oraz przenosić odtwarzanie w obie strony.
- **Obecność (`/api/presence`) i historia odsłuchów (`/api/stats/play`)** — liczy się do Statystyk i Wrapped.
- **Tworzenie playlist (`c`) i dodawanie utworów do playlist (`d`)**.

## Skróty klawiszowe
| Klawisz | Akcja |
| --- | --- |
| `Tab` | Przełącz panel (Biblioteka ↔ Lista / Ustawienia) |
| `↑` / `↓` (lub `j`/`k`) | Nawigacja |
| `Enter` | Otwórz / Odtwórz / Zastosuj |
| `/` | Wyszukiwarka |
| `Space` | Pauza / Wznów |
| `n` / `p` | Następny / Poprzedni utwór |
| `←` / `→` | Przewiń -5 s / +5 s |
| `+` / `-` | Głośność +5% / -5% |
| `l` | Polub / Odlub |
| `a` | Dodaj do kolejki |
| `d` | Dodaj do playlisty |
| `c` | Nowa playlista |
| `r` | Radio (`/api/radio`) |
| `t` | Przełącz Tekst / Kolejkę (w Studio) lub pokaż Tekst (w Default) |
| `z` / `o` | Shuffle / Repeat |
| `q` / `Ctrl+C` | Wyjście |
