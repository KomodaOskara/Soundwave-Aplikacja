# Wdrożenie v112 w repo Soundwave-Aplikacja

## 1. Pliki

Skopiuj zawartość tego katalogu do repo, zachowując ścieżki:

```
.github/workflows/android.yml            ← podmień
.github/workflows/build-all-desktop.yml  ← podmień
desktop/main.js                          ← nowy
desktop/package.json                     ← nowy
native/SoundwaveMediaService.java        ← podmień (poprawka pauzy w Android Auto)
native/SoundwaveAuth.java, SoundwaveApi.java, automotive_app_desc.xml  ← bez zmian, dla kompletu
www/offline.html                         ← nowy (strona „Brak połączenia”)
```

`.github/workflows/build.yml` (iOS) i `capacitor.config.json` zostają bez zmian.
Plik `WDROZENIE.md` do repo nie jest potrzebny.

`www/index.html` i `www/sw.js` w repo przestają mieć znaczenie dla Androida
(aplikacja ładuje stronę z serwera), ale nadal używa ich build iOS.

## 2. Sekret: stały klucz podpisu APK

Bez tego każdy build ma inny klucz i **nie zainstaluje się jako aktualizacja**.

1. Otwórz plik `soundwave-debug.keystore.b64` (dostałeś go osobno — nie ma go
   w tej paczce i nie powinien trafić do repo ani na serwer WWW).
2. GitHub → repo → **Settings → Secrets and variables → Actions →
   zakładka Secrets → New repository secret**.
3. Nazwa: `ANDROID_DEBUG_KEYSTORE`, wartość: cała zawartość pliku `.b64`.

Zachowaj też `soundwave-debug.keystore` w bezpiecznym miejscu. Jeśli klucz
zginie, kolejne APK znowu będą wymagały odinstalowania starego.

## 3. Zmienna: Discord Rich Presence (opcjonalnie)

Ta sama strona, zakładka **Variables → New repository variable**:
nazwa `DISCORD_APP_ID`, wartość — Application ID z
<https://discord.com/developers/applications>. Szczegóły w `desktop/README.md`
z paczki serwerowej. Bez tej zmiennej desktop buduje się normalnie, tylko bez
statusu na Discordzie.

## 4. Pierwsze uruchomienie

Wrzucenie plików odpali oba workflowy automatycznie (zmieniły się pliki
workflowów). Gotowe pliki pojawią się w zakładce **Releases** repo.

**Android — jednorazowo:** odinstaluj starą aplikację, zainstaluj nowy APK,
zaloguj się ponownie. Utwory zapisane offline w starej aplikacji nie
przechodzą. Od tej chwili zmiany na serwerze widać w aplikacji od razu,
a o nowym APK aplikacja powie sama.
