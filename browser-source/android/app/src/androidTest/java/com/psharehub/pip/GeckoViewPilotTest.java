package com.psharehub.pip;

import android.content.Context;

import androidx.test.core.app.ApplicationProvider;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;

import org.junit.Test;
import org.junit.runner.RunWith;
import org.mozilla.geckoview.GeckoRuntime;
import org.mozilla.geckoview.GeckoRuntimeSettings;
import org.mozilla.geckoview.GeckoSession;
import org.mozilla.geckoview.GeckoSessionSettings;

import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.LinkedBlockingQueue;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

import static org.junit.Assert.*;

/** Debug-only engine evaluation; it does not change the shipping WebView browser. */
@RunWith(AndroidJUnit4.class)
public final class GeckoViewPilotTest {
    @Test public void geckoProfilesIsolateAndPersistCookiesWhilePrivateSessionDoesNotTouchThem() throws Exception {
        try (HttpFixture fixture = new HttpFixture()) {
            Context context = ApplicationProvider.getApplicationContext();
            AtomicReference<GeckoRuntime> runtimeResult = new AtomicReference<>();
            InstrumentationRegistry.getInstrumentation().runOnMainSync(() ->
                    runtimeResult.set(GeckoViewPilotActivity.getOrCreateRuntime(context)));
            GeckoRuntime runtime = runtimeResult.get();
            GeckoSession profileA = openSession(runtime,
                    GeckoViewPilotActivity.createSessionSettings(GeckoViewPilotActivity.PROFILE_A, false));
            GeckoSession profileB = openSession(runtime,
                    GeckoViewPilotActivity.createSessionSettings(GeckoViewPilotActivity.PROFILE_B, false));
            GeckoSession privateSession = openSession(runtime,
                    GeckoViewPilotActivity.createSessionSettings(null, true));
            try {
                navigate(profileA, fixture.url("/set?value=alpha"));
                HttpFixture.Request setA = fixture.take("/set?value=alpha");
                assertNull(setA.cookie);

                navigate(profileB, fixture.url("/echo"));
                assertNull("A different context must not receive profile A cookies",
                        fixture.take("/echo").cookie);

                navigate(profileA, fixture.url("/echo"));
                assertEquals("pip=alpha", fixture.take("/echo").cookie);

                navigate(privateSession, fixture.url("/set?value=secret"));
                fixture.take("/set?value=secret");
                navigate(profileA, fixture.url("/echo"));
                assertEquals("Private mode must not change the persistent profile",
                        "pip=alpha", fixture.take("/echo").cookie);

                closeSession(profileA);
                profileA = openSession(runtime,
                        GeckoViewPilotActivity.createSessionSettings(
                                GeckoViewPilotActivity.PROFILE_A, false));
                navigate(profileA, fixture.url("/echo"));
                assertEquals("Reopening a profile session must retain its cookie jar",
                        "pip=alpha", fixture.take("/echo").cookie);
            } finally {
                closeSession(profileA);
                closeSession(profileB);
                closeSession(privateSession);
            }
        }
    }

    @Test public void profileAndPrivateSessionSettingsAreSeparated() {
        GeckoSessionSettings profileA = GeckoViewPilotActivity.createSessionSettings(
                GeckoViewPilotActivity.PROFILE_A, false);
        GeckoSessionSettings profileB = GeckoViewPilotActivity.createSessionSettings(
                GeckoViewPilotActivity.PROFILE_B, false);
        GeckoSessionSettings privateSession = GeckoViewPilotActivity.createSessionSettings(null, true);

        assertEquals(GeckoViewPilotActivity.PROFILE_A, profileA.getContextId());
        assertEquals(GeckoViewPilotActivity.PROFILE_B, profileB.getContextId());
        assertNotEquals(profileA.getContextId(), profileB.getContextId());
        assertTrue(privateSession.getUsePrivateMode());
        assertNull(privateSession.getContextId());
    }

    @Test public void googleDoHIsConfiguredInStrictOnlyMode() {
        GeckoRuntimeSettings settings = GeckoViewPilotActivity.createRuntimeSettings();

        assertEquals(GeckoViewPilotActivity.GOOGLE_DOH,
                settings.getTrustedRecursiveResolverUri());
        assertEquals(GeckoRuntimeSettings.TRR_MODE_ONLY,
                settings.getTrustedRecusiveResolverMode());
    }

    @Test public void strictGoogleDoHLoadsAnExternalHostnameInGeckoSession() throws Exception {
        Context context = ApplicationProvider.getApplicationContext();
        AtomicReference<GeckoRuntime> runtimeResult = new AtomicReference<>();
        InstrumentationRegistry.getInstrumentation().runOnMainSync(() ->
                runtimeResult.set(GeckoViewPilotActivity.getOrCreateRuntime(context)));
        GeckoSession session = openSession(runtimeResult.get(),
                GeckoViewPilotActivity.createSessionSettings(GeckoViewPilotActivity.PROFILE_A, false));
        CountDownLatch pageStopped = new CountDownLatch(1);
        AtomicReference<Boolean> loadSucceeded = new AtomicReference<>(false);
        AtomicReference<String> finalUri = new AtomicReference<>();
        InstrumentationRegistry.getInstrumentation().runOnMainSync(() -> {
            session.setNavigationDelegate(new GeckoSession.NavigationDelegate() {
                @Override public void onLocationChange(GeckoSession page, String url,
                        List<GeckoSession.PermissionDelegate.ContentPermission> permissions,
                        Boolean hasUserGesture) {
                    finalUri.set(url);
                }
            });
            session.setProgressDelegate(new GeckoSession.ProgressDelegate() {
                @Override public void onPageStop(GeckoSession page, boolean success) {
                    String uri = finalUri.get();
                    if (uri == null || uri.equals("about:blank")) return;
                    loadSucceeded.set(success);
                    pageStopped.countDown();
                }
            });
            session.loadUri("https://example.com/");
        });
        try {
            assertTrue("GeckoView strict DoH request to example.com must finish",
                    pageStopped.await(30, TimeUnit.SECONDS));
            assertTrue("GeckoView must load the external host in TRR_MODE_ONLY", loadSucceeded.get());
            assertNotNull(finalUri.get());
            assertTrue("Navigation must finish on the requested HTTPS host: " + finalUri.get(),
                    finalUri.get().startsWith("https://example.com/"));
        } finally {
            closeSession(session);
        }
    }

    private static GeckoSession openSession(GeckoRuntime runtime, GeckoSessionSettings settings) {
        AtomicReference<GeckoSession> result = new AtomicReference<>();
        InstrumentationRegistry.getInstrumentation().runOnMainSync(() -> {
            GeckoSession session = new GeckoSession(settings);
            session.open(runtime);
            result.set(session);
        });
        return result.get();
    }

    private static void navigate(GeckoSession session, String uri) throws Exception {
        CountDownLatch pageStopped = new CountDownLatch(1);
        AtomicReference<Boolean> success = new AtomicReference<>(false);
        InstrumentationRegistry.getInstrumentation().runOnMainSync(() -> {
            session.setProgressDelegate(new GeckoSession.ProgressDelegate() {
                @Override public void onPageStop(GeckoSession page, boolean didSucceed) {
                    success.set(didSucceed);
                    pageStopped.countDown();
                }
            });
            session.loadUri(uri);
        });
        assertTrue("GeckoView must finish the loopback page", pageStopped.await(15, TimeUnit.SECONDS));
        assertTrue("GeckoView loopback page must load successfully", success.get());
    }

    private static void closeSession(GeckoSession session) {
        InstrumentationRegistry.getInstrumentation().runOnMainSync(session::close);
    }

    private static final class HttpFixture implements AutoCloseable {
        private final ServerSocket server;
        private final BlockingQueue<Request> requests = new LinkedBlockingQueue<>();
        private final Thread worker;
        private volatile boolean closed;

        HttpFixture() throws Exception {
            server = new ServerSocket(0);
            worker = new Thread(this::serve, "gecko-pilot-http-fixture");
            worker.setDaemon(true);
            worker.start();
        }

        String url(String path) {
            return "http://127.0.0.1:" + server.getLocalPort() + path;
        }

        Request take(String path) throws Exception {
            long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(10);
            while (System.nanoTime() < deadline) {
                Request request = requests.poll(1, TimeUnit.SECONDS);
                if (request != null && request.path.equals(path)) return request;
            }
            fail("Timed out waiting for HTTP fixture request " + path);
            return null;
        }

        private void serve() {
            while (!closed) {
                try (Socket socket = server.accept()) {
                    BufferedReader input = new BufferedReader(new InputStreamReader(
                            socket.getInputStream(), StandardCharsets.US_ASCII));
                    String requestLine = input.readLine();
                    if (requestLine == null) continue;
                    String path = requestLine.split(" ")[1];
                    String cookie = null;
                    String header;
                    while ((header = input.readLine()) != null && !header.isEmpty()) {
                        if (header.regionMatches(true, 0, "Cookie:", 0, 7)) {
                            cookie = header.substring(7).trim();
                        }
                    }
                    requests.offer(new Request(path, cookie));
                    String body = "<!doctype html><title>Gecko pilot</title><p>ready</p>";
                    byte[] bodyBytes = body.getBytes(StandardCharsets.UTF_8);
                    OutputStream output = socket.getOutputStream();
                    String value = path.startsWith("/set?value=")
                            ? path.substring("/set?value=".length()) : null;
                    String response = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n"
                            + (value == null ? "" : "Set-Cookie: pip=" + value
                                    + "; Path=/; Max-Age=3600; SameSite=Lax\r\n")
                            + "Connection: close\r\nContent-Length: " + bodyBytes.length + "\r\n\r\n";
                    output.write(response.getBytes(StandardCharsets.US_ASCII));
                    output.write(bodyBytes);
                    output.flush();
                } catch (Exception error) {
                    if (!closed) requests.offer(new Request("fixture-error", error.toString()));
                }
            }
        }

        @Override public void close() throws Exception {
            closed = true;
            server.close();
            worker.join(1000);
        }

        private static final class Request {
            final String path;
            final String cookie;

            Request(String path, String cookie) {
                this.path = path;
                this.cookie = cookie;
            }
        }
    }
}
