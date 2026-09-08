package pl.Soundwave.apka;

import android.content.Context;
import android.content.SharedPreferences;
import android.util.Log;
import android.webkit.ValueCallback;
import android.webkit.WebView;

import org.json.JSONObject;
import org.json.JSONTokener;

/**
 * Przechowuje adres API i token sesji, żeby natywny odtwarzacz (Android Auto)
 * mógł korzystać z tego samego konta co aplikacja w telefonie.
 *
 * Token NIE jest przekazywany przez JavascriptInterface — MainActivity odczytuje
 * go z localStorage przez evaluateJavascript i zapisuje tutaj. Ta droga jest
 * jednokierunkowa i nie zależy od mostu Capacitora, który potrafił się nie
 * zarejestrować (patrz historia MediaSession w v101).
 */
public final class SoundwaveAuth {

    private static final String TAG = "SoundwaveAuth";
    private static final String PREFS = "soundwave_auth";
    private static final String DEFAULT_API = "https://muzyka.komodaoskara.pl";

    private SoundwaveAuth() {}

    private static SharedPreferences prefs(Context ctx) {
        return ctx.getApplicationContext().getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    public static String api(Context ctx) {
        String v = prefs(ctx).getString("api", "");
        return (v == null || v.isEmpty()) ? DEFAULT_API : v;
    }

    public static String token(Context ctx) {
        String v = prefs(ctx).getString("token", "");
        return v == null ? "" : v;
    }

    public static boolean hasToken(Context ctx) {
        return !token(ctx).isEmpty();
    }

    public static void save(Context ctx, String api, String token) {
        if (token == null) token = "";
        SharedPreferences.Editor e = prefs(ctx).edit();
        if (api != null && !api.isEmpty()) e.putString("api", api);
        e.putString("token", token);
        e.apply();
    }

    /**
     * Odczytuje adres API i token z localStorage strony i zapisuje je lokalnie.
     *
     * Ten kawałek JS celowo mieszka w pliku .java, a nie w łatce tekstowej
     * w workflow — przy trzech poziomach cudzysłowów (YAML → Python → Java → JS)
     * każda zmiana byłaby zgadywanką.
     */
    public static void syncFromWebView(final Context ctx, WebView webView) {
        if (webView == null) return;
        final String js =
            "(function(){try{return JSON.stringify({"
          + "api:(typeof API!=='undefined'?API:''),"
          + "token:(localStorage.getItem('sw_token')||'')"
          + "});}catch(e){return '{}';}})()";
        try {
            webView.evaluateJavascript(js, new ValueCallback<String>() {
                @Override public void onReceiveValue(String value) {
                    saveFromJs(ctx, value);
                }
            });
        } catch (Exception e) {
            Log.w(TAG, "syncFromWebView: " + e);
        }
    }

    /**
     * Zapisuje dane zwrócone przez evaluateJavascript.
     *
     * evaluateJavascript oddaje wynik zakodowany jako JSON, więc string z JS
     * przychodzi jako string JSON-owy ZAWIERAJĄCY JSON — stąd podwójne
     * parsowanie. Bez tego dostalibyśmy dosłowne cudzysłowy w tokenie.
     */
    public static void saveFromJs(Context ctx, String jsResult) {
        if (jsResult == null || jsResult.isEmpty() || "null".equals(jsResult)) return;
        try {
            Object first = new JSONTokener(jsResult).nextValue();
            String inner = (first instanceof String) ? (String) first : jsResult;
            JSONObject o = new JSONObject(inner);
            String token = o.optString("token", "");
            if (token.isEmpty()) return;                    // niezalogowany — nie kasujemy starego
            save(ctx, o.optString("api", ""), token);
            Log.d(TAG, "Dane logowania przekazane do odtwarzacza natywnego");
        } catch (Exception e) {
            Log.w(TAG, "Nie udało się odczytać danych logowania: " + e);
        }
    }
}
