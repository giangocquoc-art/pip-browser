package com.psharehub.pip;

import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.database.sqlite.SQLiteDatabase;
import android.database.sqlite.SQLiteOpenHelper;

import java.util.ArrayList;
import java.util.List;

/** Small app-private store for browser history and bookmarks; it never stores cookies. */
final class BrowserDataStore extends SQLiteOpenHelper {
    private static final String DATABASE = "pip-browser.db";
    private static final int VERSION = 1;
    private static final int MAX_HISTORY = 1000;

    static final class Entry {
        final String url;
        final String title;
        final long visitedAt;

        Entry(String url, String title, long visitedAt) {
            this.url = url;
            this.title = title;
            this.visitedAt = visitedAt;
        }

        String label() {
            return title == null || title.isEmpty() || title.equals(url) ? url : title + "\n" + url;
        }
    }

    BrowserDataStore(Context context) {
        super(context.getApplicationContext(), DATABASE, null, VERSION);
    }

    @Override public void onCreate(SQLiteDatabase db) {
        db.execSQL("CREATE TABLE history (id INTEGER PRIMARY KEY AUTOINCREMENT, "
                + "url TEXT NOT NULL, title TEXT NOT NULL, visited_at INTEGER NOT NULL)");
        db.execSQL("CREATE INDEX history_visited_at ON history(visited_at DESC, id DESC)");
        db.execSQL("CREATE TABLE bookmarks (url TEXT PRIMARY KEY NOT NULL, "
                + "title TEXT NOT NULL, created_at INTEGER NOT NULL)");
    }

    @Override public void onUpgrade(SQLiteDatabase db, int oldVersion, int newVersion) {
        // Keep user data intact. Future schema changes must add explicit migrations here.
    }

    void recordVisit(String url, String title) {
        if (!Address.isWebUrl(url)) return;
        SQLiteDatabase db = getWritableDatabase();
        ContentValues values = new ContentValues();
        values.put("url", url);
        values.put("title", safeTitle(title, url));
        values.put("visited_at", System.currentTimeMillis());
        db.insert("history", null, values);
        db.execSQL("DELETE FROM history WHERE id NOT IN "
                + "(SELECT id FROM history ORDER BY visited_at DESC, id DESC LIMIT ?)",
                new Object[]{MAX_HISTORY});
    }

    void addBookmark(String url, String title) {
        if (!Address.isWebUrl(url)) return;
        ContentValues values = new ContentValues();
        values.put("url", url);
        values.put("title", safeTitle(title, url));
        values.put("created_at", System.currentTimeMillis());
        getWritableDatabase().insertWithOnConflict(
                "bookmarks", null, values, SQLiteDatabase.CONFLICT_REPLACE);
    }

    void removeBookmark(String url) {
        getWritableDatabase().delete("bookmarks", "url = ?", new String[]{url});
    }

    boolean isBookmarked(String url) {
        try (Cursor cursor = getReadableDatabase().rawQuery(
                "SELECT 1 FROM bookmarks WHERE url = ? LIMIT 1", new String[]{url})) {
            return cursor.moveToFirst();
        }
    }

    List<Entry> getBookmarks() {
        return query("SELECT url, title, created_at FROM bookmarks ORDER BY title COLLATE NOCASE, url", null, 500);
    }

    List<Entry> getRecentHistory() {
        return query("SELECT url, title, visited_at FROM history ORDER BY visited_at DESC, id DESC", null, 500);
    }

    void clearHistory() {
        getWritableDatabase().delete("history", null, null);
    }

    int historyCount() {
        try (Cursor cursor = getReadableDatabase().rawQuery("SELECT COUNT(*) FROM history", null)) {
            return cursor.moveToFirst() ? cursor.getInt(0) : 0;
        }
    }

    private List<Entry> query(String sql, String[] args, int limit) {
        ArrayList<Entry> entries = new ArrayList<>();
        try (Cursor cursor = getReadableDatabase().rawQuery(sql + " LIMIT " + limit, args)) {
            while (cursor.moveToNext()) {
                entries.add(new Entry(cursor.getString(0), cursor.getString(1), cursor.getLong(2)));
            }
        }
        return entries;
    }

    private static String safeTitle(String title, String url) {
        if (title == null || title.trim().isEmpty()) return url;
        String value = title.trim();
        return value.length() <= 512 ? value : value.substring(0, 512);
    }
}
