package com.psharehub.pip;

/** URL and filename checks shared by Android's download handler and unit tests. */
final class DownloadSupport {
    private DownloadSupport() {}

    static boolean isAllowedUrl(String value) {
        return Address.isWebUrl(value);
    }

    static String safeFilename(String candidate) {
        if (candidate == null) return "";
        String value = candidate.trim().replaceAll("[\\p{Cntrl}/\\\\:*?\"<>|]+", "_");
        while (value.contains("..")) value = value.replace("..", "_");
        while (value.startsWith(".")) value = value.substring(1);
        if (value.length() > 180) value = value.substring(0, 180);
        return value;
    }
}
