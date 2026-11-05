package com.psharehub.pip;

import android.annotation.SuppressLint;
import android.app.Activity;
import android.app.AlertDialog;
import android.graphics.Color;
import android.net.http.SslError;
import android.os.Build;
import android.os.Bundle;
import android.os.Message;
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
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.Button;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;
import android.widget.Toast;

import java.util.ArrayList;

/** Initial Android shell; platform WebView owns rendering and its security updates. */
public final class MainActivity extends Activity {
    private static final int MAX_TABS = 8;
    private final ArrayList<Tab> tabs = new ArrayList<>();
    private int selected;
    private EditText address;
    private Button back;
    private Button forward;
    private Button tabButton;
    private FrameLayout content;
    private TextView status;
    private ProgressBar progress;
    private boolean paused;
    private boolean destroying;

    private static final class Tab {
        String url;
        String title = "";
        WebView view;
        Tab(String url) { this.url = url; }
    }

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        WebView.setWebContentsDebuggingEnabled(BuildConfig.DEBUG);
        CookieManager.getInstance().setAcceptCookie(true);
        buildUi();
        ArrayList<String> saved = state == null ? null : state.getStringArrayList("tabUrls");
        if (saved != null) {
            for (String url : saved) {
                if (tabs.size() < MAX_TABS && Address.isWebUrl(url)) tabs.add(new Tab(url));
            }
        }
        if (tabs.isEmpty()) tabs.add(new Tab(Address.HOME));
        selected = state == null ? 0 : Math.min(Math.max(0, state.getInt("selected")), tabs.size() - 1);
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
            if (view != null && view.canGoForward()) view.goForward();
        });
        Button reload = button(R.string.reload, () -> {
            Tab tab = tabs.get(selected);
            if (tab.view == null) selectTab(selected);
            else tab.view.reload();
        });
        tabButton = button(R.string.tabs, this::showTabs);
        Button add = button(R.string.new_tab, () -> addTab(Address.HOME, true));
        for (Button item : new Button[]{back, forward, reload, tabButton, add}) {
            controls.addView(item, new LinearLayout.LayoutParams(0, dp(56), 1));
        }
        root.addView(controls);
        progress = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal);
        progress.setMax(100);
        root.addView(progress, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(3)));
        status = new TextView(this);
        status.setTextSize(15);
        status.setPadding(dp(12), dp(6), dp(12), dp(6));
        status.setVisibility(View.GONE);
        status.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);
        root.addView(status);
        content = new FrameLayout(this);
        root.addView(content, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1));
        setContentView(root);
        root.requestApplyInsets();
    }

    private Button button(int text, Runnable action) {
        Button button = new Button(this);
        button.setText(text);
        button.setTextSize(12);
        button.setAllCaps(false);
        button.setPadding(0, 0, 0, 0);
        button.setContentDescription(getString(text));
        button.setOnClickListener(v -> action.run());
        return button;
    }

    private int dp(int size) { return Math.round(size * getResources().getDisplayMetrics().density); }

    private void openAddress() {
        String url = Address.resolve(address.getText().toString());
        if (url == null) { showStatus(R.string.blocked_address); return; }
        Tab tab = tabs.get(selected);
        tab.url = url;
        if (tab.view == null) selectTab(selected);
        else tab.view.loadUrl(url);
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
        selectTab(tabs.size() - 1, load);
        return true;
    }

    private void selectTab(int index) { selectTab(index, true); }

    private void selectTab(int index, boolean load) {
        WebView previous = activeView();
        if (previous != null) previous.onPause();
        content.removeAllViews();
        selected = index;
        Tab tab = tabs.get(selected);
        boolean created = tab.view == null;
        if (created) tab.view = createWebView(tab);
        content.addView(tab.view, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        if (!paused) tab.view.onResume();
        status.setVisibility(View.GONE);
        address.setText(tab.url);
        if (created && load) tab.view.loadUrl(tab.url);
        updateControls();
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
            if (isCurrentView(tab, view)) {
                Toast.makeText(this, R.string.download_unavailable, Toast.LENGTH_LONG).show();
            }
        });
        view.setWebViewClient(new WebViewClient() {
            @Override public boolean shouldOverrideUrlLoading(WebView webView, WebResourceRequest request) {
                if (!isLiveView(tab, webView)) return true;
                boolean allowed = Address.isWebUrl(request.getUrl().toString());
                if (!allowed && request.isForMainFrame() && isCurrentView(tab, webView)) {
                    showStatus(R.string.blocked_address);
                }
                return !allowed;
            }

            @Override public void onPageStarted(WebView webView, String url, android.graphics.Bitmap icon) {
                if (!isLiveView(tab, webView)) return;
                if (Address.isWebUrl(url)) tab.url = url;
                if (isCurrentView(tab, webView)) {
                    status.setVisibility(View.GONE);
                    if (!address.hasFocus()) address.setText(tab.url);
                    updateControls();
                }
            }

            @Override public void onPageFinished(WebView webView, String url) {
                if (isCurrentView(tab, webView)) updateControls();
            }

            @Override public void onReceivedError(WebView webView, WebResourceRequest request, WebResourceError error) {
                if (request.isForMainFrame() && isCurrentView(tab, webView)) showStatus(R.string.load_error);
            }

            @Override public void onReceivedSslError(WebView webView, SslErrorHandler handler, SslError error) {
                handler.cancel();
                if (isCurrentView(tab, webView)) showStatus(R.string.ssl_error);
            }

            @Override public boolean onRenderProcessGone(WebView webView, RenderProcessGoneDetail detail) {
                if (!isLiveView(tab, webView)) return true;
                boolean wasCurrent = isCurrentView(tab, webView);
                tab.view = null;
                if (webView.getParent() instanceof ViewGroup) ((ViewGroup) webView.getParent()).removeView(webView);
                webView.destroy();
                if (wasCurrent) {
                    showStatus(R.string.render_error);
                    updateControls();
                }
                return true;
            }
        });
        view.setWebChromeClient(new WebChromeClient() {
            @Override public void onProgressChanged(WebView webView, int value) {
                if (isCurrentView(tab, webView)) {
                    progress.setProgress(value);
                    progress.setVisibility(value == 100 ? View.INVISIBLE : View.VISIBLE);
                }
            }

            @Override public void onReceivedTitle(WebView webView, String title) {
                if (isLiveView(tab, webView)) tab.title = title == null ? "" : title;
            }

            @Override public void onPermissionRequest(PermissionRequest request) {
                request.deny();
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

    private void showTabs() {
        String[] labels = new String[tabs.size()];
        for (int i = 0; i < tabs.size(); i++) {
            Tab tab = tabs.get(i);
            String title = tab.title.isEmpty() ? tab.url : tab.title;
            labels[i] = (i == selected ? "✓ " : "") + title;
        }
        new AlertDialog.Builder(this)
                .setTitle(getString(R.string.tabs) + " (" + tabs.size() + ")")
                .setItems(labels, (dialog, index) -> selectTab(index))
                .setPositiveButton(R.string.new_tab, (dialog, which) -> addTab(Address.HOME, true))
                .setNeutralButton(R.string.close_tab, (dialog, which) -> closeTab())
                .setNegativeButton(R.string.cancel, null)
                .show();
    }

    private void closeTab() {
        Tab removed = tabs.remove(selected);
        WebView removedView = removed.view;
        removed.view = null;
        if (tabs.isEmpty()) tabs.add(new Tab(Address.HOME));
        selected = Math.min(selected, tabs.size() - 1);
        // Repair selection and revoke the old view identity before WebView teardown
        // can deliver queued navigation/progress/renderer callbacks.
        if (removedView != null) {
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

    private WebView activeView() {
        return destroying || selected < 0 || selected >= tabs.size() ? null : tabs.get(selected).view;
    }

    private void updateControls() {
        WebView view = activeView();
        back.setEnabled(view != null && view.canGoBack());
        forward.setEnabled(view != null && view.canGoForward());
        tabButton.setText(getString(R.string.tabs_count, tabs.size()));
    }

    private void showStatus(int message) {
        status.setText(message);
        status.setVisibility(View.VISIBLE);
        progress.setVisibility(View.INVISIBLE);
    }

    private void navigateBack() {
        WebView view = activeView();
        if (view != null && view.canGoBack()) view.goBack();
        else finish();
    }

    // API 33+ gestures use the platform OnBackInvokedCallback registered in onCreate.
    // This override remains solely for Android 8-12 compatibility.
    @SuppressLint("GestureBackNavigation")
    @SuppressWarnings("deprecation")
    @Override public void onBackPressed() { navigateBack(); }

    @Override protected void onPause() {
        paused = true;
        for (Tab tab : tabs) if (tab.view != null) tab.view.onPause();
        CookieManager.getInstance().flush();
        super.onPause();
    }

    @Override protected void onResume() {
        super.onResume();
        paused = false;
        WebView view = activeView();
        if (view != null) view.onResume();
    }

    @Override protected void onSaveInstanceState(Bundle state) {
        ArrayList<String> urls = new ArrayList<>();
        for (Tab tab : tabs) {
            // Bound saved-instance data; do not serialize renderer/page memory into Binder.
            urls.add(tab.url.length() <= 8192 ? tab.url : Address.HOME);
        }
        state.putStringArrayList("tabUrls", urls);
        state.putInt("selected", selected);
        super.onSaveInstanceState(state);
    }

    @Override protected void onDestroy() {
        destroying = true;
        content.removeAllViews();
        for (Tab tab : tabs) {
            WebView view = tab.view;
            tab.view = null;
            if (view != null) {
                view.stopLoading();
                view.destroy();
            }
        }
        super.onDestroy();
    }
}
