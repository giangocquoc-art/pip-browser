package com.psharehub.pip;

import org.junit.Test;
import static org.junit.Assert.*;

public final class AddressTest {
    @Test public void navigationExemptionsNormalizeOnlyBrowserUrlSpelling() {
        assertTrue(Address.sameNavigationUrl("https://ad88vip6.com", "HTTPS://AD88VIP6.COM:443/"));
        assertTrue(Address.sameNavigationUrl("https://example.com/a?q=1#part", "https://EXAMPLE.COM:443/a?q=1#part"));
        assertFalse(Address.sameNavigationUrl("https://ad88vip6.com", "https://ad88vip6.com/redirect"));
        assertFalse(Address.sameNavigationUrl("https://ad88vip6.com/", "https://ad88vip6.com/?q=1"));
        assertFalse(Address.sameNavigationUrl("https://example.com/?q=1", "https://example.com/?q=2"));
        assertFalse(Address.sameNavigationUrl("https://example.com/#a", "https://example.com/#b"));
        assertFalse(Address.sameNavigationUrl("https://example.com/a%2Fb", "https://example.com/a/b"));
        assertFalse(Address.sameNavigationUrl("https://example.com/", "https://example.com:8443/"));
        assertFalse(Address.sameNavigationUrl("https://example.com/", "https://sub.example.com/"));
        assertFalse(Address.sameNavigationUrl("https://example.com/", "https://user@example.com/"));
        assertFalse(Address.sameNavigationUrl(null, "https://example.com/"));
    }
    @Test public void routesDomainsAndSearchWithoutChangingGoogleDefault() {
        assertEquals(Address.HOME, Address.resolve(" "));
        assertEquals("https://example.com/path?q=1", Address.resolve(" example.com/path?q=1 "));
        assertEquals("https://localhost:8443/test", Address.resolve("localhost:8443/test"));
        assertEquals("https://www.google.com/search?q=b%C3%A0n+c%E1%BB%9D",
                Address.resolve("bàn cờ"));
        assertEquals("https://www.google.com/search?q=hello+world", Address.resolve("hello world"));
    }

    @Test public void refusesUnsafeSchemesCredentialsAndMalformedHosts() {
        assertNull(Address.resolve("javascript:alert(1)"));
        assertNull(Address.resolve("file:///sdcard/passwords"));
        assertNull(Address.resolve("content://contacts/1"));
        assertNull(Address.resolve("intent://example"));
        assertNull(Address.resolve("http://example.com"));
        assertNull(Address.resolve("https://user:pass@example.com"));
        assertNull(Address.resolve("https://example.com:70000"));
        assertNull(Address.resolve("//example.com"));
        assertNull(Address.resolve("https://example.com/\nscript"));
        assertFalse(Address.isWebUrl("https://trusted.example\\@evil.example"));
        assertFalse(Address.isWebUrl("https:///example.com"));
    }
}
