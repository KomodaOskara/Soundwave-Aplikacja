# SoundWave — aplikacja desktopowa (Electron)

Podmienia `main.js` i `package.json` w projekcie, z którego budujecie RPM-a.
Reszta (ikona, konfiguracja electron-buildera, workflow) zostaje bez zmian.

Przeanalizowana wersja: **soundwave-1.0.0.x86_64.rpm**, Electron 29.4.6,
`app.asar` zawierał `main.js` (422 B), `package.json` i ikonę.

---

## 1. Discord Rich Presence

### Dlaczego dopiero teraz to proste

Rich Presence wymaga połączenia z **lokalnym gniazdem IPC** klienta Discorda
(named pipe na Windowsie, socket uniksowy na Linuksie). Przeglądarka nie ma do
niego dostępu i mieć nie będzie — to celowa granica piaskownicy, nie brak API.
Dlatego z poziomu aplikacji webowej było to niewykonalne bez osobnego programu.

Proces główny Electrona to zwykły Node.js, więc dostęp ma. Skoro macie już
aplikację desktopową, cała „osobna aplikacja pomocnicza" okazuje się zbędna.

### Jak to działa

Proces główny co 8 sekund odpytuje stronę przez `executeJavaScript` i czyta
`cur` oraz `aud` — te same zmienne, których używa interfejs. **Nie trzeba
zmieniać niczego w aplikacji webowej ani dokładać endpointu w API.**

Status wysyłamy tylko przy realnej zmianie; pozycja w utworze jest zaokrąglana
do 15 sekund, żeby samo płynięcie czasu nie generowało ruchu — Discord i tak
sam animuje pasek postępu między aktualizacjami. Pauza kasuje status.

### Konfiguracja

1. Wejdź na <https://discord.com/developers/applications> → **New Application**,
   nazwij ją `SoundWave` i skopiuj **Application ID**.
2. W tej samej aplikacji: **Rich Presence → Art Assets** → wgraj logo pod nazwą
   `soundwave` (to wartość stałej `DISCORD_LOGO`).
3. W `main.js` wklej identyfikator:
   ```js
   const DISCORD_APP_ID = process.env.SOUNDWAVE_DISCORD_APP_ID || 'TU_WKLEJ_ID';
   ```
4. Zbuduj paczkę jak dotąd.

Bez identyfikatora integracja po prostu się nie włącza, a aplikacja działa
dokładnie jak wcześniej. Wyłączony albo niezainstalowany Discord też niczego
nie psuje — połączenie po cichu nie dochodzi do skutku.

### Czego nie sprawdziłem

Samego połączenia z Discordem — wymaga działającego klienta i Application ID,
których tu nie mam. Zweryfikowałem składnię, mechanizm odczytu stanu ze strony
(podczas grania zwraca tytuł, wykonawcę i pozycję; po pauzie `null`) oraz to,
jaka aktywność zostałaby z tego złożona.

Dwie rzeczy do sprawdzenia u siebie:

- **Status najpewniej pokaże się jako „Gra w SoundWave"**, a nie „Słucha".
  Typ „Listening" Discord traktuje specjalnie i dla zewnętrznych aplikacji
  bywa niedostępny.
- **Pasek postępu** — jeśli się nie pojawi, zamień w `main.js` obiekty `Date`
  na liczby (`teraz - stan.pos * 1000`). Biblioteki różnią się tym, czy
  oczekują sekund, milisekund czy `Date`.

Okładki poszczególnych utworów celowo pominąłem: Rich Presence bierze obrazki
z zasobów wgranych w portalu Discorda, a dowolny adres wymaga nieudokumentowanego
obejścia, które potrafi przestać działać z dnia na dzień. Stałe logo jest nudne,
ale nie zepsuje się samo.

---

## 2. Poprawki konfiguracji okna

Wersja 1.0.0 miała:

```js
webPreferences: { nodeIntegration: true, webSecurity: false }
```

Obie te opcje są usunięte, i warto wiedzieć dlaczego.

**`webSecurity: false`** wyłącza regułę tego samego pochodzenia dla całej
zawartości okna. Nie było potrzebne — API SoundWave odpowiada z nagłówkiem
CORS `*`, a wszystkie zasoby idą z tej samej domeny. Najpewniej został po
jakimś debugowaniu.

**`nodeIntegration: true` przy ładowaniu ZDALNEJ strony** to najgorsza możliwa
konfiguracja Electrona. Gdyby domena kiedykolwiek trafiła w niepowołane ręce
(wygaśnięcie, przejęcie DNS), strona dostałaby dostęp do Node.js na każdym
komputerze z zainstalowaną aplikacją. W praktyce ryzyko było mniejsze, bo
`contextIsolation` w Electronie 29 jest domyślnie włączone i odcina Node od
głównego świata strony — ale ta opcja i tak nie była do niczego potrzebna,
bo aplikacja nie ma preloada ani lokalnej zawartości.

Gdyby po tej zmianie cokolwiek przestało działać, to są dwie linijki do
cofnięcia — ale nie powinno.

**Osobno warta rozważenia:** Electron 29 wyszedł w 2024 i jest już po końcu
wsparcia, czyli nie dostaje łatek bezpieczeństwa Chromium. Przy aplikacji
renderującej zdalną stronę warto podbić wersję przy najbliższej okazji.

---

## 3. Dodatkowo

- **Blokada drugiej instancji** — dwie kopie oznaczałyby dwóch klientów RPC
  bijących się o ten sam status i dwa okna grające naraz. Uruchomienie drugiej
  kopii przywraca istniejące okno.
- **Kolor tła okna** ustawiony na `#0a0a0f`, żeby przy starcie nie mignęło
  białe tło przed załadowaniem strony.

---

## 4. Build

Doszła jedna zależność:

```json
"dependencies": { "@xhayper/discord-rpc": "^1.3.4" }
```

To czysty JavaScript, bez modułów natywnych — nie ma potrzeby `electron-rebuild`
ani niczego dodatkowego w workflow. Wymaga tylko, żeby `npm install` wykonał się
przed pakowaniem (electron-builder sam wciąga `dependencies` do `app.asar`).

Biblioteka `discord-rpc` bez prefiksu ma ostatnie wydanie z 2021 roku;
`@xhayper/discord-rpc` jest utrzymywany (1.3.4 z kwietnia 2026).
