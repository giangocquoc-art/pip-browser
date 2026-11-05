package com.psharehub.pip;

import org.junit.Test;
import static org.junit.Assert.*;

public final class AddressTest {
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
