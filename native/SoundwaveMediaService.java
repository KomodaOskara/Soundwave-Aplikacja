package pl.Soundwave.apka;

import android.app.PendingIntent;
import android.content.Intent;
import android.net.Uri;
import android.os.Build;
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
import androidx.media3.session.LibraryResult;
import androidx.media3.session.MediaLibraryService;
import androidx.media3.session.MediaSession;

import com.google.common.collect.ImmutableList;
import com.google.common.util.concurrent.Futures;
import com.google.common.util.concurrent.ListenableFuture;
import com.google.common.util.concurrent.SettableFuture;

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
                        wv.evaluateJavascript(
                            "(function(){try{if(window.aud&&!aud.paused){aud.pause();}}catch(e){}})()", null);
                    } catch (Exception ignored) {}
                }
            });
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
                if (isPlaying) pauseWebPlayer();
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
        if (player != null && !player.getPlayWhenReady()) stopSelf();
        super.onTaskRemoved(rootIntent);
    }

    @Override
    public void onDestroy() {
        if (session != null) { session.release(); session = null; }
        if (player != null) { player.release(); player = null; }
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

    // ── Callback biblioteki ────────────────────────────────────────────────
    private final class LibraryCallback implements MediaLibrarySession.Callback {

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
