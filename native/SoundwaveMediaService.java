package pl.Soundwave.apka;

import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.net.Uri;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import android.webkit.WebView;

import androidx.annotation.Nullable;
import androidx.media3.common.AudioAttributes;
import androidx.media3.common.C;
import androidx.media3.common.MediaItem;
import androidx.media3.common.MediaMetadata;
import androidx.media3.common.Player;
import androidx.media3.common.util.UnstableApi;
import androidx.media3.exoplayer.ExoPlayer;
import androidx.media3.session.DefaultMediaNotificationProvider;
import androidx.media3.session.LibraryResult;
import androidx.media3.session.MediaLibraryService;
import androidx.media3.session.MediaSession;

import com.google.common.collect.ImmutableList;
import com.google.common.util.concurrent.Futures;
import com.google.common.util.concurrent.ListenableFuture;
import com.google.common.util.concurrent.SettableFuture;

import org.json.JSONArray;
import org.json.JSONObject;

import java.lang.ref.WeakReference;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * Natywny odtwarzacz dla Androida Auto.
 *
 * DLACZEGO W OGÓLE ISTNIEJE: Android Auto nie wyświetla WebView. Samochód
 * rysuje własny ekran przeglądania i wysyła komendy do serwisu, a serwis musi
 * SAM odtwarzać dźwięk. Auto potrafi go uruchomić, gdy Activity aplikacji w
 * ogóle nie istnieje (telefon zablokowany w kieszeni), więc most do odtwarzacza
 * w WebView byłby zawodny z definicji.
 *
 * Backend już serwuje strumienie z obsługą zakresów HTTP (206), więc ExoPlayer
 * odtwarza je bezpośrednio spod /api/stream/{id}?t={token}.
 *
 * Odtwarzacz w telefonie (WebView) zostaje bez zmian — te dwa światy nie
 * nachodzą na siebie, bo start odtwarzania tutaj pauzuje tamten.
 */
@UnstableApi
public class SoundwaveMediaService extends MediaLibraryService {

    private static final String TAG = "SoundwaveMedia";

    private static final String ROOT = "root";
    private static final String NODE_PLAYLISTS = "playlists";
    private static final String NODE_LIKED = "liked";
    private static final String PREFIX_PLAYLIST = "pl:";
    private static final String PREFIX_TRACK = "tr:";

    /** Uchwyt do WebView aplikacji — tylko po to, żeby ją wyciszyć, gdy gra auto. */
    private static WeakReference<WebView> webViewRef = new WeakReference<>(null);

    public static void attachWebView(WebView wv) {
        webViewRef = new WeakReference<>(wv);
    }

    private static void pauseWebPlayer() {
        final WebView wv = webViewRef.get();
        if (wv == null) return;
        try {
            wv.post(new Runnable() {
                @Override public void run() {
                    try {
                        // `aud` to const w zasięgu skryptu strony, a NIE właściwość
                        // window — wcześniejsze sprawdzenie `window.aud` zawsze
                        // dawało undefined, więc WebView nigdy nie był pauzowany.
                        wv.evaluateJavascript(
                            "(function(){try{if(typeof aud!=='undefined'&&aud&&!aud.paused){aud.pause();}}catch(e){}})()", null);
                    } catch (Exception ignored) {}
                }
            });
        } catch (Exception ignored) {}
    }

    // ── Zapamiętywanie sesji ───────────────────────────────────────────────
    // Wyłączenie silnika zabija serwis razem z kolejką. Stan trzymamy więc
    // w SharedPreferences: lista utworów, pozycja w niej i milisekunda utworu.
    private static final String PREFS_STATE = "soundwave_player";
    private static final int STATE_MAX_ITEMS = 300;
    private static final long SAVE_EVERY_MS = 10_000;
    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private final Runnable periodicSave = new Runnable() {
        @Override public void run() {
            if (player != null && player.isPlaying()) {
                saveState();
                mainHandler.postDelayed(this, SAVE_EVERY_MS);
            }
        }
    };

    // ── Sprzątanie karty multimediów (v116) ────────────────────────────────
    // Wstrzymany odtwarzacz nie trzyma serwisu w nieskończoność: po tym czasie
    // serwis się zamyka, a razem z nim znika karta multimediów w telefonie.
    // Gdy auto jest wciąż podłączone, serwis i tak żyje, dopóki auto go trzyma.
    private static final long IDLE_STOP_MS = 10 * 60_000;
    private final Runnable idleStop = new Runnable() {
        @Override public void run() {
            if (player == null || player.isPlaying()) return;
            saveState();
            pauseAllPlayersAndStopSelf();
        }
    };

    private static final String SYSTEM_UI = "com.android.systemui";

    /**
     * Zapytanie od panelu multimediów telefonu (System UI albo powiadomienie
     * media3, na które System UI jest mapowany) — a nie od auta czy słuchawek.
     */
    private static boolean fromSystemUi(MediaSession s, MediaSession.ControllerInfo c) {
        return s.isMediaNotificationController(c) || SYSTEM_UI.equals(c.getPackageName());
    }

    /**
     * media3 1.4.1 przy zwalnianiu sesji NIE usuwa powiadomienia
     * (MediaNotificationManager.removeSession tylko odpina kontroler), a proces
     * ubity przez system w ogóle nie dochodzi do onDestroy. Wstrzymane
     * odtwarzanie zostawiało więc w telefonie kartę z martwą sesją.
     */
    private void cancelMediaNotification() {
        try {
            NotificationManager nm = (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
            if (nm != null) nm.cancel(DefaultMediaNotificationProvider.DEFAULT_NOTIFICATION_ID);
        } catch (Exception ignored) {}
    }

    private ExoPlayer player;
    private MediaLibrarySession session;
    private SoundwaveApi api;
    private ExecutorService io;

    // Zapamiętane listy pozwalają odtworzyć CAŁĄ playlistę od klikniętego
    // utworu. Bez tego Auto zagrałoby jeden kawałek i ucichło.
    private final Map<String, List<MediaItem>> childrenCache = new ConcurrentHashMap<>();
    private final Map<String, String> trackParent = new ConcurrentHashMap<>();
    private final Map<String, MediaItem> itemById = new ConcurrentHashMap<>();

    @Override
    public void onCreate() {
        super.onCreate();
        // Pozostałość po poprzednim procesie — ta sesja już nie istnieje.
        cancelMediaNotification();
        api = new SoundwaveApi(this);
        io = Executors.newSingleThreadExecutor();

        player = new ExoPlayer.Builder(this)
                .setAudioAttributes(new AudioAttributes.Builder()
                        .setUsage(C.USAGE_MEDIA)
                        .setContentType(C.AUDIO_CONTENT_TYPE_MUSIC)
                        .build(), /* handleAudioFocus= */ true)
                .setHandleAudioBecomingNoisy(true)
                .build();

        player.addListener(new Player.Listener() {
            @Override public void onIsPlayingChanged(boolean isPlaying) {
                mainHandler.removeCallbacks(periodicSave);
                mainHandler.removeCallbacks(idleStop);
                if (isPlaying) {
                    pauseWebPlayer();
                    // Co 10 s — nagłe odcięcie zasilania w aucie nie daje
                    // serwisowi szansy na zapis przy zamykaniu.
                    mainHandler.postDelayed(periodicSave, SAVE_EVERY_MS);
                } else {
                    saveState();
                    mainHandler.postDelayed(idleStop, IDLE_STOP_MS);
                }
            }

            @Override public void onMediaItemTransition(@Nullable MediaItem mediaItem, int reason) {
                saveState();
            }

            @Override public void onPositionDiscontinuity(Player.PositionInfo oldPosition,
                                                          Player.PositionInfo newPosition, int reason) {
                if (reason == Player.DISCONTINUITY_REASON_SEEK) saveState();
            }
        });

        int flags = PendingIntent.FLAG_UPDATE_CURRENT;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) flags |= PendingIntent.FLAG_IMMUTABLE;
        PendingIntent openApp = PendingIntent.getActivity(
                this, 0, new Intent(this, MainActivity.class), flags);

        session = new MediaLibrarySession.Builder(this, player, new LibraryCallback())
                .setSessionActivity(openApp)
                .build();
    }

    @Nullable
    @Override
    public MediaLibrarySession onGetSession(MediaSession.ControllerInfo controllerInfo) {
        return session;
    }

    @Override
    public void onTaskRemoved(Intent rootIntent) {
        // Zamknięcie aplikacji z listy zadań nie może uciszyć muzyki w aucie,
        // ale gdy nic nie gra, nie ma po co trzymać serwisu.
        saveState();
        if (player != null && !player.getPlayWhenReady()) stopSelf();
        super.onTaskRemoved(rootIntent);
    }

    @Override
    public void onDestroy() {
        mainHandler.removeCallbacks(periodicSave);
        mainHandler.removeCallbacks(idleStop);
        saveState();
        if (session != null) { session.release(); session = null; }
        if (player != null) { player.release(); player = null; }
        cancelMediaNotification();
        if (io != null) io.shutdownNow();
        super.onDestroy();
    }

    // ── Budowanie pozycji ──────────────────────────────────────────────────
    private MediaItem folder(String id, String title) {
        MediaItem item = new MediaItem.Builder()
                .setMediaId(id)
                .setMediaMetadata(new MediaMetadata.Builder()
                        .setTitle(title)
                        .setIsBrowsable(true)
                        .setIsPlayable(false)
                        .setMediaType(MediaMetadata.MEDIA_TYPE_FOLDER_MIXED)
                        .build())
                .build();
        itemById.put(id, item);
        return item;
    }

    private MediaItem info(String id, String title) {
        return new MediaItem.Builder()
                .setMediaId(id)
                .setMediaMetadata(new MediaMetadata.Builder()
                        .setTitle(title)
                        .setIsBrowsable(false)
                        .setIsPlayable(false)
                        .build())
                .build();
    }

    private MediaItem track(SoundwaveApi.Track t) {
        MediaMetadata.Builder md = new MediaMetadata.Builder()
                .setTitle(t.title)
                .setArtist(t.artist)
                .setIsBrowsable(false)
                .setIsPlayable(true)
                .setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC);
        if (t.thumbnail != null && t.thumbnail.startsWith("http")) {
            md.setArtworkUri(Uri.parse(t.thumbnail));
        }
        MediaItem item = new MediaItem.Builder()
                .setMediaId(PREFIX_TRACK + t.id)
                .setMediaMetadata(md.build())
                .build();
        itemById.put(item.mediaId, item);
        return item;
    }

    /**
     * Dokłada adres strumienia. Pozycje wysyłane do przeglądarki celowo nie mają
     * URI — dopiero przy odtwarzaniu uzupełniamy je aktualnym tokenem.
     */
    private MediaItem withUri(MediaItem item) {
        String id = item.mediaId;
        if (id == null || !id.startsWith(PREFIX_TRACK)) return item;
        String trackId = id.substring(PREFIX_TRACK.length());
        return item.buildUpon().setUri(api.streamUrl(trackId)).build();
    }

    private ImmutableList<MediaItem> loadChildren(String parentId) {
        if (!api.isLoggedIn()) {
            return ImmutableList.of(info("brak-logowania",
                    "Zaloguj się w aplikacji SoundWave na telefonie"));
        }
        List<MediaItem> out = new ArrayList<>();
        if (ROOT.equals(parentId)) {
            out.add(folder(NODE_PLAYLISTS, "Playlisty"));
            out.add(folder(NODE_LIKED, "Polubione"));
        } else if (NODE_PLAYLISTS.equals(parentId)) {
            for (SoundwaveApi.Playlist p : api.playlists()) {
                out.add(folder(PREFIX_PLAYLIST + p.id, p.name));
            }
            if (out.isEmpty()) out.add(info("brak-playlist", "Brak playlist"));
        } else if (NODE_LIKED.equals(parentId)) {
            addTracks(out, api.liked(), parentId);
            if (out.isEmpty()) out.add(info("brak-polubionych", "Brak polubionych utworów"));
        } else if (parentId.startsWith(PREFIX_PLAYLIST)) {
            addTracks(out, api.playlistTracks(parentId.substring(PREFIX_PLAYLIST.length())), parentId);
            if (out.isEmpty()) out.add(info("pusta-playlista", "Playlista jest pusta"));
        }
        ImmutableList<MediaItem> list = ImmutableList.copyOf(out);
        childrenCache.put(parentId, list);
        return list;
    }

    private void addTracks(List<MediaItem> out, List<SoundwaveApi.Track> tracks, String parentId) {
        for (SoundwaveApi.Track t : tracks) {
            MediaItem item = track(t);
            trackParent.put(item.mediaId, parentId);
            out.add(item);
        }
    }

    // ── Zapis i odczyt sesji ───────────────────────────────────────────────
    private SharedPreferences stateStore() {
        return getApplicationContext().getSharedPreferences(PREFS_STATE, Context.MODE_PRIVATE);
    }

    private void saveState() {
        if (player == null) return;
        int n = player.getMediaItemCount();
        if (n == 0) return;                     // pusty odtwarzacz nie kasuje zapisanej sesji
        try {
            int cur = player.getCurrentMediaItemIndex();
            // Okno wokół bieżącego utworu — przy ogromnej playliście nie zapisujemy wszystkiego
            int start = Math.max(0, Math.min(cur - 50, n - STATE_MAX_ITEMS));
            int end = Math.min(n, start + STATE_MAX_ITEMS);
            JSONArray items = new JSONArray();
            int outIdx = -1;
            for (int i = start; i < end; i++) {
                MediaItem m = player.getMediaItemAt(i);
                if (m.mediaId == null || !m.mediaId.startsWith(PREFIX_TRACK)) continue;
                MediaMetadata md = m.mediaMetadata;
                JSONObject o = new JSONObject();
                o.put("id", m.mediaId.substring(PREFIX_TRACK.length()));
                o.put("title", md.title != null ? md.title.toString() : "");
                o.put("artist", md.artist != null ? md.artist.toString() : "");
                o.put("thumbnail", md.artworkUri != null ? md.artworkUri.toString() : "");
                if (i == cur) outIdx = items.length();
                items.put(o);
            }
            if (outIdx < 0) return;
            long pos = player.getPlaybackState() == Player.STATE_ENDED
                    ? 0 : Math.max(0, player.getCurrentPosition());
            JSONObject st = new JSONObject();
            st.put("savedAt", System.currentTimeMillis());
            st.put("index", outIdx);
            st.put("positionMs", pos);
            st.put("items", items);
            stateStore().edit().putString("state", st.toString()).apply();
        } catch (Exception e) {
            Log.w(TAG, "saveState: " + e);
        }
    }

    private static final class ResumeState {
        final List<MediaItem> items;
        final int index;
        final long positionMs;
        final long savedAt;
        ResumeState(List<MediaItem> items, int index, long positionMs, long savedAt) {
            this.items = items; this.index = index; this.positionMs = positionMs; this.savedAt = savedAt;
        }
    }

    /**
     * Sesja do wznowienia: nowsza z dwóch — ostatnio grane w aucie (natywnie)
     * albo ostatnio grane w aplikacji na telefonie (localStorage, przekazywane
     * przez SoundwaveAuth przy wejściu/wyjściu z aplikacji).
     */
    @Nullable
    private ResumeState loadResumeState() {
        if (!api.isLoggedIn()) return null;
        ResumeState nat = null, web = null;
        try { nat = parseNativeState(stateStore().getString("state", "")); }
        catch (Exception e) { Log.w(TAG, "stan natywny: " + e); }
        try { web = parseWebSession(SoundwaveAuth.webSession(this)); }
        catch (Exception e) { Log.w(TAG, "sesja z telefonu: " + e); }
        if (nat == null) return web;
        if (web == null) return nat;
        return web.savedAt > nat.savedAt ? web : nat;
    }

    private MediaItem resumeItem(JSONObject o) {
        String id = o.optString("id", "");
        if (id.isEmpty()) id = o.optString("track_id", "");
        if (id.isEmpty()) return null;
        return withUri(track(new SoundwaveApi.Track(id,
                o.optString("title", "Nieznany"),
                o.optString("artist", ""),
                o.optString("thumbnail", ""))));
    }

    @Nullable
    private ResumeState parseNativeState(String json) throws Exception {
        if (json == null || json.isEmpty()) return null;
        JSONObject st = new JSONObject(json);
        JSONArray arr = st.optJSONArray("items");
        if (arr == null || arr.length() == 0) return null;
        List<MediaItem> items = new ArrayList<>();
        int savedIdx = st.optInt("index", 0), idx = 0;
        for (int i = 0; i < arr.length(); i++) {
            MediaItem m = resumeItem(arr.getJSONObject(i));
            if (m == null) continue;
            if (i == savedIdx) idx = items.size();
            items.add(m);
        }
        if (items.isEmpty()) return null;
        return new ResumeState(items, idx, Math.max(0, st.optLong("positionMs", 0)),
                st.optLong("savedAt", 0));
    }

    /** Format z index.html: {cur, queue, qIdx, currentTime (s), savedAt}. */
    @Nullable
    private ResumeState parseWebSession(String json) throws Exception {
        if (json == null || json.isEmpty()) return null;
        JSONObject s = new JSONObject(json);
        JSONObject cur = s.optJSONObject("cur");
        if (cur == null) return null;
        String curId = cur.optString("id", cur.optString("track_id", ""));
        if (curId.isEmpty()) return null;
        JSONArray q = s.optJSONArray("queue");
        List<MediaItem> items = new ArrayList<>();
        int idx = -1;
        int qIdx = s.optInt("qIdx", -1);
        if (q != null) {
            for (int i = 0; i < q.length(); i++) {
                JSONObject o = q.optJSONObject(i);
                if (o == null) continue;
                MediaItem m = resumeItem(o);
                if (m == null) continue;
                String id = m.mediaId.substring(PREFIX_TRACK.length());
                if (id.equals(curId) && (idx < 0 || i == qIdx)) idx = items.size();
                items.add(m);
            }
        }
        if (idx < 0) {                         // kolejka nie zawiera utworu — gramy sam utwór
            MediaItem m = resumeItem(cur);
            if (m == null) return null;
            items.clear();
            items.add(m);
            idx = 0;
        }
        long posMs = Math.max(0, Math.round(s.optDouble("currentTime", 0) * 1000));
        return new ResumeState(items, idx, posMs, s.optLong("savedAt", 0));
    }

    // ── Callback biblioteki ────────────────────────────────────────────────
    private final class LibraryCallback implements MediaLibrarySession.Callback {

        /**
         * Po podłączeniu auta ładujemy ostatnią sesję do odtwarzacza — BEZ
         * prepare(), więc nic się nie buforuje ani nie gra samo. Auto od razu
         * pokazuje ostatni utwór, a play wznawia go od zapamiętanej sekundy
         * (MediaSession sam wywołuje prepare() przy play na bezczynnym odtwarzaczu).
         */
        @Override
        public void onPostConnect(MediaSession s, MediaSession.ControllerInfo controller) {
            // Panel multimediów telefonu łączy się sam (np. po starcie systemu) —
            // dla niego nie wczytujemy sesji, inaczej każde takie połączenie
            // budziło serwis z utworem gotowym do pokazania jako kolejna karta.
            if (fromSystemUi(s, controller)) return;
            if (player == null || player.getMediaItemCount() > 0) return;
            ResumeState st = loadResumeState();
            if (st == null) return;
            player.setMediaItems(st.items, st.index, st.positionMs);
            Log.d(TAG, "Wczytano ostatnią sesję: " + st.items.size() + " utw., pozycja "
                    + st.index + " @ " + st.positionMs + " ms");
        }

        /**
         * Play przy pustym odtwarzaczu (przycisk w aucie, na słuchawkach,
         * systemowe wznawianie) — media3 pyta tutaj, co zagrać.
         */
        @Override
        public ListenableFuture<MediaSession.MediaItemsWithStartPosition> onPlaybackResumption(
                MediaSession ms, MediaSession.ControllerInfo controller) {
            // Wtyczka MediaSession z WebView dokłada do manifestu MediaButtonReceiver,
            // więc media3 ogłasza systemowi wznawianie odtwarzania. Od v115 ta metoda
            // zwracała sesję także System UI, który na tej podstawie tworzył w
            // telefonie karty "do wznowienia". Wznawiamy tylko dla auta i słuchawek.
            if (fromSystemUi(ms, controller)) {
                return Futures.immediateFailedFuture(new UnsupportedOperationException("wznawianie tylko w aucie"));
            }
            ResumeState st = loadResumeState();
            if (st == null) {
                return Futures.immediateFailedFuture(new UnsupportedOperationException("brak zapisanej sesji"));
            }
            return Futures.immediateFuture(new MediaSession.MediaItemsWithStartPosition(
                    st.items, st.index, st.positionMs));
        }

        @Override
        public ListenableFuture<LibraryResult<MediaItem>> onGetLibraryRoot(
                MediaLibrarySession s, MediaSession.ControllerInfo browser,
                @Nullable LibraryParams params) {
            return Futures.immediateFuture(LibraryResult.ofItem(folder(ROOT, "SoundWave"), params));
        }

        @Override
        public ListenableFuture<LibraryResult<MediaItem>> onGetItem(
                MediaLibrarySession s, MediaSession.ControllerInfo browser, String mediaId) {
            MediaItem item = itemById.get(mediaId);
            if (item != null) return Futures.immediateFuture(LibraryResult.ofItem(item, null));
            return Futures.immediateFuture(LibraryResult.<MediaItem>ofError(
                    LibraryResult.RESULT_ERROR_BAD_VALUE));
        }

        @Override
        public ListenableFuture<LibraryResult<ImmutableList<MediaItem>>> onGetChildren(
                MediaLibrarySession s, MediaSession.ControllerInfo browser, String parentId,
                int page, int pageSize, @Nullable LibraryParams params) {
            final SettableFuture<LibraryResult<ImmutableList<MediaItem>>> f = SettableFuture.create();
            final LibraryParams p = params;
            // Zapytania sieciowe nie mogą blokować wątku głównego — Auto ma
            // krótki limit na odpowiedź i ubiłoby połączenie.
            io.execute(new Runnable() {
                @Override public void run() {
                    try {
                        f.set(LibraryResult.ofItemList(loadChildren(parentId), p));
                    } catch (Throwable t) {
                        Log.w(TAG, "onGetChildren(" + parentId + "): " + t);
                        f.set(LibraryResult.<ImmutableList<MediaItem>>ofError(
                                LibraryResult.RESULT_ERROR_UNKNOWN));
                    }
                }
            });
            return f;
        }

        @Override
        public ListenableFuture<List<MediaItem>> onAddMediaItems(
                MediaSession ms, MediaSession.ControllerInfo controller, List<MediaItem> mediaItems) {
            List<MediaItem> out = new ArrayList<>();
            for (MediaItem it : mediaItems) out.add(withUri(it));
            return Futures.immediateFuture(out);
        }

        @Override
        public ListenableFuture<MediaSession.MediaItemsWithStartPosition> onSetMediaItems(
                MediaSession ms, MediaSession.ControllerInfo controller,
                List<MediaItem> mediaItems, int startIndex, long startPositionMs) {
            // Kliknięcie utworu w aucie ma zagrać całą listę od tego miejsca,
            // a nie jeden kawałek i ciszę.
            if (mediaItems.size() == 1) {
                String id = mediaItems.get(0).mediaId;
                String parent = id != null ? trackParent.get(id) : null;
                List<MediaItem> siblings = parent != null ? childrenCache.get(parent) : null;
                if (siblings != null) {
                    List<MediaItem> playable = new ArrayList<>();
                    int idx = -1;
                    for (MediaItem m : siblings) {
                        if (m.mediaId != null && m.mediaId.startsWith(PREFIX_TRACK)) {
                            if (m.mediaId.equals(id)) idx = playable.size();
                            playable.add(withUri(m));
                        }
                    }
                    if (idx >= 0) {
                        return Futures.immediateFuture(new MediaSession.MediaItemsWithStartPosition(
                                playable, idx, startPositionMs));
                    }
                }
            }
            List<MediaItem> out = new ArrayList<>();
            for (MediaItem m : mediaItems) out.add(withUri(m));
            return Futures.immediateFuture(new MediaSession.MediaItemsWithStartPosition(
                    out, startIndex, startPositionMs));
        }
    }
}
