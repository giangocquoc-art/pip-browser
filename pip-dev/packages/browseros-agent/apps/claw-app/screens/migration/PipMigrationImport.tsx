import { useEffect, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { useNavigate } from 'react-router'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import {
  countBookmarks,
  countHistoryEvents,
  applyPipMigrationTheme,
  importPipMigration,
  parsePipMigration,
  type ImportProgress,
  type ImportResult,
  type ImportSelection,
  type PipMigrationBundle,
} from '@/modules/migration/pip-migration'
import { useTheme } from '@/components/theme-provider'

const MAX_FILE_BYTES = 20 * 1024 * 1024

export function PipMigrationImport() {
  const navigate = useNavigate()
  const { setTheme } = useTheme()
  const fileInput = useRef<HTMLInputElement>(null)
  const [bundle, setBundle] = useState<PipMigrationBundle | null>(null)
  const [fileName, setFileName] = useState('')
  const [selection, setSelection] = useState<ImportSelection>({
    bookmarks: true,
    history: false,
    settings: false,
  })
  const [understood, setUnderstood] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [progress, setProgress] = useState<ImportProgress | null>(null)
  const [result, setResult] = useState<ImportResult | null>(null)
  const [savedLocalData, setSavedLocalData] = useState<Record<string, unknown> | null>(null)

  useEffect(() => {
    let active = true
    void chrome.storage.local.get('pipMigration').then((stored) => {
      if (active && isRecord(stored.pipMigration)) setSavedLocalData(stored.pipMigration)
    }).catch(() => undefined)
    return () => {
      active = false
    }
  }, [])

  const handleFile = async (file?: File) => {
    setBundle(null)
    setFileName('')
    setError(null)
    setProgress(null)
    setResult(null)
    setUnderstood(false)
    if (!file) return
    if (file.size > MAX_FILE_BYTES) {
      setError('Tệp vượt quá giới hạn 20 MB.')
      return
    }
    try {
      const parsed = parsePipMigration(await file.text())
      setBundle(parsed)
      setFileName(file.name)
      setSelection({ bookmarks: true, history: false, settings: false })
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Không thể đọc tệp Pip migration.')
    }
  }

  const handleImport = async () => {
    if (!bundle || busy || !understood) return
    setBusy(true)
    setError(null)
    setProgress(null)
    try {
      const imported = await importPipMigration(bundle, selection, setProgress)
      if (imported.settingsSaved) {
        applyPipMigrationTheme(bundle.settings, setTheme)
      }
      setResult(imported)
      try {
        const stored = await chrome.storage.local.get('pipMigration')
        if (isRecord(stored.pipMigration)) setSavedLocalData(stored.pipMigration)
      } catch {
        // The browser-side import has already completed; this is only a preview refresh.
      }
    } catch (cause) {
      setError(
        cause instanceof Error
          ? `${cause.message} Dữ liệu đã ghi trước lỗi có thể đã được nhập một phần.`
          : 'Nhập dữ liệu bị lỗi; một phần dữ liệu có thể đã được nhập.',
      )
    } finally {
      setBusy(false)
    }
  }

  const hasSelectedData = selection.bookmarks || selection.history || selection.settings

  return (
    <div className="mx-auto flex w-full max-w-2xl flex-col gap-6 px-6 pt-8 pb-16">
      <Button
        type="button"
        variant="ghost"
        className="-ml-2 w-fit"
        disabled={busy}
        onClick={() => navigate('/settings')}
      >
        ← Settings
      </Button>

      <header className="flex flex-col gap-1">
        <h1 className="font-extrabold text-3xl leading-tight tracking-tight md:text-4xl">
          Import Pip data
        </h1>
        <p className="text-ink-2 text-sm">
          Choose a Pip JSON export. The file is read locally by this extension;
          nothing is uploaded.
        </p>
      </header>

      <Card>
        <CardHeader>
          <CardTitle>Choose export file</CardTitle>
          <CardDescription>
            Use <code>pip-dev-migration.json</code> created by the Pip export tool. Maximum size: 20 MB.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <input
            ref={fileInput}
            type="file"
            accept=".json,application/json"
            aria-label="Select Pip migration JSON file"
            disabled={busy}
            onChange={(event) => {
              void handleFile(event.currentTarget.files?.[0])
              event.currentTarget.value = ''
            }}
            className="block w-full cursor-pointer rounded-md border border-border bg-background px-3 py-2 text-sm file:mr-3 file:rounded file:border-0 file:bg-muted file:px-3 file:py-1.5 file:text-sm"
          />
          {fileName && <p className="text-xs text-ink-2">Validated: {fileName}</p>}
        </CardContent>
      </Card>

      {bundle && (
        <>
          <Card>
            <CardHeader>
              <CardTitle>Review before importing</CardTitle>
              <CardDescription>
                {bundle.profiles.length} Pip profile(s)
                {bundle.exported_at ? ` · exported ${bundle.exported_at}` : ''}.
                {' '}Profile names are kept as labels and bookmark folders.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="space-y-2">
                {bundle.profiles.map((profile) => (
                  <div
                    key={profile.id}
                    className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-border px-3 py-2"
                  >
                    <span className="font-medium text-sm">{profile.name}</span>
                    <span className="text-xs text-ink-2">
                      {profile.bookmarks.length} bookmarks ·{' '}
                      {profile.history_visits.length || profile.history.length} history events
                    </span>
                  </div>
                ))}
              </div>

              <div className="space-y-3 border-t border-border pt-4">
                <ImportOption
                  id="pip-import-bookmarks"
                  checked={selection.bookmarks}
                  disabled={busy || countBookmarks(bundle) === 0}
                  onCheckedChange={(checked) =>
                    setSelection((current) => ({ ...current, bookmarks: checked }))
                  }
                  title={`Bookmarks (${countBookmarks(bundle)})`}
                  description="Imported under separate “Pip — profile name” folders in the current browser profile."
                />
                <ImportOption
                  id="pip-import-history"
                  checked={selection.history}
                  disabled={busy || countHistoryEvents(bundle) === 0}
                  onCheckedChange={(checked) =>
                    setSelection((current) => ({ ...current, history: checked }))
                  }
                  title={`History (${countHistoryEvents(bundle)} visits)`}
                  description="Adds visits to the current browser profile. Chromium assigns the import time as the visit time; original timestamps cannot be set through its extension API. URLs can contain sensitive query values."
                />
                <ImportOption
                  id="pip-import-settings"
                  checked={selection.settings}
                  disabled={busy || Object.keys(bundle.settings).length === 0}
                  onCheckedChange={(checked) =>
                    setSelection((current) => ({ ...current, settings: checked }))
                  }
                  title="Pip preferences"
                  description={(
                    <>
                      Applies a recognized light, dark, or system theme to the Pip Dev cockpit.
                      Other recognized preferences are stored locally for reference; Chromium browser
                      settings are unchanged.
                      <span lang="vi" className="mt-1 block">
                        Áp dụng giao diện sáng, tối hoặc theo hệ thống cho Pip Dev. Các tùy chọn được nhận diện khác chỉ lưu cục bộ để tham khảo; cài đặt trình duyệt Chromium không bị thay đổi.
                      </span>
                    </>
                  )}
                />
              </div>

              {bundle.omittedSensitiveSettings > 0 && (
                <p className="rounded-md bg-muted px-3 py-2 text-xs text-ink-2">
                  {bundle.omittedSensitiveSettings} sensitive or unsupported preference value(s) were omitted.
                </p>
              )}
            </CardContent>
          </Card>

          <Card className="border-amber-500/30">
            <CardHeader>
              <CardTitle>What this import can and cannot do</CardTitle>
              <CardDescription>
                Chromium extensions cannot create or switch browser profiles.
                Bookmarks from all Pip profiles are grouped in folders, history
                is added to the current profile, and the original profile names
                are saved locally. Website sessions, cookies, and saved
                passwords are not transferred. Importing the same file again
                adds another copy; existing bookmarks and history are not merged
                or replaced.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <label className="flex items-start gap-3 text-sm leading-relaxed">
                <Checkbox
                  checked={understood}
                  disabled={busy}
                  onCheckedChange={(checked) => setUnderstood(checked === true)}
                  aria-label="Confirm migration behavior"
                  className="mt-0.5"
                />
                <span>
                  I reviewed the selected data and understand that history is
                  merged into this browser profile, imported history dates
                  become the import time, and websites will ask me to sign in
                  again.
                </span>
              </label>
              <div className="flex flex-wrap items-center gap-3">
                <Button
                  type="button"
                  disabled={busy || !understood || !hasSelectedData}
                  onClick={() => void handleImport()}
                >
                  {busy ? 'Importing…' : 'Import selected data'}
                </Button>
                {!busy && !hasSelectedData && (
                  <span className="text-xs text-ink-2">Select at least one data type.</span>
                )}
              </div>
              {progress && (
                <p role="status" className="text-xs text-ink-2">
                  {progress.phase === 'bookmarks'
                    ? 'Importing bookmarks'
                    : progress.phase === 'history'
                      ? 'Importing history'
                      : 'Saving preferences'}
                  {progress.phase !== 'settings' && ` · ${progress.completed} / ${progress.total}`}
                </p>
              )}
            </CardContent>
          </Card>
        </>
      )}

      {error && (
        <p
          role="alert"
          className="rounded-md border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive"
        >
          {error}
        </p>
      )}

      {result && (
        <Card role="status">
          <CardHeader>
            <CardTitle>Import finished</CardTitle>
            <CardDescription>
              Added {result.bookmarks} bookmark(s) and {result.historyEvents}{' '}
              history event(s).
              {result.settingsSaved
                ? ' A valid Pip Dev cockpit theme was applied if present; other preferences were saved locally for reference.'
                : ''}
              {result.settingsSaved && (
                <span lang="vi" className="mt-1 block">
                  Đã áp dụng giao diện Pip Dev hợp lệ nếu có; các tùy chọn khác được lưu cục bộ để tham khảo.
                </span>
              )}
              {' '}Sign back in to websites; sessions were not transferred.
            </CardDescription>
          </CardHeader>
        </Card>
      )}

      {savedLocalData && (
        <Card>
          <CardHeader>
            <CardTitle>Saved Pip migration data on this device</CardTitle>
            <CardDescription>
              Profile labels and selected preference values stay in this extension's local storage. A
              recognized appearance theme is applied to the Pip Dev cockpit; other values remain local
              reference data and Chromium browser settings are unchanged.
              <span lang="vi" className="mt-1 block">
                Tên hồ sơ và các tùy chọn đã chọn được lưu trong bộ nhớ cục bộ của tiện ích. Giao diện hợp lệ được áp dụng cho Pip Dev; các giá trị khác chỉ để tham khảo và không đổi cài đặt Chromium.
              </span>
            </CardDescription>
          </CardHeader>
          <CardContent>
            <details>
              <summary className="cursor-pointer text-sm font-medium">View saved details</summary>
              <pre className="mt-3 max-h-80 overflow-auto rounded-md bg-muted p-3 text-xs leading-relaxed">
                {JSON.stringify(savedLocalData, null, 2)}
              </pre>
            </details>
          </CardContent>
        </Card>
      )}
    </div>
  )
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function ImportOption({
  id,
  checked,
  disabled,
  onCheckedChange,
  title,
  description,
}: {
  id: string
  checked: boolean
  disabled: boolean
  onCheckedChange: (checked: boolean) => void
  title: string
  description: ReactNode
}) {
  return (
    <label htmlFor={id} className="flex cursor-pointer items-start gap-3">
      <Checkbox
        id={id}
        checked={checked}
        disabled={disabled}
        onCheckedChange={(value) => onCheckedChange(value === true)}
        className="mt-0.5"
      />
      <span className="flex flex-col gap-1">
        <span className="font-medium text-sm">{title}</span>
        <span className="text-xs leading-relaxed text-ink-2">{description}</span>
      </span>
    </label>
  )
}
