package com.psharehub.pip;

import android.webkit.ServiceWorkerClient;
import android.webkit.ServiceWorkerController;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;

import java.io.ByteArrayInputStream;
import java.util.Collections;
import java.util.Locale;
import java.util.Map;

/** Request callbacks run off the UI thread; callers pass immutable URL snapshots only. */
final class AdblockRequests {
    private static final ServiceWorkerClient SERVICE_WORKER = new ServiceWorkerClient() {
        @Override public WebResourceResponse shouldInterceptRequest(WebResourceRequest request) {
            String source = header(request, "referer");
            // Android does not supply a ServiceWorker client/tab ID. A missing
            // Referer uses same-origin context conservatively, so third-party
            // rules may not match. Generic host/path rules still apply.
            if (!Address.isWebUrl(source)) source = request.getUrl().toString();
            return intercept(request, source, null);
        }
    };

    private AdblockRequests() {}

    static ServiceWorkerClient serviceWorkerClient() { return SERVICE_WORKER; }

    static void installServiceWorkerFiltering() {
        ServiceWorkerController controller = ServiceWorkerController.getInstance();
        controller.getServiceWorkerWebSettings().setAllowFileAccess(false);
        controller.getServiceWorkerWebSettings().setAllowContentAccess(false);
        controller.setServiceWorkerClient(SERVICE_WORKER);
    }

    static WebResourceResponse intercept(WebResourceRequest request, String topUrl, String explicitUrl) {
        String url = request.getUrl().toString();
        if (!Address.isWebUrl(url)) return null;
        // Do not block a URL the user explicitly typed or restored. Subresources
        // of that document still pass through the real network filter.
        if (request.isForMainFrame() && Address.sameNavigationUrl(url, explicitUrl)) return null;
        // shouldOverrideUrlLoading excludes POST. Enforce destination policy here too,
        // before generic network rules, so form submissions cannot bypass it.
        if (request.isForMainFrame() && NativeAdblock.checkNavigationHost(request.getUrl().getHost()) == 1) {
            return blockedResponse();
        }
        String source = Address.isWebUrl(topUrl) ? topUrl : url;
        if (NativeAdblock.check(url, source, resourceType(request), request.getMethod()) != 1) return null;
        return blockedResponse();
    }

    private static WebResourceResponse blockedResponse() {
        return new WebResourceResponse("text/plain", "UTF-8", 204, "No Content",
                Collections.singletonMap("Cache-Control", "no-store"),
                new ByteArrayInputStream(new byte[0]));
    }

    private static String resourceType(WebResourceRequest request) {
        if (request.isForMainFrame()) return "document";
        String destination = header(request, "sec-fetch-dest").toLowerCase(Locale.ROOT);
        switch (destination) {
            case "script": case "worker": case "sharedworker": case "serviceworker": return "script";
            case "style": return "stylesheet";
            case "image": return "image";
            case "font": return "font";
            case "audio": case "video": return "media";
            case "iframe": case "frame": return "subdocument";
            case "empty": return "xmlhttprequest";
            default: break;
        }
        String accept = header(request, "accept").toLowerCase(Locale.ROOT);
        if (accept.contains("text/css")) return "stylesheet";
        if (accept.startsWith("image/")) return "image";
        String path = request.getUrl().getPath();
        if (path != null) {
            path = path.toLowerCase(Locale.ROOT);
            if (path.endsWith(".js") || path.endsWith(".mjs")) return "script";
            if (path.endsWith(".css")) return "stylesheet";
            if (path.endsWith(".woff") || path.endsWith(".woff2")) return "font";
        }
        return "other";
    }

    private static String header(WebResourceRequest request, String name) {
        for (Map.Entry<String, String> entry : request.getRequestHeaders().entrySet()) {
            if (name.equalsIgnoreCase(entry.getKey())) return entry.getValue() == null ? "" : entry.getValue();
        }
        return "";
    }
}
