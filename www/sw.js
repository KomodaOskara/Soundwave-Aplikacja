// Wersja cache — MUSI się zmieniać przy każdym wydaniu.
// Zmiana tej stałej zmienia bajty sw.js, dzięki czemu przeglądarka wykrywa
// nowego service workera, instaluje go i w activate kasuje stare cache.
// W v101 stała stała w miejscu, a dokument HTML szedł cache-first — po
// podmianie index.html na serwerze przeglądarki nadal serwowały starą
// aplikację, dopóki ktoś ręcznie nie wyczyścił danych strony.
const CACHE_NAME = 'soundwave-ui-v104-01';

// Tylko rzeczy, które faktycznie się nie zmieniają. Dokument HTML celowo NIE
// jest tu prekeszowany — obsługuje go strategia network-first niżej.
const UI_ASSETS = ['/manifest.json', '/icon.png'];

// Install — pobierz statyczne assety i od razu przejmij kontrolę
self.addEventListener('install', e => {
  e.waitUntil(
    caches.open(CACHE_NAME)
      .then(c => c.addAll(UI_ASSETS))
      .catch(() => {})               // brak assetu nie może zablokować instalacji
      .then(() => self.skipWaiting())
  );
});

// Activate — skasuj wszystkie cache innej wersji
self.addEventListener('activate', e => {
  e.waitUntil(
    caches.keys()
      .then(keys => Promise.all(keys.filter(k => k !== CACHE_NAME).map(k => caches.delete(k))))
      .then(() => self.clients.claim())
  );
});

self.addEventListener('fetch', e => {
  const req = e.request;
  if (req.method !== 'GET') return;

  let url;
  try { url = new URL(req.url); } catch (_) { return; }

  // Obce hosty (miniatury YouTube, lrclib, QR) — nie dotykamy
  if (url.origin !== self.location.origin) return;
  // API i websockety zawsze prosto do sieci
  if (url.pathname.startsWith('/api/') || url.pathname.startsWith('/ws/')) return;

  const isDoc = req.mode === 'navigate'
             || url.pathname === '/'
             || url.pathname.endsWith('.html');

  if (isDoc) {
    // NETWORK-FIRST dla dokumentu: świeża wersja aplikacji zaraz po deployu,
    // a cache zostaje wyłącznie jako zapas na tryb offline.
    e.respondWith(
      fetch(req)
        .then(resp => {
          if (resp && resp.ok) {
            const clone = resp.clone();
            caches.open(CACHE_NAME).then(c => c.put(req, clone));
          }
          return resp;
        })
        .catch(() => caches.match(req).then(c => c || caches.match('/')))
    );
    return;
  }

  // Reszta (ikona, manifest) — cache-first, te pliki się nie zmieniają
  e.respondWith(
    caches.match(req).then(cached => {
      if (cached) return cached;
      return fetch(req).then(resp => {
        if (resp && resp.ok) {
          const clone = resp.clone();
          caches.open(CACHE_NAME).then(c => c.put(req, clone));
        }
        return resp;
      }).catch(() => cached);
    })
  );
});

// Message handler — ręczne wymuszenie aktywacji z aplikacji
self.addEventListener('message', e => {
  if (e.data === 'skipWaiting') self.skipWaiting();
});
