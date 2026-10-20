/**
 * Local-only importer for the JSON bundle emitted by Pip's export script.
 * Cookies, credentials, and arbitrary executable content are never imported.
 */

const MAX_FILE_BYTES = 20 * 1024 * 1024
const MAX_PROFILES = 50
const MAX_BOOKMARKS = 5_000
const MAX_HISTORY_EVENTS = 5_000
const MAX_FOLDERS_PER_PROFILE = 200
const SAFE_SETTINGS_KEYS = new Set([
  'search_engine',
  'appearance',
  'performance',
  'privacy',
  'tabs',
  'adblock_mode',
  'google_dns',
])
const SENSITIVE_KEY = /cookie|password|credential|secret|token|session|auth|autofill/i
const UNSAFE_OBJECT_KEYS = new Set(['__proto__', 'constructor', 'prototype'])

export interface PipPage {
  url: string
  title: string
}

export interface PipBookmarkFolder {
  id: string
  name: string
  parent_id: string | null
}

export interface PipBookmarkAssignment {
  url: string
  folder_id: string | null
}

export interface PipHistoryVisit extends PipPage {
  visited_at_ms: number
}

export interface PipMigrationProfile {
  id: string
  name: string
  bookmarks: PipPage[]
  bookmark_folders: PipBookmarkFolder[]
  bookmark_folder_assignments: PipBookmarkAssignment[]
  history: PipPage[]
  history_visits: PipHistoryVisit[]
}

export interface PipMigrationBundle {
  format: 'pip-dev-migration'
  version: 1
  exported_at?: string
  profiles: PipMigrationProfile[]
  settings: Record<string, unknown>
  omittedSensitiveSettings: number
}

export interface ImportSelection {
  bookmarks: boolean
  history: boolean
  settings: boolean
}

export interface ImportProgress {
  phase: 'bookmarks' | 'history' | 'settings'
  completed: number
  total: number
}

export interface ImportResult {
  bookmarks: number
  historyEvents: number
  settingsSaved: boolean
}

export function parsePipMigration(text: string): PipMigrationBundle {
  if (new TextEncoder().encode(text).byteLength > MAX_FILE_BYTES) {
    throw new Error('Tệp vượt quá giới hạn 20 MB.')
  }

  let input: unknown
  try {
    input = JSON.parse(text)
  } catch {
    throw new Error('Tệp không phải JSON hợp lệ.')
  }
  if (!isRecord(input) || input.format !== 'pip-dev-migration' || input.version !== 1) {
    throw new Error('Không nhận ra định dạng Pip migration (cần version 1).')
  }
  if (!Array.isArray(input.profiles) || input.profiles.length < 1 || input.profiles.length > MAX_PROFILES) {
    throw new Error(`Tệp phải có từ 1 đến ${MAX_PROFILES} hồ sơ.`)
  }
  let declaredBookmarks = 0
  let declaredHistoryEvents = 0
  for (const [index, profile] of input.profiles.entries()) {
    if (!isRecord(profile)) throw new Error(`Hồ sơ ${index + 1} không hợp lệ.`)
    if (profile.bookmarks !== undefined && !Array.isArray(profile.bookmarks)) {
      throw new Error(`Dấu trang của hồ sơ ${index + 1} không hợp lệ.`)
    }
    if (profile.history !== undefined && !Array.isArray(profile.history)) {
      throw new Error(`Lịch sử của hồ sơ ${index + 1} không hợp lệ.`)
    }
    if (profile.history_visits !== undefined && !Array.isArray(profile.history_visits)) {
      throw new Error(`Lượt lịch sử của hồ sơ ${index + 1} không hợp lệ.`)
    }
    declaredBookmarks += Array.isArray(profile.bookmarks) ? profile.bookmarks.length : 0
    declaredHistoryEvents += Array.isArray(profile.history_visits) && profile.history_visits.length
      ? profile.history_visits.length
      : Array.isArray(profile.history)
        ? profile.history.length
        : 0
    if (declaredBookmarks > MAX_BOOKMARKS || declaredHistoryEvents > MAX_HISTORY_EVENTS) {
      throw new Error(`Tệp vượt giới hạn ${MAX_BOOKMARKS} dấu trang hoặc ${MAX_HISTORY_EVENTS} lượt lịch sử.`)
    }
  }

  let bookmarkCount = 0
  let historyCount = 0
  const seenProfileIds = new Set<string>()
  const profiles = input.profiles.map((value, index) => {
    if (!isRecord(value)) throw new Error(`Hồ sơ ${index + 1} không hợp lệ.`)
    const id = boundedString(value.id, `ID hồ sơ ${index + 1}`, 128)
    if (!/^[A-Za-z0-9_-]+$/.test(id) || seenProfileIds.has(id)) {
      throw new Error(`ID hồ sơ “${id}” không hợp lệ hoặc bị trùng.`)
    }
    seenProfileIds.add(id)
    const name = boundedString(value.name ?? id, `Tên hồ sơ ${index + 1}`, 120)
    const bookmarks = parsePages(value.bookmarks, `dấu trang của ${name}`)
    bookmarkCount += bookmarks.length
    const folders = parseFolders(value.bookmark_folders, name)
    if (folders.length > MAX_FOLDERS_PER_PROFILE) {
      throw new Error(`Hồ sơ “${name}” vượt quá ${MAX_FOLDERS_PER_PROFILE} thư mục dấu trang.`)
    }
    validateFolderTree(folders, name)
    const assignments = parseAssignments(value.bookmark_folder_assignments, name)
    const knownFolders = new Set(folders.map((folder) => folder.id))
    if (assignments.some((assignment) => assignment.folder_id && !knownFolders.has(assignment.folder_id))) {
      throw new Error(`Hồ sơ “${name}” có dấu trang trỏ tới thư mục không tồn tại.`)
    }
    const history = parsePages(value.history, `lịch sử của ${name}`)
    const historyVisits = parseVisits(value.history_visits, name)
    historyCount += historyVisits.length || history.length
    return {
      id,
      name,
      bookmarks,
      bookmark_folders: folders,
      bookmark_folder_assignments: assignments,
      history,
      history_visits: historyVisits,
    }
  })

  if (bookmarkCount > MAX_BOOKMARKS) {
    throw new Error(`Tệp có ${bookmarkCount} dấu trang; giới hạn là ${MAX_BOOKMARKS}.`)
  }
  if (historyCount > MAX_HISTORY_EVENTS) {
    throw new Error(`Tệp có ${historyCount} lượt lịch sử; giới hạn là ${MAX_HISTORY_EVENTS}.`)
  }

  const safe = sanitizeSettings(input.settings)
  return {
    format: 'pip-dev-migration',
    version: 1,
    exported_at: typeof input.exported_at === 'string' ? input.exported_at.slice(0, 80) : undefined,
    profiles,
    settings: safe.value,
    omittedSensitiveSettings: safe.omitted,
  }
}

export function countBookmarks(bundle: PipMigrationBundle): number {
  return bundle.profiles.reduce((count, profile) => count + profile.bookmarks.length, 0)
}

export function countHistoryEvents(bundle: PipMigrationBundle): number {
  return bundle.profiles.reduce(
    (count, profile) => count + (profile.history_visits.length || profile.history.length),
    0,
  )
}

/** Applies selected data through the browser's local extension APIs only. */
export async function importPipMigration(
  bundle: PipMigrationBundle,
  selection: ImportSelection,
  onProgress?: (progress: ImportProgress) => void,
): Promise<ImportResult> {
  const result: ImportResult = { bookmarks: 0, historyEvents: 0, settingsSaved: false }

  if (selection.bookmarks) {
    const tree = await chrome.bookmarks.getTree()
    const bar = tree[0]?.children?.find((node) => node.id === '1') ?? tree[0]?.children?.[0]
    const parentId = bar?.id ?? tree[0]?.id
    if (!parentId) throw new Error('Không tìm thấy thanh dấu trang của hồ sơ hiện tại.')

    const total = countBookmarks(bundle)
    let completed = 0
    const createdRoots: string[] = []
    try {
      for (const profile of bundle.profiles) {
        const root = await chrome.bookmarks.create({
          parentId,
          title: `Pip — ${profile.name}`.slice(0, 250),
        })
        createdRoots.push(root.id)
        const folderIds = new Map<string, string>()
        const pending = [...profile.bookmark_folders]
        while (pending.length) {
          let createdThisPass = 0
          for (let index = pending.length - 1; index >= 0; index--) {
            const folder = pending[index]
            const parentFolderId = folder.parent_id ? folderIds.get(folder.parent_id) : root.id
            if (!parentFolderId) continue
            const created = await chrome.bookmarks.create({
              parentId: parentFolderId,
              title: folder.name.slice(0, 250),
            })
            folderIds.set(folder.id, created.id)
            pending.splice(index, 1)
            createdThisPass += 1
          }
          if (createdThisPass === 0) {
            throw new Error(`Không thể tạo cây thư mục trong hồ sơ “${profile.name}”.`)
          }
        }

        const folderByUrl = new Map(
          profile.bookmark_folder_assignments.map((assignment) => [assignment.url, assignment.folder_id]),
        )
        for (const page of profile.bookmarks) {
          const folderId = folderByUrl.get(page.url)
          const destination = folderId ? folderIds.get(folderId) : root.id
          if (!destination) continue
          await chrome.bookmarks.create({
            parentId: destination,
            title: (page.title || page.url).slice(0, 250),
            url: page.url,
          })
          completed += 1
          if (completed % 25 === 0 || completed === total) {
            onProgress?.({ phase: 'bookmarks', completed, total })
          }
        }
      }
    } catch (error) {
      for (const rootId of createdRoots.reverse()) {
        await chrome.bookmarks.removeTree(rootId).catch(() => undefined)
      }
      throw error
    }
    result.bookmarks = completed
  }

  if (selection.history) {
    const events = bundle.profiles.flatMap((profile) =>
      profile.history_visits.length
        ? profile.history_visits.map((visit) => visit.url)
        : profile.history.map((page) => page.url),
    )
    for (let index = 0; index < events.length; index++) {
      try {
        await chrome.history.addUrl({ url: events[index] })
      } catch (error) {
        throw new Error(
          `Lịch sử được nhập ${result.historyEvents}/${events.length} lượt trước khi dừng: ${error instanceof Error ? error.message : 'Chrome từ chối lượt truy cập.'}`,
        )
      }
      result.historyEvents += 1
      if ((index + 1) % 25 === 0 || index + 1 === events.length) {
        onProgress?.({ phase: 'history', completed: index + 1, total: events.length })
      }
    }
  }

  if (selection.settings || selection.bookmarks || selection.history) {
    const profileLabels = Object.fromEntries(bundle.profiles.map(({ id, name }) => [id, name]))
    const previous = await chrome.storage.local.get('pipMigration')
    const previousMigration = isRecord(previous.pipMigration) ? previous.pipMigration : {}
    await chrome.storage.local.set({
      pipMigration: {
        importedAt: new Date().toISOString(),
        profileLabels,
        ...(selection.settings
          ? { settings: bundle.settings }
          : isRecord(previousMigration.settings)
            ? { settings: previousMigration.settings }
            : {}),
      },
    })
    result.settingsSaved = selection.settings
    if (selection.settings) onProgress?.({ phase: 'settings', completed: 1, total: 1 })
  }
  return result
}

function parsePages(value: unknown, label: string): PipPage[] {
  if (value === undefined) return []
  if (!Array.isArray(value)) throw new Error(`${label} không hợp lệ.`)
  return value.map((page, index) => {
    if (!isRecord(page)) throw new Error(`${label}, mục ${index + 1} không hợp lệ.`)
    return {
      url: safeUrl(page.url, `${label}, mục ${index + 1}`),
      title: typeof page.title === 'string' ? page.title.replace(/[\u0000-\u001f]/g, ' ').slice(0, 500) : '',
    }
  })
}

function parseFolders(value: unknown, profileName: string): PipBookmarkFolder[] {
  if (value === undefined) return []
  if (!Array.isArray(value)) throw new Error(`Thư mục dấu trang của “${profileName}” không hợp lệ.`)
  if (value.length > MAX_FOLDERS_PER_PROFILE) {
    throw new Error(`Hồ sơ “${profileName}” vượt quá ${MAX_FOLDERS_PER_PROFILE} thư mục dấu trang.`)
  }
  return value.map((folder, index) => {
    if (!isRecord(folder)) throw new Error(`Thư mục ${index + 1} của “${profileName}” không hợp lệ.`)
    const id = boundedString(folder.id, 'ID thư mục', 128)
    const parent = folder.parent_id
    if (parent !== null && parent !== undefined && typeof parent !== 'string') {
      throw new Error(`Thư mục “${profileName}” có parent_id không hợp lệ.`)
    }
    if (typeof parent === 'string' && !parent) throw new Error(`Thư mục “${profileName}” có parent_id rỗng.`)
    return {
      id,
      name: boundedString(folder.name, 'Tên thư mục', 250),
      parent_id: typeof parent === 'string' ? parent : null,
    }
  })
}

function parseAssignments(value: unknown, profileName: string): PipBookmarkAssignment[] {
  if (value === undefined) return []
  if (!Array.isArray(value)) throw new Error(`Vị trí dấu trang của “${profileName}” không hợp lệ.`)
  if (value.length > MAX_BOOKMARKS) {
    throw new Error(`Hồ sơ “${profileName}” có quá nhiều vị trí dấu trang.`)
  }
  return value.map((assignment, index) => {
    if (!isRecord(assignment)) throw new Error(`Vị trí dấu trang ${index + 1} không hợp lệ.`)
    const folderId = assignment.folder_id
    if (folderId !== null && folderId !== undefined && typeof folderId !== 'string') {
      throw new Error(`Vị trí dấu trang ${index + 1} không hợp lệ.`)
    }
    if (typeof folderId === 'string' && !folderId) throw new Error(`Vị trí dấu trang ${index + 1} không hợp lệ.`)
    return {
      url: safeUrl(assignment.url, `Vị trí dấu trang ${index + 1}`),
      folder_id: typeof folderId === 'string' ? folderId : null,
    }
  })
}

function parseVisits(value: unknown, profileName: string): PipHistoryVisit[] {
  if (value === undefined) return []
  if (!Array.isArray(value)) throw new Error(`Lượt lịch sử của “${profileName}” không hợp lệ.`)
  return value.map((visit, index) => {
    if (!isRecord(visit)) throw new Error(`Lượt lịch sử ${index + 1} không hợp lệ.`)
    const timestamp = visit.visited_at_ms
    if (typeof timestamp !== 'number' || !Number.isSafeInteger(timestamp) || timestamp < 0) {
      throw new Error(`Thời gian của lượt lịch sử ${index + 1} không hợp lệ.`)
    }
    return {
      url: safeUrl(visit.url, `Lượt lịch sử ${index + 1}`),
      title: typeof visit.title === 'string' ? visit.title.replace(/[\u0000-\u001f]/g, ' ').slice(0, 500) : '',
      visited_at_ms: timestamp,
    }
  })
}

function validateFolderTree(folders: PipBookmarkFolder[], profileName: string) {
  const byId = new Map(folders.map((folder) => [folder.id, folder]))
  if (byId.size !== folders.length) throw new Error(`Hồ sơ “${profileName}” có ID thư mục bị trùng.`)
  for (const folder of folders) {
    if (folder.parent_id && !byId.has(folder.parent_id)) {
      throw new Error(`Hồ sơ “${profileName}” có thư mục cha không tồn tại.`)
    }
    const seen = new Set([folder.id])
    let parentId = folder.parent_id
    while (parentId) {
      if (seen.has(parentId)) throw new Error(`Hồ sơ “${profileName}” có vòng lặp thư mục.`)
      seen.add(parentId)
      parentId = byId.get(parentId)?.parent_id ?? null
    }
  }
}

function sanitizeSettings(value: unknown): { value: Record<string, unknown>; omitted: number } {
  if (value === undefined) return { value: {}, omitted: 0 }
  if (!isRecord(value)) throw new Error('Phần cài đặt trong tệp không hợp lệ.')
  let omitted = 0
  const output: Record<string, unknown> = {}
  for (const [key, item] of Object.entries(value)) {
    if (!SAFE_SETTINGS_KEYS.has(key)) {
      omitted += 1
      continue
    }
    const cleaned = sanitizeSettingValue(item, 0)
    output[key] = cleaned.value
    omitted += cleaned.omitted
  }
  return { value: output, omitted }
}

function sanitizeSettingValue(value: unknown, depth: number): { value: unknown; omitted: number } {
  if (depth > 5) return { value: null, omitted: 1 }
  if (value === null || typeof value === 'boolean' || typeof value === 'number') {
    return { value, omitted: 0 }
  }
  if (typeof value === 'string') return { value: value.slice(0, 500), omitted: 0 }
  if (Array.isArray(value)) {
    const entries = value.slice(0, 100).map((item) => sanitizeSettingValue(item, depth + 1))
    return { value: entries.map((entry) => entry.value), omitted: entries.reduce((sum, entry) => sum + entry.omitted, 0) + Math.max(0, value.length - 100) }
  }
  if (!isRecord(value)) return { value: null, omitted: 1 }
  let omitted = 0
  const result: Record<string, unknown> = {}
  for (const [key, item] of Object.entries(value).slice(0, 100)) {
    if (SENSITIVE_KEY.test(key) || UNSAFE_OBJECT_KEYS.has(key)) {
      omitted += 1
      continue
    }
    const cleaned = sanitizeSettingValue(item, depth + 1)
    result[key.slice(0, 100)] = cleaned.value
    omitted += cleaned.omitted
  }
  omitted += Math.max(0, Object.keys(value).length - 100)
  return { value: result, omitted }
}

function safeUrl(value: unknown, label: string): string {
  if (typeof value !== 'string' || value.length > 8_192) throw new Error(`${label}: URL không hợp lệ.`)
  let url: URL
  try {
    url = new URL(value)
  } catch {
    throw new Error(`${label}: URL không hợp lệ.`)
  }
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) {
    throw new Error(`${label}: chỉ nhập URL HTTP/HTTPS không chứa tên đăng nhập hoặc mật khẩu.`)
  }
  return url.href
}

function boundedString(value: unknown, label: string, maxLength: number): string {
  if (typeof value !== 'string' || value.length < 1 || value.length > maxLength || /[\u0000-\u001f]/.test(value)) {
    throw new Error(`${label} không hợp lệ.`)
  }
  return value
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}
