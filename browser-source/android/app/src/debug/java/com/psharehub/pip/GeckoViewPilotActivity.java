package com.psharehub.pip;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.graphics.Color;
import android.os.Bundle;
import android.text.InputType;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.TextView;

import org.mozilla.geckoview.GeckoRuntime;
import org.mozilla.geckoview.GeckoRuntimeSettings;
import org.mozilla.geckoview.GeckoSession;
import org.mozilla.geckoview.GeckoSessionSettings;
import org.mozilla.geckoview.GeckoView;

import java.net.URI;
import java.util.List;

/** Debug-only engine pilot. The shipping browser remains on Android System WebView. */
public final class GeckoViewPilotActivity extends Activity {
    static final String EXTRA_INITIAL_URI = "gecko-pilot-initial-uri";
    static final String PROFILE_A = "pip-gecko-profile-a";
    static final String PROFILE_B = "pip-gecko-profile-b";
    static final String GOOGLE_DOH = "https://dns.google/dns-query";

    private static GeckoRuntime runtime;

    private GeckoView geckoView;
    private EditText address;
    private TextView status;
    private GeckoSession session;
    private String activeProfile = PROFILE_A;
    private String activeUri = "https://example.com/";
    private boolean privateMode;

    static GeckoSessionSettings createSessionSettings(String profileId, boolean privateMode) {
        String contextId = privateMode ? null : profileId;
        return new GeckoSessionSettings.Builder()
                .contextId(contextId)
                .usePrivateMode(privateMode)
                .build();
    }

    static GeckoRuntimeSettings createRuntimeSettings() {
        return new GeckoRuntimeSettings.Builder()
                .trustedRecursiveResolverUri(GOOGLE_DOH)
                .trustedRecursiveResolverMode(GeckoRuntimeSettings.TRR_MODE_ONLY)
                .build();
    }

    static synchronized GeckoRuntime getOrCreateRuntime(Context context) {
        if (runtime == null) {
            runtime = GeckoRuntime.create(context.getApplicationContext(), createRuntimeSettings());
        }
        return runtime;
    }

    @Override protected void onCreate(Bundle state) {
        super.onCreate(state);
        buildUi();
        Intent intent = getIntent();
        String firstUri = intent == null ? null : intent.getStringExtra(EXTRA_INITIAL_URI);
        if (!isPilotUri(firstUri)) firstUri = "https://example.com/";
        address.setText(firstUri);
        openSession(PROFILE_A, false, firstUri);
    }

    private void buildUi() {
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(Color.WHITE);

        TextView banner = new TextView(this);
        banner.setText("Pip GeckoView pilot — debug build only; not the release engine");
        banner.setTextSize(14);
        banner.setPadding(12, 10, 12, 10);
        root.addView(banner);

        LinearLayout profiles = new LinearLayout(this);
        profiles.addView(button("Hồ sơ A", () -> openSession(PROFILE_A, false, currentUri())));
        profiles.addView(button("Hồ sơ B", () -> openSession(PROFILE_B, false, currentUri())));
        profiles.addView(button("Ẩn danh", () -> openSession(null, true, currentUri())));
        root.addView(profiles);

        LinearLayout addressRow = new LinearLayout(this);
        address = new EditText(this);
        address.setSingleLine(true);
        address.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        address.setHint("https://example.com");
        addressRow.addView(address, new LinearLayout.LayoutParams(0, dp(52), 1));
        addressRow.addView(button("Mở", () -> navigate(address.getText().toString())));
        root.addView(addressRow);

        status = new TextView(this);
        status.setTextSize(12);
        status.setPadding(12, 6, 12, 6);
        root.addView(status);

        geckoView = new GeckoView(this);
        root.addView(geckoView, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1));
        setContentView(root);
    }

    private Button button(String title, Runnable action) {
        Button button = new Button(this);
        button.setText(title);
        button.setAllCaps(false);
        button.setOnClickListener(view -> action.run());
        return button;
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }

    private String currentUri() {
        return activeUri;
    }

    private void navigate(String uri) {
        if (!isPilotUri(uri)) {
            status.setText("Chỉ cho phép URL HTTP/HTTPS trong pilot này.");
            return;
        }
        activeUri = uri.trim();
        address.setText(uri);
        session.loadUri(uri);
        updateStatus("Đang tải");
    }

    private static boolean isPilotUri(String value) {
        if (value == null || value.trim().isEmpty() || value.contains("\\")) return false;
        try {
            URI uri = new URI(value.trim());
            String scheme = uri.getScheme();
            return scheme != null && (scheme.equalsIgnoreCase("http") || scheme.equalsIgnoreCase("https"))
                    && uri.getHost() != null && uri.getRawUserInfo() == null;
        } catch (Exception invalid) {
            return false;
        }
    }

    private void openSession(String profileId, boolean privateSession, String uri) {
        GeckoSession previous = session;
        GeckoSession next = new GeckoSession(createSessionSettings(profileId, privateSession));
        next.setProgressDelegate(new GeckoSession.ProgressDelegate() {
            @Override public void onPageStop(GeckoSession page, boolean success) {
                runOnUiThread(() -> {
                    if (session != page || isFinishing()) return;
                    address.setText(activeUri);
                    updateStatus(success ? "Trang đã tải" : "Trang tải lỗi");
                });
            }
        });
        next.setNavigationDelegate(new GeckoSession.NavigationDelegate() {
            @Override public void onLocationChange(GeckoSession page, String url,
                    List<GeckoSession.PermissionDelegate.ContentPermission> permissions,
                    Boolean hasUserGesture) {
                if (!isPilotUri(url)) return;
                runOnUiThread(() -> {
                    if (session != page || isFinishing()) return;
                    activeUri = url;
                    address.setText(url);
                });
            }
        });
        next.open(getOrCreateRuntime(this));
        session = next;
        activeProfile = privateSession ? "private" : profileId;
        privateMode = privateSession;
        geckoView.setSession(next);
        if (previous != null) previous.close();
        if (isPilotUri(uri)) {
            activeUri = uri.trim();
            address.setText(activeUri);
            next.loadUri(activeUri);
        }
        updateStatus("Sẵn sàng");
    }

    private void updateStatus(String pageState) {
        if (status == null) return;
        String profile = privateMode ? "Ẩn danh" : activeProfile;
        status.setText(profile + " · Google DoH đã cấu hình (TRR_MODE_ONLY) · " + pageState
                + " · Chưa xác minh tuyến DNS bằng trace mạng");
    }

    @Override protected void onDestroy() {
        GeckoSession current = session;
        session = null;
        if (current != null) {
            geckoView.setSession(null);
            current.close();
        }
        super.onDestroy();
    }
}
