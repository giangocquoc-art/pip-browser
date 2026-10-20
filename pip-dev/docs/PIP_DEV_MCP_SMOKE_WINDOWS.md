# Pip Dev Windows MCP smoke test

This smoke test checks the Windows server, Pip Dev extension, patched Chromium CDP integration, and a local DOM/canvas page together. It does not build Chromium or the Rust server, install software, or read normal browser profiles. The fixture and test navigation use loopback only; background browser networking is disabled. Each run writes its isolated profile, server state, logs, and process temp files under `D:\pc\pipdev-temp\pipdev-mcp-smoke-*`.

## Prerequisites

- The custom Pip Dev/BrowserOS `chrome.exe` has finished building. Use that executable, not Chrome or Edge installed for normal browsing: the native browser API used by `tabs` includes BrowserOS-specific CDP methods.
- The Rust MCP server binary exists at `packages/browseros-agent/target/debug/browseros-claw-server-rs.exe` (or pass `-ServerExe`). The smoke script does not invoke Cargo.
- The unpacked Pip Dev extension has been built and its directory contains `manifest.json`. From `D:\pip-browser-work\pip-dev\packages\browseros-agent`, build it with the repository's pinned Bun installation:

  ```powershell
  bun install --frozen-lockfile
  bun run --filter '@browseros/claw-app' build:dev
  ```

- If WXT placed the extension outside `apps/claw-app/dist/chrome-mv3-dev` or `apps/claw-app/dist/chrome-mv3`, pass that directory using `-ExtensionDirectory`.
- The default loopback ports 9200 and 49337 are free. If either is occupied, pass alternate `-ServerPort` and `-CdpPort` values.

## Run

From `D:\pip-browser-work\pip-dev`:

```powershell
& .\tools\test-pip-dev-mcp-windows.ps1 `
  -BrowserBinary 'D:\path\to\custom\chrome.exe' `
  -ExtensionDirectory '.\packages\browseros-agent\apps\claw-app\dist\chrome-mv3-dev' `
  -ShowBrowser
```

The script starts a loopback-only fixture, the prebuilt Rust server, and the specified custom browser with a fresh user-data directory. It negotiates MCP Streamable HTTP, checks health and browser readiness, lists tools, creates and selects the fixture tab, reads a DOM snapshot, clicks by a fresh element reference, and verifies the resulting diff and state. It then checks that `visual_targets` sees the fixture canvas and, when the current MCP response exposes its `observationId`, requests one bounded inline crop. Use `-RequireCrop` to make missing crop-token exposure a failing gate. No crop bytes are written to a file by the test.

The script terminates only its own server/browser process trees and its own fixture job. On success it removes only its uniquely named run directory. Failures retain that run directory for inspection; `-KeepArtifacts` also retains it after success. It never calls `Stop-Process` by a browser process name or modifies a persistent browser profile.

## What still needs the custom browser

Until the custom `chrome.exe` exists, source tests can verify server and tool logic but cannot prove a real Windows browser connection, BrowserOS-specific tab enumeration, native stable element references, screenshot crop pixels, extension loading in Pip Dev, or recovery behavior against actual renderer/layout changes. Generic Chrome is not an equivalent substitute: an earlier headless smoke reached `/system/health` and `tools/list`, but `tabs(action=list)` failed because `Browser.getTabs` is not implemented there.

`visual_targets` currently returns region JSON as text. The MCP server only forwards structured tool output for tools that advertise an output schema, and `visual_targets` does not yet advertise one. Therefore the crop test will report `SKIP` until `observationId` is exposed on the MCP wire; `-RequireCrop` can be used to enforce that once fixed. This is an API output limitation, separate from whether the browser can capture a target-bounded crop.
