import { describe, expect, it } from 'bun:test'
import {
  applyPipMigrationTheme,
  countBookmarks,
  countHistoryEvents,
  parsePipMigration,
} from './pip-migration'

function exportText(overrides: Record<string, unknown> = {}): string {
  return JSON.stringify({
    format: 'pip-dev-migration',
    version: 1,
    profiles: [
      {
        id: 'main-profile',
        name: 'Main',
        bookmarks: [{ url: 'https://example.com/path', title: 'Example\u0000 page' }],
        bookmark_folders: [{ id: 'work', name: 'Work', parent_id: null }],
        bookmark_folder_assignments: [
          { url: 'https://example.com/path', folder_id: 'work' },
        ],
        history: [{ url: 'https://history.example/', title: 'Visited' }],
        history_visits: [
          { url: 'https://history.example/', title: 'Visited', visited_at_ms: 1_700_000_000_000 },
        ],
      },
    ],
    settings: {},
    ...overrides,
  })
}

describe('Pip migration parser', () => {
  it('maps each supported appearance theme for the Pip Dev cockpit', () => {
    for (const theme of ['light', 'dark', 'system'] as const) {
      const bundle = parsePipMigration(
        exportText({ settings: { appearance: { theme } } }),
      )
      const applied: string[] = []
      expect(
        applyPipMigrationTheme(bundle.settings, (value) => applied.push(value)),
      ).toBe(theme)
      expect(applied).toEqual([theme])
    }
  })

  it('ignores invalid or misplaced theme values', () => {
    for (const theme of ['auto', 'DARK', '', null, 1]) {
      const bundle = parsePipMigration(
        exportText({ settings: { appearance: { theme } } }),
      )
      const applied: string[] = []
      expect(
        applyPipMigrationTheme(bundle.settings, (value) => applied.push(value)),
      ).toBeNull()
      expect(applied).toEqual([])
    }
    const applied: string[] = []
    expect(
      applyPipMigrationTheme({ theme: 'dark' }, (value) => applied.push(value)),
    ).toBeNull()
    expect(
      applyPipMigrationTheme({ appearance: 'dark' }, (value) => applied.push(value)),
    ).toBeNull()
    expect(applyPipMigrationTheme({}, (value) => applied.push(value))).toBeNull()
    expect(applied).toEqual([])
  })

  it('validates profile data and counts bookmark and visit records', () => {
    const migration = parsePipMigration(exportText())

    expect(migration.profiles[0]?.bookmarks[0]).toEqual({
      url: 'https://example.com/path',
      title: 'Example  page',
    })
    expect(migration.profiles[0]?.bookmark_folders[0]?.name).toBe('Work')
    expect(countBookmarks(migration)).toBe(1)
    expect(countHistoryEvents(migration)).toBe(1)
  })

  it('removes unsupported, sensitive and prototype-mutating preference keys', () => {
    const settings = JSON.parse(
      '{"theme":"dark","__proto__":{"polluted":true},"apiToken":"hidden"}',
    ) as Record<string, unknown>
    const migration = parsePipMigration(
      exportText({
        settings: {
          google_dns: true,
          appearance: settings,
          password: 'never import',
          unsupported: 'ignored',
        },
      }),
    )

    expect(migration.settings).toEqual({
      google_dns: true,
      appearance: { theme: 'dark' },
    })
    expect(migration.omittedSensitiveSettings).toBe(4)
    expect(({} as Record<string, unknown>).polluted).toBeUndefined()
  })

  it('rejects executable and credential-bearing URLs', () => {
    expect(() =>
      parsePipMigration(
        exportText({
          profiles: [
            {
              id: 'main-profile',
              name: 'Main',
              bookmarks: [{ url: 'javascript:alert(1)', title: 'bad' }],
            },
          ],
        }),
      ),
    ).toThrow('chỉ nhập URL HTTP/HTTPS')

    expect(() =>
      parsePipMigration(
        exportText({
          profiles: [
            {
              id: 'main-profile',
              name: 'Main',
              history: [{ url: 'https://user:secret@example.com', title: 'bad' }],
            },
          ],
        }),
      ),
    ).toThrow('chỉ nhập URL HTTP/HTTPS')
  })

  it('rejects cyclic bookmark folders and duplicate profile IDs', () => {
    expect(() =>
      parsePipMigration(
        exportText({
          profiles: [
            {
              id: 'main-profile',
              name: 'Main',
              bookmark_folders: [
                { id: 'a', name: 'A', parent_id: 'b' },
                { id: 'b', name: 'B', parent_id: 'a' },
              ],
            },
          ],
        }),
      ),
    ).toThrow('vòng lặp thư mục')

    expect(() =>
      parsePipMigration(
        exportText({
          profiles: [
            { id: 'same', name: 'One' },
            { id: 'same', name: 'Two' },
          ],
        }),
      ),
    ).toThrow('không hợp lệ hoặc bị trùng')
  })
})
