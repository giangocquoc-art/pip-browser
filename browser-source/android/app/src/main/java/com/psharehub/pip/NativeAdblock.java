package com.psharehub.pip;

import java.util.concurrent.CompletableFuture;

/** Thin JNI host. Filter parsing and library loading never run on the UI thread. */
final class NativeAdblock {
    private static volatile boolean ready;
    private static volatile boolean failed;

    private static final class Initialization {
        static final CompletableFuture<Boolean> RESULT = CompletableFuture.supplyAsync(() -> {
            try {
                System.loadLibrary("pip_adblock");
                ready = nativeInitialize();
                failed = !ready;
                return ready;
            } catch (LinkageError | RuntimeException error) {
                failed = true;
                return false;
            }
        });
    }

    private NativeAdblock() {}

    static CompletableFuture<Boolean> initialize() { return Initialization.RESULT; }
    static boolean isReady() { return ready; }
    static boolean hasFailed() { return failed; }

    static int check(String url, String source, String kind, String method) {
        if (!ready) return -1;
        // Bound Java UTF-16 input before JNI creates UTF-8 Rust allocations.
        if (url.length() > 16384 || source.length() > 16384 || kind.length() > 32 || method.length() > 16) return 0;
        try {
            int result = nativeCheck(url, source, kind, method);
            if (result < 0) { failed = true; ready = false; }
            return result;
        } catch (LinkageError | RuntimeException error) {
            failed = true;
            ready = false;
            return -1;
        }
    }

    static long blockedCount() {
        if (!ready) return 0;
        try { return nativeBlockedCount(); }
        catch (LinkageError error) { failed = true; ready = false; return 0; }
    }

    static String youtubeScript() {
        if (!ready) return "";
        try {
            String script = nativeYoutubeScript();
            return script == null ? "" : script;
        } catch (LinkageError | RuntimeException error) {
            return "";
        }
    }

    static int checkNavigationHost(String host) {
        if (!ready) return -1;
        if (host == null || host.length() > 254) return 0;
        try {
            int result = nativeCheckNavigationHost(host);
            if (result < 0) { failed = true; ready = false; }
            return result;
        } catch (LinkageError | RuntimeException error) {
            failed = true;
            ready = false;
            return -1;
        }
    }

    private static native boolean nativeInitialize();
    private static native int nativeCheck(String url, String source, String kind, String method);
    private static native long nativeBlockedCount();
    private static native int nativeCheckNavigationHost(String host);
    private static native String nativeYoutubeScript();
}
