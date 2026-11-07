package com.psharehub.pip;

import android.view.View;
import android.view.ViewGroup;
import android.content.Intent;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.view.inputmethod.EditorInfo;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.WebChromeClient;
import android.webkit.WebView;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebViewClient;
import android.widget.Button;
import android.widget.EditText;
import android.widget.TextView;
import android.content.Context;

import androidx.test.core.app.ApplicationProvider;
import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;

import org.junit.Test;
import org.junit.Before;
import org.junit.runner.RunWith;

import java.util.concurrent.atomic.AtomicReference;
import java.util.concurrent.TimeUnit;
import java.util.Set;
import java.util.concurrent.CountDownLatch;

import static androidx.test.espresso.Espresso.onView;
import static androidx.test.espresso.action.ViewActions.click;
import static androidx.test.espresso.matcher.RootMatchers.isDialog;
import static androidx.test.espresso.matcher.ViewMatchers.withContentDescription;
import static androidx.test.espresso.matcher.ViewMatchers.withText;
import static androidx.test.espresso.matcher.ViewMatchers.isDisplayed;
import static androidx.test.espresso.assertion.ViewAssertions.matches;
import static org.hamcrest.Matchers.containsString;
import static org.hamcrest.Matchers.startsWith;
import static org.junit.Assert.*;

/** Device tests exercise real Activity/WebView lifetimes without requiring a network response. */
@RunWith(AndroidJUnit4.class)
public final class MainActivityTest {
    private static final String FIRST = "https://first.pip.invalid/one";
    private static final String SECOND = "https://second.pip.invalid/two";

    @Before public void clearPersistedSession() {
        Context context = ApplicationProvider.getApplicationContext();
        context.getSharedPreferences("browser-session", Context.MODE_PRIVATE)
                .edit().clear().commit();
    }

    @Test public void uploadPickerUsesSystemDocumentsWithoutCameraCapture() {
        Intent single = MainActivity.createDocumentPickerIntent(
                WebChromeClient.FileChooserParams.MODE_OPEN);
        assertEquals(Intent.ACTION_OPEN_DOCUMENT, single.getAction());
        assertTrue(single.hasCategory(Intent.CATEGORY_OPENABLE));
        assertEquals("*/*", single.getType());
        assertFalse(single.getBooleanExtra(Intent.EXTRA_ALLOW_MULTIPLE, false));

        Intent typed = MainActivity.createDocumentPickerIntent(
                WebChromeClient.FileChooserParams.MODE_OPEN,
                new String[]{"image/png", " IMAGE/* ", "image/png", "text/html; charset=utf-8", "not-a-mime", "*/pdf"});
        assertArrayEquals(new String[]{"image/png", "image/*", "text/html"},
                typed.getStringArrayExtra(Intent.EXTRA_MIME_TYPES));

        Intent multiple = MainActivity.createDocumentPickerIntent(
                WebChromeClient.FileChooserParams.MODE_OPEN_MULTIPLE);
        assertTrue(multiple.getBooleanExtra(Intent.EXTRA_ALLOW_MULTIPLE, false));
        assertFalse("A website file input must not launch camera capture implicitly",
                multiple.hasExtra(android.provider.MediaStore.EXTRA_OUTPUT));

        Uri selected = Uri.parse("content://test.documents/item/one");
        Intent result = new Intent().setData(selected);
        assertArrayEquals(new Uri[]{selected}, MainActivity.parseDocumentPickerResult(
                android.app.Activity.RESULT_OK, result));
        assertNull(MainActivity.parseDocumentPickerResult(android.app.Activity.RESULT_CANCELED, null));
    }

    @Test public void mediaPermissionRequiresSecureOriginAndSupportedResources() {
        assertTrue(MainActivity.isSecureMediaOrigin("https://video.example"));
        assertTrue(MainActivity.isSecureMediaOrigin("http://localhost:3000"));
        assertTrue(MainActivity.isSecureMediaOrigin("http://127.0.0.1:8080"));
        assertFalse(MainActivity.isSecureMediaOrigin("http://video.example"));
        assertFalse(MainActivity.isSecureMediaOrigin("file:///tmp/video.html"));

        assertArrayEquals(new String[]{android.Manifest.permission.CAMERA},
                MainActivity.androidPermissionsFor(new String[]{
                        android.webkit.PermissionRequest.RESOURCE_VIDEO_CAPTURE}));
        assertArrayEquals(new String[]{android.Manifest.permission.CAMERA,
                        android.Manifest.permission.RECORD_AUDIO},
                MainActivity.androidPermissionsFor(new String[]{
                        android.webkit.PermissionRequest.RESOURCE_VIDEO_CAPTURE,
                        android.webkit.PermissionRequest.RESOURCE_AUDIO_CAPTURE}));
        assertEquals(0, MainActivity.androidPermissionsFor(new String[]{"unknown"}).length);
    }

    @Test public void androidControlsSwitchToAccessibleCompactLabelsForLargeSystemText() {
        assertFalse(MainActivity.shouldUseCompactControls(1.0f));
        assertFalse(MainActivity.shouldUseCompactControls(1.2f));
        assertTrue(MainActivity.shouldUseCompactControls(1.3f));
        assertTrue(MainActivity.shouldUseCompactControls(1.5f));
        assertTrue(MainActivity.shouldUseCompactControls(2.0f));
    }

    @Test public void nativeBrowserButtonsKeepAccessibleTouchTargets() {
        try (ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class)) {
            scenario.onActivity(activity -> {
                ArrayList<Button> buttons = new ArrayList<>();
                collectButtons(activity.findViewById(android.R.id.content), buttons);
                assertEquals("five navigation controls, address action, and filter status",
                        7, buttons.size());
                int minimumTouchSize = Math.round(48 * activity.getResources()
                        .getDisplayMetrics().density);
                for (Button button : buttons) {
                    assertNotNull("every browser button needs an accessible name", button.getContentDescription());
                    assertTrue(button.getContentDescription().toString(),
                            button.getWidth() >= minimumTouchSize);
                    assertTrue(button.getContentDescription().toString(),
                            button.getHeight() >= minimumTouchSize);
                }
            });
        }
    }

    @Test public void youtubeScriptletsRegisterOnlyForExactSecureYoutubeOrigins() throws Exception {
        assertTrue("native filter and pinned scriptlets should initialize",
                NativeAdblock.initialize().get(90, TimeUnit.SECONDS));
        Set<String> origins = YoutubeScriptletInstaller.allowedOrigins();
        assertEquals(6, origins.size());
        for (String origin : origins) {
            assertTrue("scriptlet origin must be HTTPS", origin.startsWith("https://"));
            assertFalse("scriptlet origin must not use a wildcard", origin.contains("*"));
            assertTrue("scriptlet origin must be a known YouTube host",
                    origin.endsWith("youtube.com") || origin.endsWith("youtube-nocookie.com"));
        }
        boolean supported = YoutubeScriptletInstaller.isSupported();
        AtomicReference<Boolean> installed = new AtomicReference<>();
        try (ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class)) {
            // Select through the same UI path users take so the displayed status is
            // refreshed by normal tab-selection lifecycle callbacks.
            selectTab(0);
            scenario.onActivity(activity -> {
                TextView status = findTextViewContaining(
                        activity.findViewById(android.R.id.content),
                        activity.getString(R.string.adblock_ready));
                assertNotNull("filter status must match the installed scriptlet state; views="
                                + describeTextViews(activity.findViewById(android.R.id.content)), status);
                assertTrue("filter status must expose native button semantics", status instanceof Button);
                assertTrue("filter status must be operable by touch", status.isClickable());
                assertTrue("filter status must be reachable by keyboard/accessibility", status.isFocusable());
                int minimumTouchHeight = Math.round(48 * activity.getResources()
                        .getDisplayMetrics().density);
                assertTrue("filter status target height " + status.getMinimumHeight()
                                + "px must be at least " + minimumTouchHeight + "px (48dp)",
                        status.getMinimumHeight() >= minimumTouchHeight);
                assertNotNull("filter status needs visible pressed/focus feedback", status.getBackground());
                assertTrue("accessible status needs a full screen-reader description",
                        status.getContentDescription().toString().contains(
                                activity.getString(R.string.adblock_ready_accessible)));
                assertTrue("accessible status must explain scriptlet availability",
                        status.getContentDescription().toString().contains(supported
                                ? "Scriptlet YouTube đã được đăng ký"
                                : "Scriptlet YouTube chưa sẵn sàng"));
                assertTrue("filter status click should open its details", status.performClick());
                WebView probe = new WebView(activity);
                installed.set(YoutubeScriptletInstaller.install(probe));
                probe.destroy();
            });
            onView(withText(R.string.adblock_details_title)).inRoot(isDialog())
                    .check(matches(isDisplayed()));
            onView(withText(containsString("không phải số quảng cáo"))).inRoot(isDialog())
                    .check(matches(isDisplayed()));
            onView(withText(containsString("Một số quảng cáo YouTube vẫn có thể xuất hiện")))
                    .inRoot(isDialog()).check(matches(isDisplayed()));
        }
        assertEquals("document-start script registration should match WebView capability",
                supported, installed.get());
    }

    @Test public void mobileYoutubePlayerFetchScriptletRemovesAdFieldsAndPreservesVideoData() throws Exception {
        assertTrue("native filter and pinned scriptlets should initialize",
                NativeAdblock.initialize().get(90, TimeUnit.SECONDS));
        assertTrue("test requires WebView document-start support",
                YoutubeScriptletInstaller.isSupported());
        CountDownLatch resultReady = new CountDownLatch(1);
        AtomicReference<String> result = new AtomicReference<>();
        AtomicReference<WebView> probeRef = new AtomicReference<>();
        try (ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class)) {
            scenario.onActivity(activity -> {
                WebView probe = new WebView(activity);
                probeRef.set(probe);
                probe.getSettings().setJavaScriptEnabled(true);
                assertTrue(YoutubeScriptletInstaller.install(probe));
                probe.setWebViewClient(new WebViewClient() {
                    @Override public WebResourceResponse shouldInterceptRequest(
                            WebView view, WebResourceRequest request) {
                        if (!"m.youtube.com".equals(request.getUrl().getHost())
                                || !"/youtubei/v1/player".equals(request.getUrl().getPath())) return null;
                        String body = "{\"adPlacements\":[{\"adSlotRenderer\":{}}],"
                                + "\"adSlots\":[{}],\"videoDetails\":{\"title\":\"fixture\"}}";
                        return new WebResourceResponse("application/json", "UTF-8", 200, "OK",
                                java.util.Collections.singletonMap("Cache-Control", "no-store"),
                                new java.io.ByteArrayInputStream(body.getBytes(
                                        java.nio.charset.StandardCharsets.UTF_8)));
                    }

                    @Override public void onPageFinished(WebView view, String url) {
                        pollScriptletResult(view, result, resultReady,
                                SystemClock.uptimeMillis() + TimeUnit.SECONDS.toMillis(10));
                    }
                });
                String html = "<!doctype html><meta charset='utf-8'><script>"
                        + "const fromFetch=fetch('/youtubei/v1/player?key=fetch',{method:'POST',body:'{}'})"
                        + ".then(r=>r.json());"
                        + "const fromXhr=new Promise((resolve,reject)=>{const x=new XMLHttpRequest();"
                        + "x.open('POST','/youtubei/v1/player?key=xhr');"
                        + "x.onload=()=>{try{resolve(JSON.parse(x.responseText))}catch(e){reject(e)}};"
                        + "x.onerror=()=>reject(new Error('XHR failed'));x.send('{}')});"
                        + "Promise.all([fromFetch,fromXhr]).then(x=>window.__pipScriptletResult=JSON.stringify(x))"
                        + ".catch(e=>window.__pipScriptletResult='ERROR:'+e)</script>";
                probe.loadDataWithBaseURL("https://m.youtube.com/watch?v=fixture", html,
                        "text/html", "UTF-8", null);
            });
            assertTrue("fixture player response did not finish", resultReady.await(15, TimeUnit.SECONDS));
        } finally {
            WebView probe = probeRef.get();
            if (probe != null) probe.post(probe::destroy);
        }
        assertNotNull("fixture page did not expose the player response", result.get());
        assertTrue("non-ad video data must survive the scriptlet", result.get().contains("videoDetails"));
        assertFalse("player ad placements should be pruned", result.get().contains("adPlacements"));
        assertFalse("player ad slots should be pruned", result.get().contains("adSlots"));
    }

    private static void pollScriptletResult(WebView view, AtomicReference<String> result,
            CountDownLatch ready, long deadlineUptime) {
        view.evaluateJavascript("window.__pipScriptletResult || null", value -> {
            if (value != null && !"null".equals(value)) {
                result.set(value);
                ready.countDown();
            } else if (SystemClock.uptimeMillis() >= deadlineUptime) {
                ready.countDown();
            } else {
                new Handler(Looper.getMainLooper()).postDelayed(
                        () -> pollScriptletResult(view, result, ready, deadlineUptime), 100);
            }
        });
    }

    @Test public void selectedTabAndUrlsSurviveActivityRecreation() {
        try (ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class)) {
            navigateWithoutWaitingForNetwork(scenario, FIRST);
            openNewTab();
            navigateWithoutWaitingForNetwork(scenario, SECOND);
            assertUi(scenario, 2, SECOND);

            selectTab(0);
            assertUi(scenario, 2, FIRST);
            scenario.recreate();
            assertUi(scenario, 2, FIRST);

            selectTab(1);
            assertUi(scenario, 2, SECOND);
            closeCurrentTab();
            assertUi(scenario, 1, FIRST);
        }
    }

    @Test public void lateStartFromPreviousPageCannotOverwriteRestoredUrl() {
        try (ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class)) {
            navigateWithoutWaitingForNetwork(scenario, FIRST);
            scenario.onActivity(activity -> {
                WebView view = webView(activity);
                view.stopLoading();
                view.getWebViewClient().onPageStarted(view, Address.HOME, null);
            });
            assertUi(scenario, 1, FIRST);
            scenario.recreate();
            assertUi(scenario, 1, FIRST);
        }
    }

    @Test public void selectedTabAndUrlsSurviveClosingAndReopeningTheApp() {
        ActivityScenario<MainActivity> first = ActivityScenario.launch(MainActivity.class);
        navigateWithoutWaitingForNetwork(first, FIRST);
        openNewTab();
        navigateWithoutWaitingForNetwork(first, SECOND);
        assertUi(first, 2, SECOND);
        first.close();

        try (ActivityScenario<MainActivity> reopened = ActivityScenario.launch(MainActivity.class)) {
            assertUi(reopened, 2, SECOND);
            selectTab(0);
            assertUi(reopened, 2, FIRST);
        }
    }

    @Test public void staleCallbacksCannotClearReplacementRendererOrLastTab() {
        try (ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class)) {
            navigateWithoutWaitingForNetwork(scenario, FIRST);
            AtomicReference<WebView> oldView = new AtomicReference<>();
            AtomicReference<WebViewClient> oldClient = new AtomicReference<>();
            AtomicReference<WebChromeClient> oldChrome = new AtomicReference<>();
            scenario.onActivity(activity -> {
                WebView view = webView(activity);
                oldView.set(view);
                oldClient.set(view.getWebViewClient());
                oldChrome.set(view.getWebChromeClient());
                assertTrue(oldClient.get().onRenderProcessGone(view, rendererExit()));
                assertNull(find(activity.findViewById(android.R.id.content), WebView.class));
            });

            onView(withContentDescription(R.string.reload)).perform(click());
            scenario.onActivity(activity -> {
                WebView replacement = webView(activity);
                replacement.stopLoading();
                assertNotSame(oldView.get(), replacement);
                deliverStaleCallbacks(oldView.get(), oldClient.get(), oldChrome.get());
                assertSame(replacement, webView(activity));
            });
            assertUi(scenario, 1, FIRST);

            // Closing the final tab creates a new one. Queued old callbacks must
            // neither index a removed selection nor overwrite the new tab's state.
            scenario.onActivity(activity -> {
                WebView view = webView(activity);
                oldView.set(view);
                oldClient.set(view.getWebViewClient());
                oldChrome.set(view.getWebChromeClient());
            });
            closeCurrentTab();
            scenario.onActivity(activity -> {
                WebView replacement = webView(activity);
                replacement.stopLoading();
                assertNotSame(oldView.get(), replacement);
                deliverStaleCallbacks(oldView.get(), oldClient.get(), oldChrome.get());
                assertSame(replacement, webView(activity));
            });
            assertUi(scenario, 1, Address.HOME);
        }
    }

    private static void navigateWithoutWaitingForNetwork(
            ActivityScenario<MainActivity> scenario, String url) {
        scenario.onActivity(activity -> {
            webView(activity).stopLoading();
            EditText address = find(activity.findViewById(android.R.id.content), EditText.class);
            assertNotNull(address);
            address.setText(url);
            address.onEditorAction(EditorInfo.IME_ACTION_GO);
            webView(activity).stopLoading();
        });
    }

    private static void assertUi(ActivityScenario<MainActivity> scenario, int count, String url) {
        scenario.onActivity(activity -> {
            webView(activity).stopLoading();
            View root = activity.findViewById(android.R.id.content);
            EditText address = find(root, EditText.class);
            assertNotNull(address);
            assertEquals(url, address.getText().toString());
            String tabDescription = activity.getString(R.string.tabs_count, count);
            Button tabs = findButton(root, tabDescription);
            assertNotNull(tabs);
            assertEquals(tabDescription, tabs.getContentDescription().toString());
            assertEquals(activity.getString(R.string.tabs) + " " + count, tabs.getText().toString());
        });
    }

    private static TextView findTextViewContaining(View root, String text) {
        if (root instanceof TextView) {
            TextView candidate = (TextView) root;
            if (candidate.getText().toString().contains(text)) return candidate;
        }
        if (root instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) root;
            for (int index = 0; index < group.getChildCount(); index++) {
                TextView candidate = findTextViewContaining(group.getChildAt(index), text);
                if (candidate != null) return candidate;
            }
        }
        return null;
    }

    private static String describeTextViews(View root) {
        if (root instanceof TextView) return ((TextView) root).getText().toString();
        if (!(root instanceof ViewGroup)) return "";
        StringBuilder result = new StringBuilder();
        ViewGroup group = (ViewGroup) root;
        for (int index = 0; index < group.getChildCount(); index++) {
            String child = describeTextViews(group.getChildAt(index));
            if (!child.isEmpty()) {
                if (result.length() > 0) result.append(" | ");
                result.append(child);
            }
        }
        return result.toString();
    }

    private static void selectTab(int position) {
        onView(withContentDescription(startsWith("Tab "))).perform(click());
        onView(withContentDescription(startsWith("Chuyển đến tab " + (position + 1) + ":")))
                .inRoot(isDialog()).perform(click());
    }

    private static void openNewTab() {
        onView(withContentDescription(startsWith("Tab "))).perform(click());
        onView(withText(R.string.new_tab)).inRoot(isDialog()).perform(click());
    }

    private static void closeCurrentTab() {
        onView(withContentDescription(startsWith("Tab "))).perform(click());
        onView(withContentDescription(startsWith("Đóng tab đang chọn:")))
                .inRoot(isDialog()).perform(click());
    }

    private static void deliverStaleCallbacks(
            WebView view, WebViewClient client, WebChromeClient chrome) {
        client.onPageStarted(view, "https://stale.pip.invalid/", null);
        chrome.onProgressChanged(view, 99);
        chrome.onReceivedTitle(view, "stale renderer");
        assertTrue(client.onRenderProcessGone(view, rendererExit()));
    }

    private static RenderProcessGoneDetail rendererExit() {
        return new RenderProcessGoneDetail() {
            @Override public boolean didCrash() { return false; }
            @Override public int rendererPriorityAtExit() { return WebView.RENDERER_PRIORITY_BOUND; }
        };
    }

    private static WebView webView(MainActivity activity) {
        WebView view = find(activity.findViewById(android.R.id.content), WebView.class);
        assertNotNull("an active browser WebView must be attached", view);
        return view;
    }

    private static Button findButton(View root, String description) {
        if (root instanceof Button && description.contentEquals(root.getContentDescription())) {
            return (Button) root;
        }
        if (root instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) root;
            for (int i = 0; i < group.getChildCount(); i++) {
                Button found = findButton(group.getChildAt(i), description);
                if (found != null) return found;
            }
        }
        return null;
    }

    private static void collectButtons(View root, List<Button> result) {
        if (root instanceof Button) result.add((Button) root);
        if (root instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) root;
            for (int index = 0; index < group.getChildCount(); index++) {
                collectButtons(group.getChildAt(index), result);
            }
        }
    }

    private static <T extends View> T find(View root, Class<T> type) {
        if (type.isInstance(root)) return type.cast(root);
        if (root instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) root;
            for (int i = 0; i < group.getChildCount(); i++) {
                T found = find(group.getChildAt(i), type);
                if (found != null) return found;
            }
        }
        return null;
    }
}
