package com.psharehub.pip;

import android.content.Context;

import androidx.test.core.app.ApplicationProvider;
import androidx.test.ext.junit.runners.AndroidJUnit4;

import org.junit.Test;
import org.junit.runner.RunWith;

import java.util.List;

import static org.junit.Assert.*;

/** Uses Android's real SQLite implementation and app-private database. */
@RunWith(AndroidJUnit4.class)
public final class BrowserDataStoreTest {
    @Test public void bookmarksAndHistoryPersistAndCanBeClearedIndependently() {
        Context context = ApplicationProvider.getApplicationContext();
        context.deleteDatabase("pip-browser.db");

        BrowserDataStore first = new BrowserDataStore(context);
        first.addBookmark("https://example.com/", "Example");
        first.recordVisit("https://example.com/", "Example");
        first.recordVisit("https://example.com/page", "Example page");
        first.close();

        BrowserDataStore reopened = new BrowserDataStore(context);
        assertTrue(reopened.isBookmarked("https://example.com/"));
        List<BrowserDataStore.Entry> bookmarks = reopened.getBookmarks();
        assertEquals(1, bookmarks.size());
        assertEquals("Example", bookmarks.get(0).title);

        List<BrowserDataStore.Entry> history = reopened.getRecentHistory();
        assertEquals(2, history.size());
        assertEquals("https://example.com/page", history.get(0).url);
        reopened.clearHistory();
        assertTrue(reopened.getRecentHistory().isEmpty());
        assertTrue("Clearing history must not remove bookmarks", reopened.isBookmarked("https://example.com/"));
        reopened.removeBookmark("https://example.com/");
        assertFalse(reopened.isBookmarked("https://example.com/"));
        reopened.close();
        context.deleteDatabase("pip-browser.db");
    }

    @Test public void historyBoundRejectsNonHttpsAndPrunesOldestEntries() {
        Context context = ApplicationProvider.getApplicationContext();
        context.deleteDatabase("pip-browser.db");
        BrowserDataStore store = new BrowserDataStore(context);
        store.recordVisit("http://insecure.example/", "Ignored");
        for (int i = 0; i < 1005; i++) {
            store.recordVisit("https://example.com/" + i, "Page " + i);
        }
        assertEquals(1000, store.historyCount());
        List<BrowserDataStore.Entry> history = store.getRecentHistory();
        assertEquals(500, history.size());
        assertEquals("https://example.com/1004", history.get(0).url);
        assertFalse(Address.isWebUrl("http://insecure.example/"));
        store.close();
        context.deleteDatabase("pip-browser.db");
    }
}
