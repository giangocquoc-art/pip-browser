package com.psharehub.pip;

import org.junit.Test;
import static org.junit.Assert.*;

public final class DownloadSupportTest {
    @Test public void onlyAcceptsHttpsWebUrls() {
        assertTrue(DownloadSupport.isAllowedUrl("https://example.com/file.zip"));
        assertFalse(DownloadSupport.isAllowedUrl("http://example.com/file.zip"));
        assertFalse(DownloadSupport.isAllowedUrl("file:///sdcard/private.txt"));
        assertFalse(DownloadSupport.isAllowedUrl("https://user@example.com/file.zip"));
    }

    @Test public void sanitizesServerProvidedFileNames() {
        String pathLikeName = DownloadSupport.safeFilename("../report?.zip");
        assertFalse(pathLikeName.contains("/"));
        assertFalse(pathLikeName.contains(".."));
        assertFalse(DownloadSupport.safeFilename("../../").contains(".."));
        assertTrue(DownloadSupport.safeFilename("x".repeat(200)).length() <= 180);
    }
}
