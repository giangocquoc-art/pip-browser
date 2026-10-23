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
  before and after capture against URL, viewport, scroll, visual-target node identities and bounds.
  Coordinate actions must land within one of those fresh observed visual regions and reject points
  covered by another element; ordinary DOM controls should use semantic element references.
  The MCP `visual_targets` tool declares a structured output schema with that token and the
  regions, so clients can pass the fresh token directly to `screenshot.region`; its legacy text
  region list remains.
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
  can appear together when both apply. While one session is paused for human help, its lock is
  released so the user can respond; another live session can keep the tab locked. Before the
  agent resumes, Pip Dev refreshes live owners and reacquires the lock. Resolving help is checked
  against the sender tab.
- Idle visible tabs poll session state every 15 seconds, active agent tabs every 3 seconds, and
  hidden tabs stop polling until they become visible. The help timer runs only while a help
  request is displayed.
- Pip Dev's takeover UI now reads native input-lock state from the claw-server live-session API,
  keyed to the current browser tab, page, target, and CDP connection epoch. It reports `locked`
  only after the server's BrowserOS native command is acknowledged; `paused`, `absent`, and
  `uncertain` remain distinct, and the extension's local CDP state is not authoritative. The
  server rejects page-scoped agent dispatch when it cannot confirm the required lock.
- The active `D:\src` tree now contains the native `Input.setAgentInputLock` command,
  `WebContentsImpl::AcquireNativeInputLock`, and Aura gates for mouse, keyboard, scroll, touch,
  and gesture input. Source presence is verified, but its runtime behavior is not: no Pip Dev
  Chromium executable has been produced yet. Do not treat a native lock as active or claim that
  physical input is blocked until it is tested in the running browser. The UI only makes that
  claim when the server reports an acknowledged `locked` state.
- The existing Pip data directory can be exported with `tools/export-pip-dev-migration.py`.
  The Pip Dev Settings migration screen previews a versioned JSON export, validates URLs and
  folder trees, requires explicit selection/confirmation, imports bookmarks into named folders,
  and makes history import opt-in. Cookies, credentials and passwords are excluded.
- The Windows PowerShell launcher keeps Cargo's registry/cache in the ignored project-level
  `.cargo-home`, Cargo build output in `packages/browseros-agent/.cargo-target`, and Bun/npm/runtime
  caches plus temporary files in `packages/browseros-agent/.data/windows-dev`; it restores the
  caller's Cargo, npm, Bun, and temp environment values after exiting. Custom Cargo registry
  configuration must be provisioned in that D-only home. It then builds and starts the neo MCP/API
  server, waits for its health endpoint, and loads the WXT extension in a Windows Chromium exe with
  an isolated dev profile.

## Important limits and remaining work

- Historical Windows Chromium build snapshot (2026-10-23): the lower-memory incremental `ninja -j 2 -C D:\src\out\PipDevPortableSDK chrome` build was observed at 731/7,272 steps and stopped then to reduce D HDD load; the Ninja/Clang/LLD process tree had exited at that snapshot. As of this documentation update (2026-10-24), the current process and artifact state on `D:\src` has not been rechecked and is unknown. The prior snapshot reported `chrome.exe`, `chrome.dll`, `mini_installer.exe`, and `initialexe/chrome.exe` absent, with GN args `is_debug=true`, `enable_updater=false`, and `enable_winsparkle=false`; those historical settings describe a debug `chrome` target, not a distributable installer. Chromium source, sysroot, build output, and logs are local D-drive state and are not included in this repository.
- Upstream prebuilt test option checked 2026-10-23: the official [BrowserOS neo v0.51.1.2 release](https://github.com/browseros-ai/BrowserOS/releases/tag/browserclaw%2Fv0.51.1.2) includes a Windows x64 installer (137,344,056 bytes) and ZIP (137,355,879 bytes), built on Chromium 155.0.8059.26. If used, it can check process startup, extension loading, MCP health, and `tools/list`; it cannot verify page-scoped actions or crops because this release predates Pip Dev's required native input-lock command. It also cannot verify Pip Dev branding. The archive has not been downloaded; keep it as a limited smoke-test option only if HDD load permits.
- Hand-back safety on PR #19: the extension background requires an acknowledged native input lock before resolving help, then reconciles current live-tab owners after a false or rejected resolve. Tests cover unavailable/throwing lock acquisition, a false response unlocking a paused-only tab, and a deferred resolve failure retaining another live owner. The HTTP `sessions.rs::resolve_help` endpoint remains session-ID-only and has no exact tab/page/target/connection-epoch rollback guard; if the extension background exits mid-request, cleanup relies on Chromium debugger-session teardown, which still needs runtime verification. The checked-in Aura test asserts physical mouse/key callbacks are handled while locked and the lock state clears after release; it does not assert DevTools synthetic dispatch or physical event delivery after unlock. Native enforcement remains unverified until Pip Dev Chromium runs.
- Low-I/O CI coverage is now defined in `.github/workflows/verify-pip-dev.yml` for Rust core/MCP/server tests, Pip Dev extension tests/typecheck, and Claw API generated-schema contracts. The full Claw MCP browser contract suite is not run there: it requires a custom BrowserOS executable with Pip Dev's native lock command, which is not available until the Chromium target is built.

- Fresh coordinate actions now require an `observationId` from `visual_targets`; tokens are bound to the page, target, CDP connection epoch, and current layout. The `run` helper exposes `page.visualTargets()` and passes the token through click/type/hover/drag helpers. Missing or stale tokens are rejected before dispatch. Rust unit and MCP contract coverage was added; Rust formatting, Biome, and targeted TypeScript checks passed, but the new Cargo and Bun tests have not yet run in this checkout. A local Bun 1.4.2 bootstrap was attempted with npm and D-only cache/temp paths; npm stalled while resolving registry metadata, so the command was stopped before extension tests ran. Native input-lock transitions now mark state uncertain before awaiting Chromium, schedule exact-page reconciliation on cancellation, and keep `release_owner` cleanup pinned to the exact snapshot it observed. After the latest generation/cancellation edits, `rustfmt --check` and the cancellation regression tests remain unverified; those checks were stopped to reduce D HDD load.

- The current `out/PipDevPortableSDK/args.gn` disables both `enable_updater` and `enable_winsparkle`; it only builds the `chrome` target and is not a distributable package configuration. BrowserOS's Windows package step requires `mini_installer.exe` and `WinSparkle.dll`. After runtime checks pass, regenerate with the Pip Dev Windows release configuration and build the package targets. Pip Dev's artifact prefix is now `PipDevSetup`, matching the Windows install guide; descriptor and context tests verify the installer and DMG names.

- The Rust canvas/crop regression subset passed all 6 tests on D. The repository Cargo config places its shared `build-dir` under Cargo's home by default; to keep this Windows build on D, use:

  ```powershell
  cargo --config 'build.build-dir="D:/pip-browser-work/pip-dev/packages/browseros-agent/.cargo-target"' test -p browseros-mcp visual
  ```

- Windows MCP smoke check on 2026-10-21 used the existing Rust server binary with official
  Chrome for Testing 155 headless: `/system/health` returned 200 and `tools/list` exposed
  22 tools, but `tabs(action=list)` failed with `CDP error: 'Browser.getTabs' wasn't found`.
  This confirms generic Chrome lacks the BrowserOS tab API; Pip Dev needs its patched
  Chromium build for browser operations.
- Windows validation on 2026-10-20: the Chromium 155 source sync completed and all 402 BrowserOS patches applied. Microsoft SDK 10.0.28000.2270, MSVC 14.44, and the matching ATL headers/libraries were assembled into a D-contained portable sysroot. GN generated the 35,573-target graph, and `autoninja -C out\PipDevPortableSDK base` completed all 2,175 build steps and produced `base.dll.lib`. This verifies Chromium core can compile against the staged Windows toolchain. The full browser target, Pip Dev executable, installer, and runtime flows are still unbuilt/unverified. The Chromium checkout, portable sysroot, and GN output are local files on D and are not included in the GitHub repository.
- The toolbar emergency-stop popup and extension routing are implemented. The production WXT
  extension build and TypeScript check pass; 34 focused takeover tests pass. Browser runtime
  behavior remains unverified until the full Windows browser target is built. The native command
  and input gate still need Windows runtime verification; until then the bar must show that the
  tab is unlocked. BrowserOS's cancel API stops the whole agent session, which may be using other
  tabs too.
- Chromium's extension APIs cannot create or switch browser profiles or restore original
  history timestamps. Imported bookmarks are separated by Pip profile folders; history is added
  to the current profile. A valid Pip `settings.appearance.theme` value (`light`, `dark`, or
  `system`) is applied to the Pip Dev cockpit. Other recognized preferences remain local
  reference data and do not change Chromium browser settings. Website sessions are not
  transferred; users sign in again.
- After the browser target finishes, verify Windows MCP/browser operation, DevTools, source maps,
  Network and Performance panels, DOM and canvas flows, tab lifecycle failures and lock release.
  The launcher is still development-only; package a Windows build only after those runtime checks
  pass.
- BrowserOS neo's upstream Go development supervisor depends on Unix process groups, `flock`,
  `lsof` and shell commands; use the PowerShell launcher on Windows.

## Windows development

Run `run-pip-dev-windows.ps1 -BrowserBinary "D:\path\to\BrowserOS.exe"` with a Windows Pip Dev/BrowserOS Chromium executable. The default launcher builds the BrowserOS neo Rust server, installs dependencies when needed, and starts the extension through WXT with a dedicated profile. It uses Bun 1.4.2 from PATH or invokes that pinned version through npm; Rust MSVC is required for this default build. `-InstallDependencies` forces a frozen-lockfile install, and `-BuildJobs` defaults to one to limit concurrent Cargo disk activity.

After a successful full launch, `-UseExistingArtifacts` is an explicit fast-start option. It requires Bun 1.4.2 directly on PATH, the existing `node_modules` directory, the browser `.exe`, and `packages/browseros-agent/target/debug/browseros-claw-server-rs.exe`. It checks their presence and skips Bun dependency installation, Rust/Cargo prerequisite checks, and Cargo compilation. It cannot verify that the server binary matches current source, so rerun the default launcher after Rust source or dependency changes to avoid a stale server. The option cannot be combined with `-InstallDependencies`.

This mode is not no-write: WXT, browser profile, runtime state, temporary files, configuration and logs still write to the project. It only avoids dependency installation and Cargo compilation. The default launcher uses an isolated Cargo home and build directory; Cargo outputs, WXT caches and runtime files still write to D. This remains a development loop, not an installer. The extension build has been verified, but the full browser runtime has not because this checkout has no Pip Dev/BrowserOS Chromium executable.

To create the migration file on Windows, run from the repository root:

```powershell
python tools\export-pip-dev-migration.py --output "$env:USERPROFILE\Downloads\pip-dev-migration.json"
```

The exporter reads `%APPDATA%\evergreen-browser` by default; use `--data-dir` if Pip is in
portable mode or stored elsewhere. Treat the JSON as personal browsing history before moving
it, then select it in Pip Dev Settings → Import Pip data.
