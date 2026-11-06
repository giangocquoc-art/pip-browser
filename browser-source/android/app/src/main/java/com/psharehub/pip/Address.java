package com.psharehub.pip;

import java.net.URI;
import java.net.URLEncoder;
import java.io.UnsupportedEncodingException;
import java.util.Locale;
import java.util.Objects;

/** HTTPS-first omnibox routing; never forwards file, content, intent or script URLs. */
final class Address {
    static final String HOME = "https://www.google.com/";

    private Address() {}

    static String resolve(String input) {
        String value = input == null ? "" : input.trim();
        if (value.isEmpty()) return HOME;
        if (value.chars().anyMatch(c -> Character.isISOControl(c))) return null;
        if (value.regionMatches(true, 0, "https://", 0, 8)) {
            return isWebUrl(value) ? value : null;
        }
        if (value.contains("://") || value.startsWith("//")) return null;
        boolean hostLike = !value.contains(" ") && !value.contains("\\")
                && (value.contains(".") || value.startsWith("[")
                || value.equals("localhost") || value.startsWith("localhost:"));
        if (hostLike) {
            String candidate = "https://" + value;
            if (isWebUrl(candidate)) return candidate;
        }
        if (value.matches("(?i)^[a-z][a-z0-9+.-]*:.*") || value.startsWith("//")) return null;
        try {
            return "https://www.google.com/search?q=" + URLEncoder.encode(value, "UTF-8");
        } catch (UnsupportedEncodingException impossible) {
            throw new AssertionError(impossible);
        }
    }

    static boolean isWebUrl(String value) {
        if (value == null || value.contains("\\")) return false;
        try {
            URI uri = new URI(value);
            return "https".equals(uri.getScheme().toLowerCase(Locale.ROOT))
                    && uri.getHost() != null && !uri.getHost().isEmpty()
                    && uri.getRawUserInfo() == null
                    && uri.getPort() >= -1 && uri.getPort() <= 65535;
        } catch (Exception invalid) {
            return false;
        }
    }

    /** Browser spelling equivalence only; never widen a typed exemption to a whole site. */
    static boolean sameNavigationUrl(String first, String second) {
        if (!isWebUrl(first) || !isWebUrl(second)) return false;
        try {
            URI a = new URI(first);
            URI b = new URI(second);
            String aPath = a.getRawPath();
            String bPath = b.getRawPath();
            if (aPath == null || aPath.isEmpty()) aPath = "/";
            if (bPath == null || bPath.isEmpty()) bPath = "/";
            return a.getScheme().equalsIgnoreCase(b.getScheme())
                    && a.getHost().equalsIgnoreCase(b.getHost())
                    && (a.getPort() == -1 ? 443 : a.getPort()) == (b.getPort() == -1 ? 443 : b.getPort())
                    && aPath.equals(bPath)
                    && Objects.equals(a.getRawQuery(), b.getRawQuery())
                    && Objects.equals(a.getRawFragment(), b.getRawFragment());
        } catch (Exception invalid) {
            return false;
        }
    }
}
