package com.psharehub.pip;

import android.webkit.WebView;

import androidx.webkit.WebViewCompat;
import androidx.webkit.WebViewFeature;

import java.util.Arrays;
import java.util.Collections;
import java.util.HashSet;
import java.util.Set;

/** Installs only the bundled, reviewed YouTube scriptlets into exact YouTube origins. */
final class YoutubeScriptletInstaller {
    private static final Set<String> ALLOWED_ORIGINS = Collections.unmodifiableSet(
            new HashSet<>(Arrays.asList(
                    "https://youtube.com",
                    "https://www.youtube.com",
                    "https://m.youtube.com",
                    "https://music.youtube.com",
                    "https://youtube-nocookie.com",
                    "https://www.youtube-nocookie.com")));

    private YoutubeScriptletInstaller() {}

    static Set<String> allowedOrigins() { return ALLOWED_ORIGINS; }

    static boolean isSupported() {
        return WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT);
    }

    static boolean install(WebView view) {
        if (view == null || !isSupported()) return false;
        String script = NativeAdblock.youtubeScript();
        if (script.isEmpty()) return false;
        try {
            WebViewCompat.addDocumentStartJavaScript(view, script, ALLOWED_ORIGINS);
            return true;
        } catch (IllegalArgumentException | UnsupportedOperationException error) {
            // Keep ordinary browsing and network filtering available on old WebViews.
            return false;
        }
    }
}
