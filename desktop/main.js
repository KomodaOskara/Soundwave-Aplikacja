const { app, BrowserWindow, Menu } = require('electron');
const path = require('path');

// ═══════════════════════════════════════════════════════════════════════════
// SoundWave — aplikacja desktopowa (Electron)
//
// Względem wersji 1.0.0 dochodzi Discord Rich Presence oraz porządki
// w konfiguracji okna. Sama aplikacja nadal jest tylko oknem na
// https://muzyka.komodaoskara.pl — cała logika siedzi po stronie web.
// ═══════════════════════════════════════════════════════════════════════════

const ADRES = 'https://muzyka.komodaoskara.pl';

// Identyfikator aplikacji z https://discord.com/developers/applications
// (New Application → skopiuj "Application ID").
//
// Workflow wstawia go w miejsce placeholdera ze zmiennej repozytorium
// DISCORD_APP_ID (Settings → Secrets and variables → Actions → Variables).
// Przy buildzie bez tej zmiennej placeholder zostaje nieruszony — stąd
// sprawdzenie formatu: bez poprawnego ID integracja po prostu się nie włącza.
const _DISCORD_SUROWY = process.env.SOUNDWAVE_DISCORD_APP_ID || '__DISCORD_APP_ID__';
const DISCORD_APP_ID = /^\d{15,22}$/.test(_DISCORD_SUROWY) ? _DISCORD_SUROWY : '';

// Klucz obrazka wgranego w portalu Discorda w zakładce Rich Presence → Art Assets.
const DISCORD_LOGO = 'soundwave';

const ODPYTYWANIE_MS = 8000;

// ── Jedna instancja ────────────────────────────────────────────────────────
// Dwie kopie aplikacji oznaczałyby dwóch klientów RPC bijących się o ten sam
// status w Discordzie — i dwa okna odtwarzające muzykę naraz.
if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {

let okno = null;

// ═══ DISCORD RICH PRESENCE ═════════════════════════════════════════════════
// Dlaczego tutaj, a nie w aplikacji webowej: Rich Presence wymaga połączenia
// z lokalnym gniazdem IPC klienta Discorda (named pipe na Windowsie, socket
// uniksowy na Linuksie). Przeglądarka nie ma do niego dostępu i mieć nie
// będzie — to celowa granica piaskownicy. Proces główny Electrona to zwykły
// Node.js, więc dostęp ma.

let rpc = null;
let rpcGotowy = false;
let ostatniPodpis = '';

function ustawAktywnosc(dane) {
  // Dwa warianty API: nowszy @xhayper/discord-rpc trzyma to pod client.user,
  // starszy discord-rpc wystawia wprost na kliencie.
  if (rpc && rpc.user && typeof rpc.user.setActivity === 'function') return rpc.user.setActivity(dane);
  if (rpc && typeof rpc.setActivity === 'function') return rpc.setActivity(dane);
  return Promise.resolve();
}

function wyczyscAktywnosc() {
  if (rpc && rpc.user && typeof rpc.user.clearActivity === 'function') return rpc.user.clearActivity();
  if (rpc && typeof rpc.clearActivity === 'function') return rpc.clearActivity();
  return Promise.resolve();
}

async function polaczDiscord() {
  if (!DISCORD_APP_ID) {
    console.log('[discord] brak SOUNDWAVE_DISCORD_APP_ID — integracja wyłączona');
    return;
  }
  let RPC;
  try {
    RPC = require('@xhayper/discord-rpc');
  } catch (e) {
    console.warn('[discord] brak biblioteki @xhayper/discord-rpc:', e.message);
    return;
  }
  try {
    rpc = new RPC.Client({ clientId: DISCORD_APP_ID });
    rpc.on('ready', () => {
      rpcGotowy = true;
      console.log('[discord] połączono z klientem');
    });
    rpc.on('disconnected', () => {
      rpcGotowy = false;
      console.log('[discord] rozłączono');
    });
    await rpc.login();
  } catch (e) {
    // Discord może być wyłączony albo w ogóle nieobecny — to normalna sytuacja,
    // aplikacja musi działać dalej bez żadnego komunikatu dla użytkownika.
    console.warn('[discord] nie udało się połączyć:', e.message);
    rpc = null;
  }
}

async function odswiezObecnosc() {
  if (!rpc || !rpcGotowy || !okno || okno.isDestroyed()) return;

  let stan = null;
  try {
    // Czytamy stan wprost ze strony. Nie trzeba ani preloada, ani zmian
    // w aplikacji webowej, ani dodatkowego endpointu — `cur` i `aud` są
    // zwykłymi zmiennymi w kontekście strony.
    stan = await okno.webContents.executeJavaScript(`(function () {
      try {
        if (typeof cur === 'undefined' || !cur || typeof aud === 'undefined') return null;
        if (aud.paused) return null;
        return {
          title: cur.title || '',
          artist: cur.artist || '',
          pos: aud.currentTime || 0,
          dur: (aud.duration && isFinite(aud.duration)) ? aud.duration : 0
        };
      } catch (e) { return null; }
    })()`, true);
  } catch (e) {
    return;   // strona się przeładowuje albo jeszcze nie wstała
  }

  if (!stan || !stan.title) {
    if (ostatniPodpis) {
      ostatniPodpis = '';
      wyczyscAktywnosc().catch(() => {});
    }
    return;
  }

  // Wysyłamy tylko przy realnej zmianie. Pozycję zaokrąglamy do 15 s, żeby
  // samo płynięcie czasu nie generowało ruchu przy każdym odpytaniu —
  // Discord i tak sam animuje pasek postępu między aktualizacjami.
  const podpis = stan.title + '|' + stan.artist + '|' + Math.round(stan.pos / 15);
  if (podpis === ostatniPodpis) return;
  ostatniPodpis = podpis;

  const teraz = Date.now();
  const aktywnosc = {
    details: String(stan.title).slice(0, 128),
    state: String(stan.artist || 'SoundWave').slice(0, 128),
    largeImageKey: DISCORD_LOGO,
    largeImageText: 'SoundWave',
    instance: false,
  };
  if (stan.dur > 0) {
    // Podanie obu znaczników sprawia, że Discord rysuje pasek postępu utworu.
    aktywnosc.startTimestamp = new Date(teraz - stan.pos * 1000);
    aktywnosc.endTimestamp = new Date(teraz + (stan.dur - stan.pos) * 1000);
  }
  ustawAktywnosc(aktywnosc).catch((e) => console.warn('[discord] setActivity:', e.message));
}

// ── Okno ───────────────────────────────────────────────────────────────────
function utworzOkno() {
  okno = new BrowserWindow({
    width: 1000,
    height: 800,
    icon: path.join(__dirname, 'soundwave.png'),
    backgroundColor: '#0a0a0f',      // bez tego widać białe mignięcie przy starcie
    webPreferences: {
      // ZMIANA względem 1.0.0: było `nodeIntegration: true, webSecurity: false`.
      //
      // `webSecurity: false` wyłącza regułę tego samego pochodzenia dla całej
      // zawartości okna. Nie było potrzebne — API SoundWave odpowiada z
      // nagłówkiem CORS `*`, więc najpewniej został po jakimś debugowaniu.
      //
      // `nodeIntegration: true` przy ładowaniu ZDALNEJ strony to najgorsza
      // możliwa konfiguracja Electrona: gdyby domena kiedykolwiek trafiła
      // w niepowołane ręce, strona dostałaby dostęp do Node.js na każdym
      // komputerze z tą aplikacją. Tutaj i tak nie było do niczego potrzebne —
      // aplikacja nie ma preloada ani lokalnej zawartości.
      contextIsolation: true,
      nodeIntegration: false,
    },
  });
  Menu.setApplicationMenu(null);
  okno.loadURL(ADRES);

  okno.on('closed', () => { okno = null; });
}

app.on('second-instance', () => {
  if (okno) {
    if (okno.isMinimized()) okno.restore();
    okno.focus();
  }
});

app.whenReady().then(() => {
  utworzOkno();
  polaczDiscord();
  setInterval(odswiezObecnosc, ODPYTYWANIE_MS);
});

app.on('window-all-closed', () => {
  wyczyscAktywnosc().catch(() => {});
  if (rpc) { try { rpc.destroy(); } catch (e) {} }
  if (process.platform !== 'darwin') app.quit();
});

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0) utworzOkno();
});

}
