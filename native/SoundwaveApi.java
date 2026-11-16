package pl.Soundwave.apka;

import android.content.Context;
import android.util.Log;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.net.URLEncoder;
import java.util.ArrayList;
import java.util.List;

/**
 * Minimalny klient REST API SoundWave na potrzeby Androida Auto.
 *
 * Świadomie bez zewnętrznych bibliotek — HttpURLConnection wystarcza, a każda
 * dodatkowa zależność to kolejne ryzyko przy generowanym od zera projekcie
 * Capacitora.
 */
public class SoundwaveApi {

    private static final String TAG = "SoundwaveApi";

    public static class Playlist {
        public final String id, name;
        public final int trackCount;
        Playlist(String id, String name, int trackCount) {
            this.id = id; this.name = name; this.trackCount = trackCount;
        }
    }

    public static class Track {
        public final String id, title, artist, thumbnail;
        Track(String id, String title, String artist, String thumbnail) {
            this.id = id; this.title = title; this.artist = artist; this.thumbnail = thumbnail;
        }
    }

    /** Osobisty miks (v123 na serwerze) — nazwa, opis i gotowa lista utworów. */
    public static class Mix {
        public final String id, name, desc;
        public final List<Track> tracks;
        Mix(String id, String name, String desc, List<Track> tracks) {
            this.id = id; this.name = name; this.desc = desc; this.tracks = tracks;
        }
    }

    private final Context ctx;

    public SoundwaveApi(Context ctx) { this.ctx = ctx.getApplicationContext(); }

    public boolean isLoggedIn() { return SoundwaveAuth.hasToken(ctx); }

    /** Adres strumienia. Token idzie parametrem — ExoPlayer nie ustawia nagłówków sam. */
    public String streamUrl(String trackId) {
        String path = trackId.startsWith("local_") ? "/api/stream/local/" : "/api/stream/";
        return SoundwaveAuth.api(ctx) + path + enc(trackId)
                + "?t=" + enc(SoundwaveAuth.token(ctx));
    }

    private static String enc(String s) {
        try { return URLEncoder.encode(s == null ? "" : s, "UTF-8"); }
        catch (Exception e) { return ""; }
    }

    private String get(String path) throws Exception {
        HttpURLConnection c = null;
        try {
            URL url = new URL(SoundwaveAuth.api(ctx) + path);
            c = (HttpURLConnection) url.openConnection();
            c.setRequestMethod("GET");
            c.setRequestProperty("Authorization", "Bearer " + SoundwaveAuth.token(ctx));
            c.setConnectTimeout(8000);
            c.setReadTimeout(12000);
            int code = c.getResponseCode();
            if (code != 200) throw new Exception("HTTP " + code + " dla " + path);
            InputStream in = c.getInputStream();
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] buf = new byte[8192];
            int n;
            while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
            return out.toString("UTF-8");
        } finally {
            if (c != null) c.disconnect();
        }
    }

    public List<Playlist> playlists() {
        List<Playlist> out = new ArrayList<>();
        try {
            JSONArray arr = new JSONArray(get("/api/playlists"));
            for (int i = 0; i < arr.length(); i++) {
                JSONObject o = arr.getJSONObject(i);
                out.add(new Playlist(
                        String.valueOf(o.optInt("id")),
                        o.optString("name", "Playlista"),
                        o.optInt("track_count", 0)));
            }
        } catch (Exception e) {
            Log.w(TAG, "playlists(): " + e);
        }
        return out;
    }

    public List<Track> playlistTracks(String playlistId) {
        return tracks("/api/playlists/" + enc(playlistId) + "/tracks");
    }

    public List<Track> liked() {
        return tracks("/api/liked");
    }

    private List<Track> tracks(String path) {
        try {
            return parseTracks(new JSONArray(get(path)));
        } catch (Exception e) {
            Log.w(TAG, "tracks(" + path + "): " + e);
            return new ArrayList<>();
        }
    }

    /** Odpowiedzi typu {"tracks": [...]} — radio, propozycje, wyszukiwarka. */
    private List<Track> tracksField(String path) {
        try {
            JSONArray arr = new JSONObject(get(path)).optJSONArray("tracks");
            return arr != null ? parseTracks(arr) : new ArrayList<Track>();
        } catch (Exception e) {
            Log.w(TAG, "tracksField(" + path + "): " + e);
            return new ArrayList<>();
        }
    }

    static List<Track> parseTracks(JSONArray arr) {
        List<Track> out = new ArrayList<>();
        for (int i = 0; i < arr.length(); i++) {
            JSONObject o = arr.optJSONObject(i);
            if (o == null) continue;
            // Playlisty zwracają track_id, polubione mają dodatkowo id —
            // bierzemy to, co jest.
            String id = o.optString("track_id", o.optString("id", ""));
            if (id.isEmpty()) continue;
            out.add(new Track(id,
                    o.optString("title", "Nieznany"),
                    o.optString("artist", ""),
                    o.optString("thumbnail", "")));
        }
        return out;
    }

    /**
     * Kolejka radiowa od utworu (ten sam endpoint co w aplikacji). anchor = utwór,
     * od którego radio wystartowało — bez niego każde dociągnięcie szukałoby
     * „podobnego do podobnego” i radio odpływało.
     */
    public List<Track> radio(Track seed, @androidx.annotation.Nullable Track anchor) {
        String q = "/api/radio?title=" + enc(seed.title) + "&artist=" + enc(seed.artist)
                + "&seed_id=" + enc(seed.id) + "&algo=mix";
        if (anchor != null && !anchor.id.equals(seed.id)) {
            q += "&anchor_id=" + enc(anchor.id) + "&anchor_title=" + enc(anchor.title)
                    + "&anchor_artist=" + enc(anchor.artist);
        }
        return tracksField(q);
    }

    /** „Polecane dla Ciebie” ze strony głównej. */
    public List<Track> recommend(int limit) {
        return tracksField("/api/recommend?limit=" + limit);
    }

    public List<Track> search(String query) {
        return tracksField("/api/search?q=" + enc(query));
    }

    public List<Mix> mixes() {
        List<Mix> out = new ArrayList<>();
        try {
            JSONArray arr = new JSONObject(get("/api/mixes")).optJSONArray("mixes");
            if (arr == null) return out;
            for (int i = 0; i < arr.length(); i++) {
                JSONObject o = arr.getJSONObject(i);
                JSONArray t = o.optJSONArray("tracks");
                out.add(new Mix(String.valueOf(o.optInt("idx", i)),
                        o.optString("name", "Miks"), o.optString("desc", ""),
                        t != null ? parseTracks(t) : new ArrayList<Track>()));
            }
        } catch (Exception e) {
            Log.w(TAG, "mixes(): " + e);
        }
        return out;
    }
}
