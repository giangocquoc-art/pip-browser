package com.psharehub.pip;

import android.view.View;
import android.view.ViewGroup;
import android.content.Intent;
import android.net.Uri;
import android.view.inputmethod.EditorInfo;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.WebChromeClient;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.Button;
import android.widget.EditText;
import android.content.Context;

import androidx.test.core.app.ApplicationProvider;
import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;

import org.junit.Test;
import org.junit.Before;
import org.junit.runner.RunWith;

import java.util.concurrent.atomic.AtomicReference;

import static androidx.test.espresso.Espresso.onView;
import static androidx.test.espresso.action.ViewActions.click;
import static androidx.test.espresso.matcher.RootMatchers.isDialog;
import static androidx.test.espresso.matcher.ViewMatchers.withContentDescription;
import static androidx.test.espresso.matcher.ViewMatchers.withText;
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
            Button tabs = findButton(root, activity.getString(R.string.tabs));
            assertNotNull(tabs);
            assertEquals(activity.getString(R.string.tabs) + " " + count, tabs.getText().toString());
        });
    }

    private static void selectTab(int position) {
        onView(withContentDescription(R.string.tabs)).perform(click());
        onView(withContentDescription(startsWith("Chuyển đến tab " + (position + 1) + ":")))
                .inRoot(isDialog()).perform(click());
    }

    private static void openNewTab() {
        onView(withContentDescription(R.string.tabs)).perform(click());
        onView(withText(R.string.new_tab)).inRoot(isDialog()).perform(click());
    }

    private static void closeCurrentTab() {
        onView(withContentDescription(R.string.tabs)).perform(click());
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
