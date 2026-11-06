package com.psharehub.pip;

import android.view.View;
import android.view.ViewGroup;
import android.net.Uri;
import android.view.inputmethod.EditorInfo;
import android.widget.EditText;
import android.webkit.ServiceWorkerClient;
import android.webkit.ServiceWorkerController;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebView;
import android.webkit.WebViewClient;

import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;

import org.junit.Test;
import org.junit.runner.RunWith;

import java.io.ByteArrayInputStream;
import java.nio.charset.StandardCharsets;
import java.util.Collections;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;

import static org.junit.Assert.*;

/** Real JNI + WebView callbacks. HTTPS fixture responses are test-only and never use TLS bypasses. */
@RunWith(AndroidJUnit4.class)
public final class AdblockRequestTest {
    private static final String ORIGIN = "https://fixture.pip.invalid";
    private static final String AD = "https://ad.doubleclick.net/pip-fixture.js";

    @Test public void actualAutomaticFormPostCannotBypassDestinationPolicy() throws Exception {
        assertTrue(NativeAdblock.initialize().get(30, TimeUnit.SECONDS));
        CountDownLatch deniedPost = new CountDownLatch(1);
        AtomicInteger unexpectedAllowed = new AtomicInteger();
        try (ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class)) {
            scenario.onActivity(activity -> {
                WebView view = findWebView(activity.findViewById(android.R.id.content));
                assertNotNull(view);
                view.stopLoading();
                WebViewClient production = view.getWebViewClient();
                assertNotNull(AdblockRequests.intercept(navigation("https://ad88vip6.com/post", "POST"),
                        ORIGIN, ORIGIN));
                assertNull("Bare typed host remains exempt after browser URL normalization",
                        AdblockRequests.intercept(navigation("https://AD88VIP6.COM:443/", "POST"),
                                ORIGIN, "https://ad88vip6.com"));
                view.setWebViewClient(new WebViewClient() {
                    @Override public boolean shouldOverrideUrlLoading(WebView webView, WebResourceRequest request) {
                        return production.shouldOverrideUrlLoading(webView, request);
                    }
                    @Override public WebResourceResponse shouldInterceptRequest(WebView webView, WebResourceRequest request) {
                        WebResourceResponse decision = production.shouldInterceptRequest(webView, request);
                        if ("ad88vip6.com".equals(request.getUrl().getHost())) {
                            if ("POST".equals(request.getMethod()) && decision != null && decision.getStatusCode() == 204) {
                                deniedPost.countDown();
                            } else unexpectedAllowed.incrementAndGet();
                            // Even a failed assertion never lets a test POST reach an external host.
                            return decision != null ? decision : response("text/html", "unexpected allowed request");
                        }
                        if (decision != null) return decision;
                        if (request.isForMainFrame()) return response("text/html", "<!doctype html>"
                                + "<form id='jump' method='POST' action='https://ad88vip6.com/post'>"
                                + "<input name='fixture' value='offline'></form>"
                                + "<script>document.getElementById('jump').submit();</script>");
                        return response("text/plain", "");
                    }
                    @Override public void onPageStarted(WebView webView, String url, android.graphics.Bitmap icon) {
                        production.onPageStarted(webView, url, icon);
                    }
                    @Override public void onPageFinished(WebView webView, String url) {
                        production.onPageFinished(webView, url);
                    }
                });
                view.loadUrl(ORIGIN + "/automatic-post");
            });
            assertTrue("Actual main-frame POST must be denied in interception", deniedPost.await(20, TimeUnit.SECONDS));
            assertEquals(0, unexpectedAllowed.get());
        }
    }

    @Test public void actualScriptNavigationToCasinoIsStoppedBeforeRequestOrCommit() throws Exception {
        assertTrue(NativeAdblock.initialize().get(30, TimeUnit.SECONDS));
        CountDownLatch blocked = new CountDownLatch(1);
        AtomicInteger casinoRequests = new AtomicInteger();
        try (ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class)) {
            scenario.onActivity(activity -> {
                WebView view = findWebView(activity.findViewById(android.R.id.content));
                assertNotNull(view);
                view.stopLoading();
                WebViewClient production = view.getWebViewClient();
                view.setWebViewClient(new WebViewClient() {
                    @Override public boolean shouldOverrideUrlLoading(WebView webView, WebResourceRequest request) {
                        boolean denied = production.shouldOverrideUrlLoading(webView, request);
                        if ("ad88vip6.com".equals(request.getUrl().getHost()) && denied) blocked.countDown();
                        return denied;
                    }
                    @Override public WebResourceResponse shouldInterceptRequest(WebView webView, WebResourceRequest request) {
                        if ("ad88vip6.com".equals(request.getUrl().getHost())) {
                            casinoRequests.incrementAndGet();
                            return response("text/html", "<!doctype html><title>Unexpected request</title>");
                        }
                        if (request.isForMainFrame()) return response("text/html", "<!doctype html><script>"
                                + "location.assign('https://ad88vip6.com/automatic-redirect');</script>");
                        return response("text/plain", "");
                    }
                    @Override public void onPageStarted(WebView webView, String url, android.graphics.Bitmap icon) {
                        production.onPageStarted(webView, url, icon);
                    }
                    @Override public void onPageFinished(WebView webView, String url) {
                        production.onPageFinished(webView, url);
                    }
                });
                view.loadUrl(ORIGIN + "/script-redirect");
            });
            assertTrue("Actual JS navigation must hit and be denied by the production callback",
                    blocked.await(20, TimeUnit.SECONDS));
            scenario.onActivity(activity -> {
                WebView view = findWebView(activity.findViewById(android.R.id.content));
                assertNotNull(view);
                assertFalse("Blocked destination must not commit", view.getUrl().startsWith("https://ad88vip6.com/"));
                assertEquals("Blocked destination must not reach request interception", 0, casinoRequests.get());
            });
        }
    }

    @Test public void productionNavigationCallbackBlocksCasinoButPreservesExplicitOmniboxUrl() throws Exception {
        assertTrue(NativeAdblock.initialize().get(30, TimeUnit.SECONDS));
        try (ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class)) {
            scenario.onActivity(activity -> {
                View root = activity.findViewById(android.R.id.content);
                WebView view = findWebView(root);
                assertNotNull(view);
                view.stopLoading();
                WebViewClient production = view.getWebViewClient();
                assertTrue(production.shouldOverrideUrlLoading(view, navigation("https://ad88vip6.com/home/register")));
                assertTrue(production.shouldOverrideUrlLoading(view, navigation("https://promo.ad88vip6.com/")));
                assertFalse(production.shouldOverrideUrlLoading(view, navigation("https://notad88vip6.com/")));
                assertFalse(production.shouldOverrideUrlLoading(view, navigation("https://ad88vip6.com.example.org/")));
                assertFalse(production.shouldOverrideUrlLoading(view, navigation("https://www.google.com/")));
                // Test-only fixture prevents the deliberately typed casino URL from reaching the network.
                view.setWebViewClient(new WebViewClient() {
                    @Override public WebResourceResponse shouldInterceptRequest(WebView webView, WebResourceRequest request) {
                        return response("text/html", "<!doctype html><title>Offline fixture</title>");
                    }
                });
                EditText address = findAddress(root);
                assertNotNull(address);
                String typed = "https://ad88vip6.com/user-chose-this";
                address.setText(typed);
                address.onEditorAction(EditorInfo.IME_ACTION_GO);
                view.stopLoading();
                assertFalse("Explicit exact URL is permitted", production.shouldOverrideUrlLoading(view, navigation(typed)));
                assertTrue("A different destination remains filtered", production.shouldOverrideUrlLoading(view,
                        navigation("https://ad88vip6.com/redirect-other-path")));
            });
        }
    }

    private static WebResourceRequest navigation(String url) {
        return navigation(url, "GET");
    }

    private static WebResourceRequest navigation(String url, String method) {
        return new WebResourceRequest() {
            @Override public Uri getUrl() { return Uri.parse(url); }
            @Override public boolean isForMainFrame() { return true; }
            @Override public boolean isRedirect() { return true; }
            @Override public boolean hasGesture() { return false; }
            @Override public String getMethod() { return method; }
            @Override public java.util.Map<String, String> getRequestHeaders() { return Collections.emptyMap(); }
        };
    }

    private static EditText findAddress(View root) {
        if (root instanceof EditText) return (EditText) root;
        if (root instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) root;
            for (int i = 0; i < group.getChildCount(); i++) {
                EditText found = findAddress(group.getChildAt(i));
                if (found != null) return found;
            }
        }
        return null;
    }

    @Test public void actualDocumentSubresourcesUseNativeBlockAndAllowDecisions() throws Exception {
        assertTrue("JNI engine must load", NativeAdblock.initialize().get(30, TimeUnit.SECONDS));
        CountDownLatch requests = new CountDownLatch(2);
        AtomicInteger blocked = new AtomicInteger();
        AtomicInteger allowed = new AtomicInteger();
        long before = NativeAdblock.blockedCount();
        try (ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class)) {
            scenario.onActivity(activity -> {
                WebView view = findWebView(activity.findViewById(android.R.id.content));
                assertNotNull(view);
                view.stopLoading();
                WebViewClient production = view.getWebViewClient();
                view.setWebViewClient(new WebViewClient() {
                    @Override public WebResourceResponse shouldInterceptRequest(WebView webView, WebResourceRequest request) {
                        WebResourceResponse decision = production.shouldInterceptRequest(webView, request);
                        String url = request.getUrl().toString();
                        if (url.equals(AD)) {
                            if (decision != null && decision.getStatusCode() == 204) blocked.incrementAndGet();
                            requests.countDown();
                        } else if (url.equals(ORIGIN + "/control.js")) {
                            if (decision == null) allowed.incrementAndGet();
                            requests.countDown();
                        }
                        if (decision != null) return decision;
                        if (request.isForMainFrame()) return response("text/html", "<!doctype html><script src='" + AD
                                + "'></script><script src='/control.js'></script>");
                        return response("application/javascript", "/* offline control */");
                    }
                    @Override public void onPageStarted(WebView webView, String url, android.graphics.Bitmap icon) {
                        production.onPageStarted(webView, url, icon);
                    }
                    @Override public void onPageFinished(WebView webView, String url) {
                        production.onPageFinished(webView, url);
                    }
                });
                view.loadUrl(ORIGIN + "/network-fixture");
            });
            assertTrue("WebView must request both actual script resources", requests.await(20, TimeUnit.SECONDS));
            assertEquals(1, blocked.get());
            assertEquals(1, allowed.get());
            assertTrue(NativeAdblock.blockedCount() > before);
        }
    }

    @Test public void actualServiceWorkerFetchesUseProductionNativeFilter() throws Exception {
        assertTrue("JNI engine must load", NativeAdblock.initialize().get(30, TimeUnit.SECONDS));
        CountDownLatch requests = new CountDownLatch(2);
        AtomicInteger blocked = new AtomicInteger();
        AtomicInteger allowed = new AtomicInteger();
        String origin = "https://worker-fixture.pip.invalid";
        String worker = "self.addEventListener('install',e=>self.skipWaiting());"
                + "self.addEventListener('activate',e=>e.waitUntil(self.clients.claim()));"
                + "self.addEventListener('message',e=>e.waitUntil(Promise.allSettled([fetch('" + AD
                + "'),fetch('/worker-control')])));";
        try (ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class)) {
            scenario.onActivity(activity -> {
                ServiceWorkerClient production = AdblockRequests.serviceWorkerClient();
                ServiceWorkerController.getInstance().setServiceWorkerClient(new ServiceWorkerClient() {
                    @Override public WebResourceResponse shouldInterceptRequest(WebResourceRequest request) {
                        WebResourceResponse decision = production.shouldInterceptRequest(request);
                        String url = request.getUrl().toString();
                        if (url.equals(AD)) {
                            if (decision != null && decision.getStatusCode() == 204) blocked.incrementAndGet();
                            requests.countDown();
                        } else if (url.equals(origin + "/worker-control")) {
                            if (decision == null) allowed.incrementAndGet();
                            requests.countDown();
                        }
                        if (decision != null) return decision;
                        return response("application/javascript", url.endsWith("/sw.js") ? worker : "control");
                    }
                });
                WebView view = findWebView(activity.findViewById(android.R.id.content));
                assertNotNull(view);
                view.stopLoading();
                view.setWebViewClient(new WebViewClient() {
                    @Override public WebResourceResponse shouldInterceptRequest(WebView webView, WebResourceRequest request) {
                        if (request.getUrl().getPath().equals("/sw.js")) return response("application/javascript", worker);
                        return response("text/html", "<!doctype html><script>navigator.serviceWorker.register('/sw.js')"
                                + ".then(()=>navigator.serviceWorker.ready).then(r=>r.active.postMessage('check'));</script>");
                    }
                });
                view.loadUrl(origin + "/");
            });
            assertTrue("ServiceWorker must fetch both resources through its own production hook",
                    requests.await(25, TimeUnit.SECONDS));
            assertEquals(1, blocked.get());
            assertEquals(1, allowed.get());
        } finally {
            androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().runOnMainSync(
                    AdblockRequests::installServiceWorkerFiltering);
        }
    }

    private static WebResourceResponse response(String mime, String body) {
        return new WebResourceResponse(mime, "UTF-8", 200, "OK",
                Collections.singletonMap("Cache-Control", "no-store"),
                new ByteArrayInputStream(body.getBytes(StandardCharsets.UTF_8)));
    }

    private static WebView findWebView(View root) {
        if (root instanceof WebView) return (WebView) root;
        if (root instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) root;
            for (int i = 0; i < group.getChildCount(); i++) {
                WebView found = findWebView(group.getChildAt(i));
                if (found != null) return found;
            }
        }
        return null;
    }
}
