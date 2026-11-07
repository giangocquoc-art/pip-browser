package com.psharehub.pip;

import android.annotation.SuppressLint;
import android.Manifest;
import android.app.Activity;
import android.app.AlertDialog;
import android.app.DownloadManager;
import android.content.pm.PackageManager;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.content.SharedPreferences;
import android.graphics.Color;
import android.net.Uri;
import android.net.http.SslError;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.os.Message;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputMethodManager;
import android.webkit.CookieManager;
import android.webkit.PermissionRequest;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.SslErrorHandler;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebBackForwardList;
import android.webkit.ValueCallback;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.webkit.URLUtil;
import android.widget.Button;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;

import org.json.JSONArray;
import org.json.JSONException;

import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Initial Android shell; platform WebView owns rendering and its security updates. */
public final class MainActivity extends Activity {
    private static final int MAX_TABS = 8;
    private static final int FILE_CHOOSER_REQUEST = 702;
    private static final int WEBSITE_PERMISSION_REQUEST = 703;
    private final ArrayList<Tab> tabs = new ArrayList<>();
    private BrowserDataStore dataStore;
    private ExecutorService storageExecutor;
    private SharedPreferences sessionPreferences;
    private int selected;
    private EditText address;
    private Button back;
    private Button forward;
    private Button reloadButton;
    private Button tabButton;
    private FrameLayout content;
    private TextView status;
    private TextView filteringStatus;
    private ProgressBar progress;
    private View browserRoot;
    private View fullscreenView;
    private WebChromeClient.CustomViewCallback fullscreenCallback;
    private boolean retryAvailable;
    private boolean paused;
    private boolean destroying;
    private boolean allowUnprotected;
    private ValueCallback<Uri[]> fileChooserCallback;
    private WebView fileChooserOwner;
    private PermissionRequest websitePermissionRequest;
    private WebView websitePermissionOwner;
    private String websitePermissionOrigin;
    private String[] websitePermissionResources;
    private AlertDialog websitePermissionDialog;

    private static final class Tab {
        String url;
        volatile String documentUrl;
        volatile String explicitNavigationUrl;
        String title = "";
        WebView view;
        String pendingLoadUrl;
        boolean loading;
        boolean suppressProgress;
        boolean youtubeScriptletsInstalled;
        Tab(String url) {
            this.url = url;
            this.documentUrl = url;
            this.explicitNavigationUrl = url;
        }
    }

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        dataStore = new BrowserDataStore(this);
        storageExecutor = Executors.newSingleThreadExecutor();
        sessionPreferences = getSharedPreferences("browser-session", MODE_PRIVATE);
        WebView.setWebContentsDebuggingEnabled(BuildConfig.DEBUG);
        CookieManager.getInstance().setAcceptCookie(true);
        buildUi();
        AdblockRequests.installServiceWorkerFiltering();
        NativeAdblock.initialize().whenComplete((ready, failure) -> runOnUiThread(() -> {
            if (!destroying) {
                updateFilteringStatus();
                if (NativeAdblock.isReady()) drainPendingLoads();
            }
        }));
        ArrayList<String> saved = state == null ? loadPersistedSession()
                : state.getStringArrayList("tabUrls");
        if (saved != null) {
            for (String url : saved) {
                if (tabs.size() < MAX_TABS && Address.isWebUrl(url)) tabs.add(new Tab(url));
            }
        }
        if (tabs.isEmpty()) tabs.add(new Tab(Address.HOME));
        int savedSelection = state == null ? sessionPreferences.getInt("selected", 0)
                : state.getInt("selected", 0);
        selected = Math.min(Math.max(0, savedSelection), tabs.size() - 1);
        selectTab(selected);
        if (Build.VERSION.SDK_INT >= 33) {
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                    android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT, this::navigateBack);
        }
    }

    private void buildUi() {
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(Color.rgb(255, 249, 243));
        root.setOnApplyWindowInsetsListener((view, insets) -> {
            if (Build.VERSION.SDK_INT >= 30) {
                android.graphics.Insets bars = insets.getInsets(
                        WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout()
                                | WindowInsets.Type.ime());
                view.setPadding(bars.left, bars.top, bars.right, bars.bottom);
            } else {
                view.setPadding(insets.getSystemWindowInsetLeft(), insets.getSystemWindowInsetTop(),
                        insets.getSystemWindowInsetRight(), insets.getSystemWindowInsetBottom());
            }
            return insets;
        });
        LinearLayout omnibox = new LinearLayout(this);
        omnibox.setGravity(Gravity.CENTER_VERTICAL);
        TextView brand = new TextView(this);
        brand.setText(R.string.app_name);
        brand.setTextColor(Color.rgb(249, 115, 22));
        brand.setTextSize(23);
        brand.setPadding(dp(12), 0, dp(8), 0);
        omnibox.addView(brand);
        address = new EditText(this);
        address.setSingleLine(true);
        address.setTextSize(16);
        address.setHint(R.string.address_hint);
        address.setInputType(android.text.InputType.TYPE_CLASS_TEXT
                | android.text.InputType.TYPE_TEXT_VARIATION_URI);
        address.setImeOptions(EditorInfo.IME_ACTION_GO);
        address.setSelectAllOnFocus(true);
        address.setOnEditorActionListener((v, action, event) -> {
            if (action == EditorInfo.IME_ACTION_GO) { openAddress(); return true; }
            return false;
        });
        omnibox.addView(address, new LinearLayout.LayoutParams(0, dp(56), 1));
        Button go = button(R.string.go, this::openAddress);
        omnibox.addView(go, new LinearLayout.LayoutParams(dp(64), dp(56)));
        root.addView(omnibox);

        LinearLayout controls = new LinearLayout(this);
        back = button(R.string.back, this::navigateBack);
        forward = button(R.string.forward, () -> {
            WebView view = activeView();
            if (view != null && view.canGoForward()) {
                Tab tab = tabs.get(selected);
                WebBackForwardList history = view.copyBackForwardList();
                int target = history.getCurrentIndex() + 1;
                if (target < history.getSize()) {
                    tab.explicitNavigationUrl = history.getItemAtIndex(target).getUrl();
                    tab.documentUrl = tab.explicitNavigationUrl;
                }
                view.goForward();
            }
        });
        reloadButton = button(R.string.reload, () -> {
            Tab tab = tabs.get(selected);
            if (tab.loading && tab.view != null) {
                tab.view.stopLoading();
                tab.loading = false;
                tab.suppressProgress = true;
                retryAvailable = false;
                status.setVisibility(View.GONE);
                progress.setVisibility(View.INVISIBLE);
                updateControls();
            } else if (tab.view == null) selectTab(selected);
            else if (tab.pendingLoadUrl != null || !NativeAdblock.isReady()) loadWhenProtected(tab);
            else tab.view.reload();
        });
        tabButton = button(R.string.tabs, this::showTabs);
        Button menu = button(R.string.menu, this::showBrowserMenu);
        progress = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal);
        progress.setMax(100);
        root.addView(progress, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(3)));
        status = new TextView(this);
        status.setTextSize(15);
        status.setPadding(dp(12), dp(6), dp(12), dp(6));
        status.setVisibility(View.GONE);
        status.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);
        status.setOnClickListener(v -> {
            if (!retryAvailable) return;
            retryAvailable = false;
            status.setVisibility(View.GONE);
            WebView view = activeView();
            if (view == null) selectTab(selected);
            else if (tabs.get(selected).pendingLoadUrl != null) loadWhenProtected(tabs.get(selected));
            else view.reload();
        });
        root.addView(status);
        filteringStatus = new TextView(this);
        filteringStatus.setTextSize(14);
        filteringStatus.setMinHeight(dp(48));
        filteringStatus.setGravity(Gravity.CENTER_VERTICAL | Gravity.START);
        TypedValue statusSelector = new TypedValue();
        if (getTheme().resolveAttribute(android.R.attr.selectableItemBackground, statusSelector, true)
                && statusSelector.resourceId != 0) {
            filteringStatus.setBackgroundResource(statusSelector.resourceId);
        }
        filteringStatus.setPadding(dp(12), dp(3), dp(12), dp(3));
        filteringStatus.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);
        filteringStatus.setClickable(true);
        filteringStatus.setFocusable(true);
        root.addView(filteringStatus);
        filteringStatus.setOnClickListener(v -> {
            if (NativeAdblock.hasFailed()) {
                new AlertDialog.Builder(this).setMessage(R.string.adblock_failure_prompt)
                        .setPositiveButton(R.string.continue_unprotected, (dialog, which) -> {
                            allowUnprotected = true;
                            drainPendingLoads();
                        }).setNegativeButton(R.string.cancel, null).show();
            } else if (NativeAdblock.isReady()) {
                long blocked = NativeAdblock.blockedCount();
                boolean youtubeScriptletsInstalled = selected >= 0 && selected < tabs.size()
                        && tabs.get(selected).youtubeScriptletsInstalled;
                new AlertDialog.Builder(this).setTitle(R.string.adblock_details_title)
                        .setMessage(getString(R.string.adblock_details, blocked,
                                getString(youtubeScriptletsInstalled
                                        ? R.string.adblock_scriptlet_ready
                                        : R.string.adblock_scriptlet_unavailable)))
                        .setPositiveButton(R.string.cancel, null).show();
            }
        });
        updateFilteringStatus();
        content = new FrameLayout(this);
        root.addView(content, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1));
        for (Button item : new Button[]{back, forward, reloadButton, tabButton, menu}) {
            item.setMinHeight(dp(48));
            controls.addView(item, new LinearLayout.LayoutParams(0,
                    ViewGroup.LayoutParams.WRAP_CONTENT, 1));
        }
        root.addView(controls);
        browserRoot = root;
        setContentView(root);
        root.requestApplyInsets();
    }

    private Button button(int text, Runnable action) {
        Button button = new Button(this);
        button.setText(text);
        button.setTextSize(13);
        button.setAllCaps(false);
        button.setPadding(0, 0, 0, 0);
        button.setContentDescription(getString(text));
        button.setMinHeight(dp(48));
        button.setOnClickListener(v -> action.run());
        return button;
    }

    private int dp(int size) { return Math.round(size * getResources().getDisplayMetrics().density); }

    private void openAddress() {
        String url = Address.resolve(address.getText().toString());
        if (url == null) { showStatus(R.string.blocked_address); return; }
        Tab tab = tabs.get(selected);
        cancelWebsitePermissionFor(tab.view);
        tab.url = url;
        tab.documentUrl = url;
        tab.explicitNavigationUrl = url;
        tab.loading = false;
        tab.suppressProgress = false;
        retryAvailable = false;
        status.setVisibility(View.GONE);
        progress.setVisibility(View.INVISIBLE);
        persistSession();
        if (tab.view == null) selectTab(selected);
        else loadWhenProtected(tab);
        address.clearFocus();
        ((InputMethodManager) getSystemService(INPUT_METHOD_SERVICE))
                .hideSoftInputFromWindow(address.getWindowToken(), 0);
    }

    private boolean addTab(String url, boolean load) {
        if (tabs.size() >= MAX_TABS) {
            Toast.makeText(this, R.string.tab_limit, Toast.LENGTH_SHORT).show();
            return false;
        }
        tabs.add(new Tab(url));
        persistSession();
        selectTab(tabs.size() - 1, load);
        return true;
    }

    private void selectTab(int index) { selectTab(index, true); }

    private void selectTab(int index, boolean load) {
        WebView previous = activeView();
        if (previous != null) cancelWebsitePermissionFor(previous);
        if (previous != null) previous.onPause();
        content.removeAllViews();
        selected = index;
        persistSession();
        Tab tab = tabs.get(selected);
        boolean created = tab.view == null;
        if (created) tab.view = createWebView(tab);
        content.addView(tab.view, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        if (!paused) tab.view.onResume();
        progress.setProgress(tab.view.getProgress());
        progress.setVisibility(tab.loading && tab.view.getProgress() < 100
                ? View.VISIBLE : View.INVISIBLE);
        status.setVisibility(View.GONE);
        retryAvailable = false;
        address.setText(tab.url);
        if (created && load) loadWhenProtected(tab);
        updateControls();
        updateFilteringStatus();
    }

    @SuppressLint("SetJavaScriptEnabled") // Browser pages need JS; no host JS interface is exposed.
    private WebView createWebView(Tab tab) {
        WebView view = new WebView(this);
        WebSettings settings = view.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
        settings.setSafeBrowsingEnabled(true);
        settings.setMediaPlaybackRequiresUserGesture(true);
        settings.setJavaScriptCanOpenWindowsAutomatically(false);
        settings.setSupportMultipleWindows(true);
        settings.setBuiltInZoomControls(true);
        settings.setDisplayZoomControls(false);
        CookieManager.getInstance().setAcceptThirdPartyCookies(view, false);
        view.setDownloadListener((url, agent, disposition, mime, length) -> {
            if (isCurrentView(tab, view)) enqueueDownload(url, disposition, mime);
        });
        view.setWebViewClient(new WebViewClient() {
            @Override public WebResourceResponse shouldInterceptRequest(
                    WebView webView, WebResourceRequest request) {
                // Android invokes this on a background thread. Never read the
                // Activity's tab collection or query the WebView from here.
                return AdblockRequests.intercept(request, tab.documentUrl, tab.explicitNavigationUrl);
            }

            @Override public boolean shouldOverrideUrlLoading(WebView webView, WebResourceRequest request) {
                if (!isLiveView(tab, webView)) return true;
                boolean allowed = Address.isWebUrl(request.getUrl().toString());
                if (allowed && request.isForMainFrame()
                        && !Address.sameNavigationUrl(request.getUrl().toString(), tab.explicitNavigationUrl)) {
                    int decision = NativeAdblock.checkNavigationHost(request.getUrl().getHost());
                    if (decision == 1) {
                        if (isCurrentView(tab, webView)) {
                            showStatus(R.string.blocked_gambling_navigation);
                            updateFilteringStatus();
                        }
                        return true;
                    }
                    if (decision < 0 && isCurrentView(tab, webView)) updateFilteringStatus();
                }
                if (allowed && request.isForMainFrame()) {
                    tab.documentUrl = request.getUrl().toString();
                }
                if (!allowed && request.isForMainFrame() && isCurrentView(tab, webView)) {
                    showStatus(R.string.blocked_address);
                }
                return !allowed;
            }

            @Override public void onPageStarted(WebView webView, String url, android.graphics.Bitmap icon) {
                if (!isLiveView(tab, webView)) return;
                cancelWebsitePermissionFor(webView);
                if (Address.isWebUrl(url)) {
                    // A load requested just before this callback may supersede a queued
                    // start from the previous page. Keep that stale callback from
                    // replacing the tab URL that session restore will persist.
                    if (!Address.sameNavigationUrl(url, tab.documentUrl)
                            && !Address.sameNavigationUrl(url, tab.explicitNavigationUrl)) return;
                    tab.url = url;
                    tab.documentUrl = url;
                    persistSession();
                }
                if (isCurrentView(tab, webView)) {
                    tab.loading = true;
                    tab.suppressProgress = false;
                    retryAvailable = false;
                    status.setVisibility(View.GONE);
                    if (!address.hasFocus()) address.setText(tab.url);
                    updateControls();
                    updateFilteringStatus();
                }
            }

            @Override public void onPageFinished(WebView webView, String url) {
                if (!isLiveView(tab, webView)) return;
                if (!Address.sameNavigationUrl(url, tab.documentUrl)
                        && !Address.sameNavigationUrl(url, tab.explicitNavigationUrl)) return;
                tab.loading = false;
                tab.suppressProgress = false;
                String visitedUrl = Address.isWebUrl(url) ? url : tab.url;
                String visitedTitle = tab.title;
                storageExecutor.execute(() -> dataStore.recordVisit(visitedUrl, visitedTitle));
                if (isCurrentView(tab, webView)) {
                    updateControls();
                    updateFilteringStatus();
                }
            }

            @Override public void onReceivedError(WebView webView, WebResourceRequest request, WebResourceError error) {
                if (request.isForMainFrame() && isCurrentView(tab, webView)
                        && (Address.sameNavigationUrl(request.getUrl().toString(), tab.documentUrl)
                        || Address.sameNavigationUrl(request.getUrl().toString(), tab.explicitNavigationUrl))) {
                    tab.loading = false;
                    tab.suppressProgress = true;
                    retryAvailable = true;
                    showMainFrameError(request.getUrl().toString(), error.getErrorCode());
                    updateControls();
                }
            }

            @Override public void onReceivedSslError(WebView webView, SslErrorHandler handler, SslError error) {
                handler.cancel();
                if (isCurrentView(tab, webView)
                        && (Address.sameNavigationUrl(error.getUrl(), tab.documentUrl)
                        || Address.sameNavigationUrl(error.getUrl(), tab.explicitNavigationUrl))) {
                    tab.loading = false;
                    tab.suppressProgress = true;
                    retryAvailable = false;
                    showStatus(R.string.ssl_error);
                    updateControls();
                }
            }

            @Override public boolean onRenderProcessGone(WebView webView, RenderProcessGoneDetail detail) {
                if (!isLiveView(tab, webView)) return true;
                cancelFileChooserFor(webView);
                cancelWebsitePermissionFor(webView);
                boolean wasCurrent = isCurrentView(tab, webView);
                tab.view = null;
                tab.youtubeScriptletsInstalled = false;
                if (webView.getParent() instanceof ViewGroup) ((ViewGroup) webView.getParent()).removeView(webView);
                webView.destroy();
                if (wasCurrent) {
                    tab.loading = false;
                    retryAvailable = true;
                    showStatus(R.string.render_error);
                    updateControls();
                    updateFilteringStatus();
                }
                return true;
            }
        });
        view.setWebChromeClient(new WebChromeClient() {
            @Override public void onProgressChanged(WebView webView, int value) {
                if (isCurrentView(tab, webView)) {
                    if (tab.suppressProgress) return;
                    progress.setProgress(value);
                    progress.setVisibility(value == 100 ? View.INVISIBLE : View.VISIBLE);
                    if (value > 0 && value < 100) tab.loading = true;
                    updateControls();
                }
            }

            @Override public void onReceivedTitle(WebView webView, String title) {
                if (isLiveView(tab, webView)) tab.title = title == null ? "" : title;
            }

            @Override public void onShowCustomView(View customView, CustomViewCallback callback) {
                if (!isCurrentView(tab, view) || fullscreenView != null) {
                    callback.onCustomViewHidden();
                    return;
                }
                fullscreenView = customView;
                fullscreenCallback = callback;
                setContentView(customView);
                if (Build.VERSION.SDK_INT >= 19) customView.setSystemUiVisibility(
                        View.SYSTEM_UI_FLAG_LAYOUT_STABLE | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                                | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                                | View.SYSTEM_UI_FLAG_FULLSCREEN | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY);
            }

            @Override public void onHideCustomView() { hideFullscreenView(); }

            @Override public void onPermissionRequest(PermissionRequest request) {
                requestWebsiteMediaPermission(view, request);
            }

            @Override public void onPermissionRequestCanceled(PermissionRequest request) {
                if (websitePermissionRequest == request) clearPendingWebsitePermission();
            }

            @Override public boolean onShowFileChooser(WebView webView,
                    ValueCallback<Uri[]> callback, FileChooserParams params) {
                if (!isCurrentView(tab, webView)) {
                    callback.onReceiveValue(null);
                    return false;
                }
                cancelPendingFileChooser();
                try {
                    // Use the system document picker, never a page-requested camera capture intent.
                    Intent chooser = createDocumentPickerIntent(params.getMode(), params.getAcceptTypes());
                    fileChooserCallback = callback;
                    fileChooserOwner = webView;
                    startActivityForResult(chooser, FILE_CHOOSER_REQUEST);
                    return true;
                } catch (ActivityNotFoundException | SecurityException | IllegalArgumentException failure) {
                    cancelPendingFileChooser();
                    Toast.makeText(MainActivity.this, R.string.file_picker_unavailable,
                            Toast.LENGTH_LONG).show();
                    return false;
                }
            }

            @Override public boolean onCreateWindow(WebView webView, boolean dialog, boolean userGesture, Message result) {
                if (!isCurrentView(tab, webView) || !userGesture || !addTab(Address.HOME, false)) return false;
                ((WebView.WebViewTransport) result.obj).setWebView(activeView());
                result.sendToTarget();
                return true;
            }
        });
        return view;
    }

    private void enqueueDownload(String rawUrl, String disposition, String mime) {
        if (!DownloadSupport.isAllowedUrl(rawUrl)) {
            Toast.makeText(this, R.string.download_blocked, Toast.LENGTH_LONG).show();
            return;
        }
        String filename = DownloadSupport.safeFilename(URLUtil.guessFileName(rawUrl, disposition, mime));
        if (filename.isEmpty()) filename = "pip-download";
        final String confirmedFilename = filename;
        String host = Uri.parse(rawUrl).getHost();
        new AlertDialog.Builder(this)
                .setTitle(R.string.download_confirm_title)
                .setMessage(getString(R.string.download_confirm_message, confirmedFilename, host))
                .setNegativeButton(R.string.cancel, null)
                .setPositiveButton(R.string.download_save, (dialog, which) ->
                        startDownload(rawUrl, mime, confirmedFilename))
                .show();
    }

    static Intent createDocumentPickerIntent(int mode) {
        return createDocumentPickerIntent(mode, null);
    }

    static Intent createDocumentPickerIntent(int mode, String[] requestedTypes) {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("*/*");
        String[] acceptedTypes = normalizedAcceptTypes(requestedTypes);
        if (acceptedTypes.length > 0) {
            intent.putExtra(Intent.EXTRA_MIME_TYPES, acceptedTypes);
        }
        if (mode == WebChromeClient.FileChooserParams.MODE_OPEN_MULTIPLE) {
            intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
        }
        return intent;
    }

    static String[] normalizedAcceptTypes(String[] requestedTypes) {
        if (requestedTypes == null || requestedTypes.length == 0) return new String[0];
        java.util.LinkedHashSet<String> accepted = new java.util.LinkedHashSet<>();
        for (String value : requestedTypes) {
            if (value == null) continue;
            String type = value.trim().toLowerCase(java.util.Locale.ROOT);
            int parameter = type.indexOf(';');
            if (parameter >= 0) type = type.substring(0, parameter).trim();
            int slash = type.indexOf('/');
            if (slash <= 0 || slash != type.lastIndexOf('/') || slash == type.length() - 1) continue;
            String major = type.substring(0, slash);
            String minor = type.substring(slash + 1);
            if (!isMimeToken(major) || !isMimeToken(minor)
                    || "*".equals(major) && !"*".equals(minor)) continue;
            accepted.add(type);
        }
        return accepted.toArray(new String[0]);
    }

    private static boolean isMimeToken(String value) {
        if ("*".equals(value)) return true;
        if (value.isEmpty()) return false;
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            boolean valid = c >= 'a' && c <= 'z' || c >= '0' && c <= '9'
                    || c == '!' || c == '#' || c == '$' || c == '&' || c == '^'
                    || c == '_' || c == '.' || c == '+' || c == '-';
            if (!valid) return false;
        }
        return true;
    }

    private void startDownload(String rawUrl, String mime, String filename) {
        try {
            Uri uri = Uri.parse(rawUrl);
            DownloadManager manager = getSystemService(DownloadManager.class);
            if (manager == null) throw new IllegalStateException("Download service unavailable");
            DownloadManager.Request request = new DownloadManager.Request(uri)
                    .setTitle(filename)
                    .setDescription(getString(R.string.download_in_progress))
                    .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
                    .setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, filename)
                    .setMimeType(mime == null || mime.trim().isEmpty()
                            ? "application/octet-stream" : mime);
            manager.enqueue(request);
            Toast.makeText(this, getString(R.string.download_started, filename), Toast.LENGTH_LONG).show();
        } catch (RuntimeException failure) {
            Toast.makeText(this, R.string.download_failed, Toast.LENGTH_LONG).show();
        }
    }

    private void showTabs() {
        final AlertDialog[] dialogRef = new AlertDialog[1];
        LinearLayout panel = new LinearLayout(this);
        panel.setOrientation(LinearLayout.VERTICAL);
        panel.setPadding(dp(16), dp(8), dp(16), dp(8));
        ScrollView scroll = new ScrollView(this);
        LinearLayout rows = new LinearLayout(this);
        rows.setOrientation(LinearLayout.VERTICAL);
        for (int i = 0; i < tabs.size(); i++) {
            final int index = i;
            Tab tab = tabs.get(i);
            LinearLayout row = new LinearLayout(this);
            row.setGravity(Gravity.CENTER_VERTICAL);
            TextView label = new TextView(this);
            String title = tab.title.isEmpty() ? tab.url : tab.title;
            label.setText((i == selected ? "●  " : "") + title + "\n" + tab.url);
            label.setTextSize(14);
            label.setMaxLines(2);
            label.setEllipsize(android.text.TextUtils.TruncateAt.END);
            label.setPadding(dp(8), dp(8), dp(8), dp(8));
            label.setMinHeight(dp(56));
            label.setGravity(Gravity.CENTER_VERTICAL);
            label.setSelected(i == selected);
            label.setContentDescription(getString(R.string.select_tab, i + 1, title));
            label.setOnClickListener(v -> {
                selectTab(index);
                if (dialogRef[0] != null) dialogRef[0].dismiss();
            });
            row.addView(label, new LinearLayout.LayoutParams(0,
                    ViewGroup.LayoutParams.WRAP_CONTENT, 1));
            Button close = button(R.string.close_tab, () -> {
                closeTabAt(index);
                if (dialogRef[0] != null) dialogRef[0].dismiss();
                showTabs();
            });
            close.setContentDescription(getString(i == selected
                    ? R.string.close_selected_tab_number : R.string.close_tab_number, index + 1));
            close.setText("×");
            close.setTextSize(20);
            row.addView(close, new LinearLayout.LayoutParams(dp(56), dp(52)));
            rows.addView(row);
            View divider = new View(this);
            divider.setBackgroundColor(Color.rgb(237, 224, 211));
            rows.addView(divider, new LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT, dp(1)));
        }
        scroll.addView(rows);
        panel.addView(scroll, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1));
        Button create = button(R.string.new_tab, () -> {
            if (dialogRef[0] != null) dialogRef[0].dismiss();
            addTab(Address.HOME, true);
        });
        panel.addView(create, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(52)));
        AlertDialog dialog = new AlertDialog.Builder(this)
                .setTitle(getString(R.string.tabs) + " (" + tabs.size() + ")")
                .setView(panel).setNegativeButton(R.string.cancel, null).create();
        dialogRef[0] = dialog;
        dialog.show();
        if (dialog.getWindow() != null) dialog.getWindow().setLayout(
                ViewGroup.LayoutParams.MATCH_PARENT,
                Math.max(dp(240), Math.min(getResources().getDisplayMetrics().heightPixels - dp(96), dp(560))));
    }

    private void showBrowserMenu() {
        Tab tab = tabs.get(selected);
        String url = tab.url;
        storageExecutor.execute(() -> {
            boolean bookmarked = dataStore.isBookmarked(url);
            runOnUiThread(() -> {
                if (destroying) return;
                String bookmarkAction = getString(bookmarked
                        ? R.string.remove_bookmark : R.string.add_bookmark);
                String[] items = {bookmarkAction, getString(R.string.bookmarks),
                        getString(R.string.history), getString(R.string.find_in_page),
                        getString(R.string.share_page), getString(R.string.downloads)};
                new AlertDialog.Builder(this).setTitle(R.string.menu).setItems(items, (dialog, which) -> {
                    if (which == 0) toggleBookmark();
                    else if (which == 1) showBookmarks();
                    else if (which == 2) showHistory();
                    else if (which == 3) showFindInPage();
                    else if (which == 4) shareCurrentPage();
                    else showDownloads();
                }).setNegativeButton(R.string.cancel, null).show();
            });
        });
    }

    private void shareCurrentPage() {
        String url = tabs.get(selected).url;
        if (!Address.isWebUrl(url)) return;
        Intent share = new Intent(Intent.ACTION_SEND);
        share.setType("text/plain");
        share.putExtra(Intent.EXTRA_TEXT, url);
        try {
            startActivity(Intent.createChooser(share, getString(R.string.share_page)));
        } catch (ActivityNotFoundException ignored) {
            Toast.makeText(this, R.string.share_unavailable, Toast.LENGTH_SHORT).show();
        }
    }

    private void showDownloads() {
        Intent downloads = new Intent(DownloadManager.ACTION_VIEW_DOWNLOADS);
        try {
            startActivity(downloads);
        } catch (ActivityNotFoundException ignored) {
            Toast.makeText(this, R.string.downloads_unavailable, Toast.LENGTH_SHORT).show();
        }
    }

    private void showFindInPage() {
        WebView view = activeView();
        if (view == null) return;
        EditText query = new EditText(this);
        query.setSingleLine(true);
        query.setHint(R.string.find_hint);
        query.setImeOptions(EditorInfo.IME_ACTION_SEARCH);
        LinearLayout panel = new LinearLayout(this);
        panel.setGravity(Gravity.CENTER_VERTICAL);
        panel.setPadding(dp(16), 0, dp(16), 0);
        panel.addView(query, new LinearLayout.LayoutParams(0, dp(56), 1));
        TextView count = new TextView(this);
        count.setPadding(dp(8), 0, dp(8), 0);
        panel.addView(count);
        AlertDialog dialog = new AlertDialog.Builder(this).setTitle(R.string.find_in_page)
                .setView(panel).setNegativeButton(R.string.cancel, (d, which) -> view.clearMatches())
                .create();
        view.setFindListener((activeMatchOrdinal, numberOfMatches, isDoneCounting) -> {
            if (isDoneCounting && numberOfMatches > 0) {
                count.setText(getString(R.string.find_count, activeMatchOrdinal + 1, numberOfMatches));
            } else if (isDoneCounting) count.setText(R.string.find_no_matches);
        });
        query.addTextChangedListener(new android.text.TextWatcher() {
            @Override public void beforeTextChanged(CharSequence s, int start, int count, int after) { }
            @Override public void onTextChanged(CharSequence s, int start, int before, int count) {
                if (isCurrentView(view)) view.findAllAsync(s.toString());
            }
            @Override public void afterTextChanged(android.text.Editable s) { }
        });
        query.setOnEditorActionListener((v, action, event) -> {
            if (action == EditorInfo.IME_ACTION_SEARCH) { view.findNext(true); return true; }
            return false;
        });
        dialog.setOnDismissListener(d -> view.setFindListener(null));
        dialog.show();
        query.requestFocus();
        query.postDelayed(() -> ((InputMethodManager) getSystemService(INPUT_METHOD_SERVICE))
                .showSoftInput(query, InputMethodManager.SHOW_IMPLICIT), 120);
    }

    private void toggleBookmark() {
        Tab tab = tabs.get(selected);
        String url = tab.url;
        String title = tab.title;
        if (!Address.isWebUrl(url)) return;
        storageExecutor.execute(() -> {
            boolean wasBookmarked = dataStore.isBookmarked(url);
            if (wasBookmarked) dataStore.removeBookmark(url);
            else dataStore.addBookmark(url, title);
            runOnUiThread(() -> {
                if (!destroying) Toast.makeText(this,
                        wasBookmarked ? R.string.bookmark_removed : R.string.bookmark_added,
                        Toast.LENGTH_SHORT).show();
            });
        });
    }

    private void showBookmarks() {
        storageExecutor.execute(() -> {
            List<BrowserDataStore.Entry> entries = dataStore.getBookmarks();
            runOnUiThread(() -> {
                if (destroying) return;
                if (entries.isEmpty()) {
                    new AlertDialog.Builder(this).setTitle(R.string.bookmarks)
                            .setMessage(R.string.empty_bookmarks).setPositiveButton(R.string.cancel, null).show();
                    return;
                }
                String[] labels = new String[entries.size()];
                for (int i = 0; i < entries.size(); i++) labels[i] = entries.get(i).label();
                new AlertDialog.Builder(this).setTitle(R.string.bookmarks).setItems(labels,
                        (dialog, index) -> openSavedUrl(entries.get(index).url))
                        .setPositiveButton(R.string.new_tab, (dialog, which) -> addTab(Address.HOME, true))
                        .setNegativeButton(R.string.cancel, null).show();
            });
        });
    }

    private void showHistory() {
        storageExecutor.execute(() -> {
            List<BrowserDataStore.Entry> entries = dataStore.getRecentHistory();
            runOnUiThread(() -> {
                if (destroying) return;
                if (entries.isEmpty()) {
                    new AlertDialog.Builder(this).setTitle(R.string.history)
                            .setMessage(R.string.empty_history).setPositiveButton(R.string.cancel, null).show();
                    return;
                }
                String[] labels = new String[entries.size()];
                for (int i = 0; i < entries.size(); i++) labels[i] = entries.get(i).label();
                new AlertDialog.Builder(this).setTitle(R.string.history).setItems(labels,
                        (dialog, index) -> openSavedUrl(entries.get(index).url))
                        .setPositiveButton(R.string.clear_history, (dialog, which) -> confirmClearHistory())
                        .setNegativeButton(R.string.cancel, null).show();
            });
        });
    }

    private void confirmClearHistory() {
        new AlertDialog.Builder(this).setMessage(R.string.confirm_clear_history)
                .setPositiveButton(R.string.clear_history, (dialog, which) -> storageExecutor.execute(() -> {
                    dataStore.clearHistory();
                    runOnUiThread(() -> {
                        if (!destroying) Toast.makeText(this, R.string.history_cleared, Toast.LENGTH_SHORT).show();
                    });
                })).setNegativeButton(R.string.cancel, null).show();
    }

    private void openSavedUrl(String url) {
        if (!Address.isWebUrl(url)) return;
        Tab tab = tabs.get(selected);
        tab.url = url;
        tab.documentUrl = url;
        tab.explicitNavigationUrl = url;
        tab.loading = false;
        tab.suppressProgress = false;
        retryAvailable = false;
        status.setVisibility(View.GONE);
        progress.setVisibility(View.INVISIBLE);
        address.setText(url);
        persistSession();
        if (tab.view == null) selectTab(selected);
        else loadWhenProtected(tab);
    }

    private void closeTabAt(int index) {
        if (index < 0 || index >= tabs.size()) return;
        int previousSelection = selected;
        Tab removed = tabs.remove(index);
        WebView removedView = removed.view;
        removed.view = null;
        if (tabs.isEmpty()) tabs.add(new Tab(Address.HOME));
        if (index < previousSelection) selected = previousSelection - 1;
        else if (index == previousSelection) selected = Math.min(index, tabs.size() - 1);
        else selected = previousSelection;
        persistSession();
        // Repair selection and revoke the old view identity before WebView teardown
        // can deliver queued navigation/progress/renderer callbacks.
        if (removedView != null) {
            cancelFileChooserFor(removedView);
            cancelWebsitePermissionFor(removedView);
            content.removeView(removedView);
            removedView.stopLoading();
            removedView.destroy();
        }
        selectTab(selected);
    }

    private boolean isLiveView(Tab tab, WebView view) {
        return !destroying && view != null && tab.view == view && tabs.contains(tab);
    }

    private boolean isCurrentView(Tab tab, WebView view) {
        return isLiveView(tab, view) && selected >= 0 && selected < tabs.size()
                && tabs.get(selected) == tab;
    }

    private boolean isCurrentView(WebView view) {
        return !destroying && view != null && activeView() == view;
    }

    private boolean isLiveView(WebView view) {
        if (destroying || view == null) return false;
        for (Tab tab : tabs) if (tab.view == view) return true;
        return false;
    }

    private void cancelPendingFileChooser() {
        ValueCallback<Uri[]> callback = fileChooserCallback;
        fileChooserCallback = null;
        fileChooserOwner = null;
        if (callback != null) callback.onReceiveValue(null);
    }

    private void cancelFileChooserFor(WebView view) {
        if (fileChooserOwner == view) cancelPendingFileChooser();
    }

    private void requestWebsiteMediaPermission(WebView owner, PermissionRequest request) {
        if (!isCurrentView(owner) || request == null
                || !isSecureMediaOrigin(request.getOrigin().toString())) {
            if (request != null) request.deny();
            return;
        }
        String[] resources = request.getResources();
        String[] androidPermissions = androidPermissionsFor(resources);
        if (androidPermissions.length == 0 || resources == null
                || androidPermissions.length != resources.length) {
            request.deny();
            return;
        }
        cancelPendingWebsitePermission();
        websitePermissionRequest = request;
        websitePermissionOwner = owner;
        websitePermissionOrigin = request.getOrigin().toString();
        websitePermissionResources = resources.clone();

        String media = mediaLabel(resources);
        websitePermissionDialog = new AlertDialog.Builder(this)
                .setTitle(R.string.website_permission_title)
                .setMessage(getString(R.string.website_media_permission_prompt,
                        request.getOrigin().getHost(), media))
                .setNegativeButton(R.string.permission_deny,
                        (dialog, which) -> cancelPendingWebsitePermission())
                .setPositiveButton(R.string.permission_allow,
                        (dialog, which) -> continueWebsiteMediaPermission(androidPermissions))
                .setOnCancelListener(dialog -> cancelPendingWebsitePermission())
                .create();
        websitePermissionDialog.show();
    }

    private void continueWebsiteMediaPermission(String[] androidPermissions) {
        if (!hasLivePendingWebsitePermission()) {
            cancelPendingWebsitePermission();
            return;
        }
        ArrayList<String> missing = new ArrayList<>();
        for (String permission : androidPermissions) {
            if (checkSelfPermission(permission) != PackageManager.PERMISSION_GRANTED) missing.add(permission);
        }
        if (missing.isEmpty()) {
            grantPendingWebsiteMediaPermission();
        } else {
            requestPermissions(missing.toArray(new String[0]), WEBSITE_PERMISSION_REQUEST);
        }
    }

    @Override public void onRequestPermissionsResult(int requestCode, String[] permissions, int[] grantResults) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        if (requestCode != WEBSITE_PERMISSION_REQUEST) return;
        if (hasLivePendingWebsitePermission()) grantPendingWebsiteMediaPermission();
        else cancelPendingWebsitePermission();
    }

    private void grantPendingWebsiteMediaPermission() {
        PermissionRequest request = websitePermissionRequest;
        String[] resources = websitePermissionResources;
        WebView owner = websitePermissionOwner;
        String origin = websitePermissionOrigin;
        clearPendingWebsitePermission();
        if (request == null || resources == null || !isCurrentView(owner)
                || !isSecureMediaOrigin(origin) || !sameOrigin(origin, owner.getUrl())) {
            if (request != null) request.deny();
            return;
        }
        ArrayList<String> granted = new ArrayList<>();
        for (String resource : resources) {
            String permission = androidPermissionForResource(resource);
            if (permission != null && checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED) {
                granted.add(resource);
            }
        }
        if (granted.isEmpty()) request.deny();
        else request.grant(granted.toArray(new String[0]));
    }

    private boolean hasLivePendingWebsitePermission() {
        return websitePermissionRequest != null && isCurrentView(websitePermissionOwner)
                && isSecureMediaOrigin(websitePermissionOrigin)
                && sameOrigin(websitePermissionOrigin, websitePermissionOwner.getUrl());
    }

    private void cancelPendingWebsitePermission() {
        PermissionRequest request = websitePermissionRequest;
        clearPendingWebsitePermission();
        if (request != null) request.deny();
    }

    private void cancelWebsitePermissionFor(WebView view) {
        if (websitePermissionOwner == view) cancelPendingWebsitePermission();
    }

    private void clearPendingWebsitePermission() {
        websitePermissionRequest = null;
        websitePermissionOwner = null;
        websitePermissionOrigin = null;
        websitePermissionResources = null;
        AlertDialog dialog = websitePermissionDialog;
        websitePermissionDialog = null;
        if (dialog != null && dialog.isShowing()) dialog.dismiss();
    }

    static String[] androidPermissionsFor(String[] resources) {
        if (resources == null || resources.length == 0) return new String[0];
        LinkedHashSet<String> permissions = new LinkedHashSet<>();
        for (String resource : resources) {
            String permission = androidPermissionForResource(resource);
            if (permission == null) return new String[0];
            permissions.add(permission);
        }
        return permissions.toArray(new String[0]);
    }

    private static String androidPermissionForResource(String resource) {
        if (PermissionRequest.RESOURCE_VIDEO_CAPTURE.equals(resource)) return Manifest.permission.CAMERA;
        if (PermissionRequest.RESOURCE_AUDIO_CAPTURE.equals(resource)) return Manifest.permission.RECORD_AUDIO;
        return null;
    }

    static boolean isSecureMediaOrigin(String rawOrigin) {
        if (rawOrigin == null) return false;
        Uri uri = Uri.parse(rawOrigin);
        String scheme = uri.getScheme();
        String host = uri.getHost();
        if (host == null) return false;
        if ("https".equalsIgnoreCase(scheme)) return true;
        if (!"http".equalsIgnoreCase(scheme)) return false;
        String normalizedHost = host.toLowerCase(Locale.ROOT);
        return "localhost".equals(normalizedHost) || "127.0.0.1".equals(normalizedHost)
                || "::1".equals(normalizedHost);
    }

    private static boolean sameOrigin(String first, String second) {
        if (first == null || second == null) return false;
        Uri a = Uri.parse(first);
        Uri b = Uri.parse(second);
        return a.getScheme() != null && a.getHost() != null
                && a.getScheme().equalsIgnoreCase(b.getScheme())
                && a.getHost().equalsIgnoreCase(b.getHost()) && a.getPort() == b.getPort();
    }

    private String mediaLabel(String[] resources) {
        boolean camera = false;
        boolean microphone = false;
        for (String resource : resources) {
            camera |= PermissionRequest.RESOURCE_VIDEO_CAPTURE.equals(resource);
            microphone |= PermissionRequest.RESOURCE_AUDIO_CAPTURE.equals(resource);
        }
        if (camera && microphone) return getString(R.string.website_media_camera_microphone);
        return getString(camera ? R.string.website_media_camera : R.string.website_media_microphone);
    }

    @Override protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != FILE_CHOOSER_REQUEST) return;
        ValueCallback<Uri[]> callback = fileChooserCallback;
        WebView owner = fileChooserOwner;
        fileChooserCallback = null;
        fileChooserOwner = null;
        if (callback == null) return;
        Uri[] selectedFiles = isLiveView(owner) ? parseDocumentPickerResult(resultCode, data) : null;
        callback.onReceiveValue(selectedFiles);
    }

    static Uri[] parseDocumentPickerResult(int resultCode, Intent data) {
        return resultCode == RESULT_OK && data != null
                ? WebChromeClient.FileChooserParams.parseResult(resultCode, data) : null;
    }

    private WebView activeView() {
        return destroying || selected < 0 || selected >= tabs.size() ? null : tabs.get(selected).view;
    }

    private void updateControls() {
        WebView view = activeView();
        back.setEnabled(view != null && view.canGoBack());
        forward.setEnabled(view != null && view.canGoForward());
        tabButton.setText(getString(R.string.tabs_count, tabs.size()));
        if (reloadButton != null) {
            reloadButton.setText(tabs.get(selected).loading ? R.string.stop : R.string.reload);
            reloadButton.setContentDescription(getString(tabs.get(selected).loading
                    ? R.string.stop : R.string.reload));
        }
    }

    private void showStatus(int message) {
        status.setText(message);
        status.setClickable(retryAvailable);
        status.setFocusable(retryAvailable);
        status.setContentDescription(getString(message) + (retryAvailable
                ? " " + getString(R.string.tap_to_retry) : ""));
        status.setVisibility(View.VISIBLE);
        progress.setVisibility(View.INVISIBLE);
    }

    private void showMainFrameError(String failedUrl, int errorCode) {
        String reason;
        if (errorCode == WebViewClient.ERROR_HOST_LOOKUP) reason = getString(R.string.network_host_error);
        else if (errorCode == WebViewClient.ERROR_CONNECT) reason = getString(R.string.network_connect_error);
        else if (errorCode == WebViewClient.ERROR_TIMEOUT) reason = getString(R.string.network_timeout_error);
        else if (errorCode == WebViewClient.ERROR_IO) reason = getString(R.string.network_io_error);
        else reason = getString(R.string.network_generic_error);
        String message = getString(R.string.load_error_detail, failedUrl, reason);
        status.setText(message);
        status.setClickable(true);
        status.setFocusable(true);
        status.setContentDescription(message);
        status.setVisibility(View.VISIBLE);
        progress.setVisibility(View.INVISIBLE);
    }

    private void updateFilteringStatus() {
        if (filteringStatus == null) return;
        if (NativeAdblock.hasFailed()) {
            filteringStatus.setText(R.string.adblock_unavailable);
            filteringStatus.setContentDescription(getString(R.string.adblock_unavailable));
        }
        else if (NativeAdblock.isReady()) {
            boolean youtubeScriptletsInstalled = selected >= 0 && selected < tabs.size()
                    && tabs.get(selected).youtubeScriptletsInstalled;
            filteringStatus.setText(R.string.adblock_ready);
            filteringStatus.setContentDescription(getString(R.string.adblock_ready)
                    + ". " + getString(R.string.tap_for_details) + ". "
                    + getString(youtubeScriptletsInstalled
                            ? R.string.adblock_scriptlet_ready
                            : R.string.adblock_scriptlet_unavailable));
        }
        else {
            filteringStatus.setText(R.string.adblock_loading);
            filteringStatus.setContentDescription(getString(R.string.adblock_loading));
        }
    }

    // UI-thread queue stores only the latest requested URL per live tab. No UI waits.
    private void loadWhenProtected(Tab tab) {
        tab.pendingLoadUrl = tab.url;
        if (destroying || tab.view == null) return;
        if (!NativeAdblock.isReady() && !allowUnprotected) {
            tab.view.stopLoading();
            updateFilteringStatus();
            return;
        }
        if (!tab.youtubeScriptletsInstalled && NativeAdblock.isReady()) {
            tab.youtubeScriptletsInstalled = YoutubeScriptletInstaller.install(tab.view);
            if (isCurrentView(tab, tab.view)) updateFilteringStatus();
        }
        String url = tab.pendingLoadUrl;
        tab.pendingLoadUrl = null;
        tab.view.loadUrl(url);
    }

    private void drainPendingLoads() {
        for (Tab tab : tabs) if (tab.pendingLoadUrl != null) {
            tab.url = tab.pendingLoadUrl;
            loadWhenProtected(tab);
        }
    }

    private void navigateBack() {
        if (fullscreenView != null) { hideFullscreenView(); return; }
        if (address != null && address.hasFocus()) {
            address.clearFocus();
            ((InputMethodManager) getSystemService(INPUT_METHOD_SERVICE))
                    .hideSoftInputFromWindow(address.getWindowToken(), 0);
            return;
        }
        WebView view = activeView();
        if (view != null && view.canGoBack()) {
            Tab tab = tabs.get(selected);
            WebBackForwardList history = view.copyBackForwardList();
            int target = history.getCurrentIndex() - 1;
            if (target >= 0) {
                tab.explicitNavigationUrl = history.getItemAtIndex(target).getUrl();
                tab.documentUrl = tab.explicitNavigationUrl;
            }
            view.goBack();
        }
        else finish();
    }

    private void hideFullscreenView() {
        View view = fullscreenView;
        WebChromeClient.CustomViewCallback callback = fullscreenCallback;
        fullscreenView = null;
        fullscreenCallback = null;
        if (view != null) {
            view.setSystemUiVisibility(View.SYSTEM_UI_FLAG_VISIBLE);
            setContentView(browserRoot);
        }
        if (callback != null) callback.onCustomViewHidden();
    }

    private ArrayList<String> loadPersistedSession() {
        ArrayList<String> urls = new ArrayList<>();
        String encoded = sessionPreferences.getString("tabUrls", null);
        if (encoded == null) return urls;
        try {
            JSONArray array = new JSONArray(encoded);
            for (int i = 0; i < array.length() && urls.size() < MAX_TABS; i++) {
                String url = array.optString(i, "");
                if (Address.isWebUrl(url)) urls.add(url);
            }
        } catch (JSONException ignored) {
            // Ignore corrupt session metadata and start with the Pip home page.
        }
        return urls;
    }

    private void persistSession() {
        if (sessionPreferences == null || tabs.isEmpty()) return;
        JSONArray urls = new JSONArray();
        for (Tab tab : tabs) urls.put(Address.isWebUrl(tab.url) && tab.url.length() <= 8192
                ? tab.url : Address.HOME);
        sessionPreferences.edit().putString("tabUrls", urls.toString())
                .putInt("selected", Math.max(0, selected)).apply();
    }

    // API 33+ gestures use the platform OnBackInvokedCallback registered in onCreate.
    // This override remains solely for Android 8-12 compatibility.
    @SuppressLint("GestureBackNavigation")
    @SuppressWarnings("deprecation")
    @Override public void onBackPressed() { navigateBack(); }

    @Override protected void onPause() {
        paused = true;
        persistSession();
        for (Tab tab : tabs) if (tab.view != null) tab.view.onPause();
        CookieManager.getInstance().flush();
        super.onPause();
    }

    @Override protected void onResume() {
        super.onResume();
        paused = false;
        WebView view = activeView();
        if (view != null) view.onResume();
        updateFilteringStatus();
    }

    @Override protected void onSaveInstanceState(Bundle state) {
        ArrayList<String> urls = new ArrayList<>();
        for (Tab tab : tabs) {
            // Bound saved-instance data; do not serialize renderer/page memory into Binder.
            urls.add(tab.url.length() <= 8192 ? tab.url : Address.HOME);
        }
        state.putStringArrayList("tabUrls", urls);
        state.putInt("selected", selected);
        persistSession();
        super.onSaveInstanceState(state);
    }

    @Override protected void onDestroy() {
        // ActivityScenario.close() and system-driven finish paths can reach onDestroy
        // without a useful save-state callback. Persist the latest tab URLs explicitly.
        persistSession();
        destroying = true;
        if (fullscreenCallback != null) {
            WebChromeClient.CustomViewCallback callback = fullscreenCallback;
            fullscreenCallback = null;
            fullscreenView = null;
            callback.onCustomViewHidden();
        }
        cancelPendingFileChooser();
        cancelPendingWebsitePermission();
        content.removeAllViews();
        for (Tab tab : tabs) {
            WebView view = tab.view;
            tab.view = null;
            if (view != null) {
                view.stopLoading();
                view.destroy();
            }
        }
        storageExecutor.execute(dataStore::close);
        storageExecutor.shutdown();
        super.onDestroy();
    }
}
