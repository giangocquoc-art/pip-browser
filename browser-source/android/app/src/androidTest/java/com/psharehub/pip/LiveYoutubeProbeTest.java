package com.psharehub.pip;

import android.graphics.Bitmap;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.EditorInfo;
import android.webkit.WebView;
import android.widget.EditText;
import android.widget.TextView;

import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;

import org.json.JSONObject;
import org.junit.Assume;
import org.junit.Test;
import org.junit.runner.RunWith;

import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

/** Opt-in network probe. It reports what YouTube served and never treats no ad as proof of blocking. */
@RunWith(AndroidJUnit4.class)
public final class LiveYoutubeProbeTest {
    private static final String URL = "https://m.youtube.com/watch?v=dQw4w9WgXcQ";
    private static final String PAGE_PROBE = "(() => {"
            + "const visible=e=>{if(!e)return false;const s=getComputedStyle(e),r=e.getBoundingClientRect();"
            + "return r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity||1)>0};"
            + "const selectors=['.ytp-ad-player-overlay','.ytp-ad-text','.ytp-ad-skip-button',"
            + "'.ytp-ad-preview-container','.ytp-ad-overlay-container',"
            + "'ytm-promoted-sparkles-web-renderer','ytd-ad-slot-renderer'];"
            + "const player=document.querySelector('#movie_player'),video=document.querySelector('video');"
            + "const response=window.ytInitialPlayerResponse||player?.getPlayerResponse?.()||{};"
            + "const body=(document.body?.innerText||'').slice(0,400);"
            + "return {url:location.href,title:document.title,ready:document.readyState==='complete',"
            + "offline:/connect to the internet|you're offline/i.test(body),"
            + "botWall:/confirm you're not a bot|unusual traffic|captcha/i.test(body),"
            + "player:!!player,video:!!video,videoReady:video?.readyState||0,"
            + "adShowing:!!player?.classList.contains('ad-showing'),"
            + "visibleAdMarkers:selectors.filter(q=>[...document.querySelectorAll(q)].some(visible)),"
            + "adFieldCounts:{playerAds:response.playerAds?.length||0,"
            + "adPlacements:response.adPlacements?.length||0,adSlots:response.adSlots?.length||0}}"
            + "})()";

    @Test public void observeLiveYoutubeInPipWebViewWhenExplicitlyRequested() throws Exception {
        boolean requested = "true".equalsIgnoreCase(
                InstrumentationRegistry.getArguments().getString("liveYoutube", "false"));
        Assume.assumeTrue("Live YouTube probe is opt-in; set liveYoutube=true to run it.", requested);
        assertTrue("Pip adblock engine did not initialize",
                NativeAdblock.initialize().get(90, TimeUnit.SECONDS));

        AtomicReference<String> latestState = new AtomicReference<>();
        JSONObject finalState = null;
        boolean loadedYoutubePage = false;
        try (ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class)) {
            scenario.onActivity(activity -> {
                EditText address = find(activity.findViewById(android.R.id.content), EditText.class);
                if (address == null) throw new AssertionError("Pip address field not found");
                address.setText(URL);
                address.onEditorAction(EditorInfo.IME_ACTION_GO);
            });

            long deadline = android.os.SystemClock.uptimeMillis() + TimeUnit.SECONDS.toMillis(90);
            while (android.os.SystemClock.uptimeMillis() < deadline) {
                CountDownLatch sampled = new CountDownLatch(1);
                scenario.onActivity(activity -> {
                    WebView webView = find(activity.findViewById(android.R.id.content), WebView.class);
                    if (webView == null) {
                        sampled.countDown();
                        return;
                    }
                    webView.evaluateJavascript(PAGE_PROBE, result -> {
                        latestState.set(result);
                        sampled.countDown();
                    });
                });
                sampled.await(3, TimeUnit.SECONDS);
                finalState = parse(latestState.get());
                if (finalState != null) {
                    String pageUrl = finalState.optString("url");
                    loadedYoutubePage = pageUrl.matches("https://([a-z0-9-]+\\.)?youtube\\.com/.*")
                            && finalState.optBoolean("ready");
                    if (loadedYoutubePage && (finalState.optBoolean("player")
                            || finalState.optBoolean("video") || finalState.optBoolean("offline")
                            || finalState.optBoolean("botWall"))) break;
                }
                Thread.sleep(1000);
            }

            if (finalState == null) finalState = new JSONObject();
            boolean scriptletRegistered = scenarioScriptletState(scenario);
            finalState.put("pipYoutubeScriptletRegistered", scriptletRegistered);
            finalState.put("liveAdConclusion", finalState.optBoolean("adShowing")
                    || finalState.optJSONArray("visibleAdMarkers") != null
                    && finalState.optJSONArray("visibleAdMarkers").length() > 0
                    ? "ad-visible" : "no-ad-observed; blocking-inconclusive");
            saveEvidence(finalState);

            if (finalState.optBoolean("offline") || finalState.optBoolean("botWall")
                    || !loadedYoutubePage) {
                System.out.println("PIP_LIVE_YOUTUBE_INCONCLUSIVE=" + finalState);
                Assume.assumeTrue("YouTube did not serve a usable watch page; probe is inconclusive: "
                        + finalState, false);
            }
            assertTrue("Pip did not register the document-start YouTube scriptlet on the live tab: "
                    + finalState, scriptletRegistered);
            boolean visibleAd = finalState.optBoolean("adShowing")
                    || finalState.optJSONArray("visibleAdMarkers") != null
                    && finalState.optJSONArray("visibleAdMarkers").length() > 0;
            System.out.println("PIP_LIVE_YOUTUBE_RESULT=" + finalState);
            assertFalse("A visible YouTube ad marker remained in Pip's live WebView: " + finalState,
                    visibleAd);
        }
    }

    private static boolean scenarioScriptletState(ActivityScenario<MainActivity> scenario) {
        AtomicReference<Boolean> registered = new AtomicReference<>(false);
        scenario.onActivity(activity -> {
            TextView status = find(activity.findViewById(android.R.id.content),
                    TextView.class, activity.getString(R.string.adblock_ready));
            CharSequence description = status == null ? null : status.getContentDescription();
            registered.set(description != null && description.toString()
                    .contains(activity.getString(R.string.adblock_scriptlet_ready)));
        });
        return Boolean.TRUE.equals(registered.get());
    }

    private static void saveEvidence(JSONObject state) throws Exception {
        android.content.Context context = InstrumentationRegistry.getInstrumentation().getTargetContext();
        File report = new File(context.getFilesDir(), "live-youtube-probe.json");
        try (FileOutputStream output = new FileOutputStream(report)) {
            output.write(state.toString(2).getBytes(StandardCharsets.UTF_8));
        }
        Bitmap screenshot = InstrumentationRegistry.getInstrumentation().getUiAutomation().takeScreenshot();
        if (screenshot == null) return;
        try (FileOutputStream output = new FileOutputStream(
                new File(context.getFilesDir(), "live-youtube-probe.png"))) {
            screenshot.compress(Bitmap.CompressFormat.PNG, 100, output);
        } finally {
            screenshot.recycle();
        }
    }

    private static JSONObject parse(String value) {
        if (value == null || "null".equals(value)) return null;
        try { return new JSONObject(value); }
        catch (org.json.JSONException ignored) { return null; }
    }

    private static <T extends View> T find(View root, Class<T> type) {
        return find(root, type, null);
    }

    private static <T extends View> T find(View root, Class<T> type, String text) {
        if (root == null) return null;
        if (type.isInstance(root)) {
            T candidate = type.cast(root);
            if (text == null || candidate instanceof TextView
                    && text.contentEquals(((TextView) candidate).getText())) return candidate;
        }
        if (root instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) root;
            for (int index = 0; index < group.getChildCount(); index++) {
                T found = find(group.getChildAt(index), type, text);
                if (found != null) return found;
            }
        }
        return null;
    }
}
