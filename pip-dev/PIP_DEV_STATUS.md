# Pip Dev implementation status

Pip Dev is developed beside the original Pip browser. This directory vendors BrowserOS at
commit `1cccd53b8be7b220ce1cf793f20bf42274a38ba0` (2026-10-19). The upstream source is
licensed under AGPL-3.0-or-later. Keep its license and notices when redistributing, and
publish corresponding source and modifications as required by that license.

See [Connect Codex and other agents](docs/PIP_DEV_AGENT_SETUP.md) for the MCP endpoint,
Codex plugin/repo-marketplace setup, manual client configuration, and privacy limits. The
[Windows installation guide](docs/INSTALL_WINDOWS.md) distinguishes the current developer
launcher from a future packaged release.

## Implemented in this checkout

- The BrowserOS product and extension are branded Pip Dev, including Chromium product icons,
  extension icons, cockpit branding and the in-page agent status bar.
- BrowserOS neo's existing MCP service provides tab selection, DOM snapshots, stable element
  references, actions, waits, diffs and multi-step workflows.
- MCP `visual_targets` finds visible canvas, image, video, SVG and `role=img` regions without
  capturing the page. `screenshot.region` only accepts a crop fully contained within one of the
  regions returned by that exact observation. It returns one visible-viewport crop inline, limited
  to 1 megapixel, and does not save it. Each crop requires the matching `observationId`, checked
  before and after capture against URL, viewport, scroll and returned visual-target bounds. MCP
  `visual_targets` declares a structured output schema with that token and the regions, so clients
  can pass the fresh token directly to `screenshot.region`; its legacy text region list remains.
  Tool metadata warns that the crop is delivered to the caller and may be forwarded to an external
  model; the browser cannot determine where the MCP client sends it next.
- Layout changes invalidate old crop coordinates. The observation token does not detect pixel
  changes inside a same-sized canvas; callers must inspect the crop and verify application
  state after every action. It does not claim universal OCR or game-state understanding.
- Canvas support currently exposes an on-demand image crop to the calling agent; no local OCR,
  board-state recognizer, or accuracy/latency benchmark has been added yet.
- A tab present in a live agent session shows a Pip Dev status bar. Its stop action is scoped
  to the content-script sender tab, fetches fresh live sessions, and cancels sessions that
  include that tab. Because the Chromium native lock suppresses page input, the in-page stop
  cannot be clicked while that lock is active. The extension toolbar now opens a separate
  stop popup that remains reachable from browser chrome; it captures the active tab/window,
  and the background accepts commands only from the extension popup URL and revalidates that
  the same tab is active before polling or stopping. The popup uses the same fresh-session
  cancellation path, reports partial/failing stops, and only releases the lock when every
  matching session is canceled or no live session remains. A human-help request and status bar
  can appear together when both apply. Resolving help is checked against the sender tab.
- Idle visible tabs poll session state every 15 seconds, active agent tabs every 3 seconds, and
  hidden tabs stop polling until they become visible. The help timer runs only while a help
  request is displayed.
- The extension now asks Chromium for a per-tab native input-lock lease while an agent session
  controls that tab, and releases it when control ends, the user stops the session, or the tab
  closes. It reports `locked` only after Chromium confirms the custom CDP command; older or
  unpatched builds show an unlocked warning instead of claiming exclusive control.
- The existing Pip data directory can be exported with `tools/export-pip-dev-migration.py`.
  The Pip Dev Settings migration screen previews a versioned JSON export, validates URLs and
  folder trees, requires explicit selection/confirmation, imports bookmarks into named folders,
  and makes history import opt-in. Cookies, credentials and passwords are excluded.
- The Windows PowerShell launcher checks prerequisites, builds and starts the neo MCP/API
  server, waits for its health endpoint, then loads the WXT extension in an existing Windows
  Chromium exe using an isolated dev profile.

## Important limits and remaining work

- As of 2026-10-21, the full Windows `chrome` target is compiling from the D-only Chromium checkout. Its generated GN graph contains 48,959 actions; the Pip Dev browser executable has not been produced yet. The checkout, portable sysroot and build output remain local on D.

- Windows MCP smoke check on 2026-10-21 used the existing Rust server binary with official
  Chrome for Testing 155 headless: `/system/health` returned 200 and `tools/list` exposed
  22 tools, but `tabs(action=list)` failed with `CDP error: 'Browser.getTabs' wasn't found`.
  This confirms generic Chrome lacks the BrowserOS tab API; Pip Dev needs its patched
  Chromium build for browser operations.
- Windows validation on 2026-10-20: the Chromium 155 source sync completed and all 402 BrowserOS patches applied. Microsoft SDK 10.0.28000.2270, MSVC 14.44, and the matching ATL headers/libraries were assembled into a D-contained portable sysroot. GN generated the 35,573-target graph, and `autoninja -C out\PipDevPortableSDK base` completed all 2,175 build steps and produced `base.dll.lib`. This verifies Chromium core can compile against the staged Windows toolchain. The full browser target, Pip Dev executable, installer, and runtime flows are still unbuilt/unverified. The Chromium checkout, portable sysroot, and GN output are local files on D and are not included in the GitHub repository.
- The toolbar emergency-stop popup and extension routing are implemented. Unit tests cover
  trusted sender validation, active-tab revalidation, cancellation outcomes, and lock release.
  The WXT popup build and browser runtime behavior remain unverified while the full Windows
  browser target is being built. The native
  command and input gate still need a complete Windows Chromium build and runtime verification;
  until then the bar must show that the tab is unlocked. BrowserOS's cancel API stops the whole
  agent session, which may be using other tabs too.
- Chromium's extension APIs cannot create or switch browser profiles or restore original
  history timestamps. Imported bookmarks are separated by Pip profile folders; history is added
  to the current profile. Recognized Pip preferences are saved locally for reference but are
  not yet applied to Chromium settings. Website sessions are not transferred; users sign in
  again.
- After the browser target finishes, verify Windows MCP/browser operation, DevTools, source maps,
  Network and Performance panels, DOM and canvas flows, tab lifecycle failures and lock release.
  The launcher is still development-only; package a Windows build only after those runtime checks
  pass.
- BrowserOS neo's upstream Go development supervisor depends on Unix process groups, `flock`,
  `lsof` and shell commands; use the PowerShell launcher on Windows.

## Windows development

Run `run-pip-dev-windows.ps1 -BrowserBinary "D:\path\to\BrowserOS.exe"` with a Pip
Dev/BrowserOS Chromium executable built for Windows. The launcher builds the BrowserOS neo Rust
server and starts the extension through WXT with a dedicated profile. It uses Bun 1.4.2 from PATH,
or invokes that pinned version through npm; Rust MSVC is required. `-InstallDependencies` installs
the locked `@browseros/claw-app` workspace dependencies. This is a development loop, not an
installer. The extension build has been verified, but the full browser runtime has not because
this checkout has no Pip Dev/BrowserOS Chromium executable.

To create the migration file on Windows, run from the repository root:

```powershell
python tools\export-pip-dev-migration.py --output "$env:USERPROFILE\Downloads\pip-dev-migration.json"
```

The exporter reads `%APPDATA%\evergreen-browser` by default; use `--data-dir` if Pip is in
portable mode or stored elsewhere. Treat the JSON as personal browsing history before moving
it, then select it in Pip Dev Settings → Import Pip data.
