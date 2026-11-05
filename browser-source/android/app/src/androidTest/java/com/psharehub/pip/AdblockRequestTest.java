package com.psharehub.pip;

import android.view.View;
import android.view.ViewGroup;
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
