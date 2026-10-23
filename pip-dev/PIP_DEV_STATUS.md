# Pip Dev implementation status

> Status snapshot: 2026-10-24, reviewed against `main` commit `94973eb7966c87a91c68b872e2b47fe5603459e3` (PR #43 merge). Main verification run [#113](https://github.com/giangocquoc-art/pip-browser/actions/runs/40038748073) is in progress; earlier CI runs [#59](https://github.com/giangocquoc-art/pip-browser/actions/runs/40033543130) and [#60](https://github.com/giangocquoc-art/pip-browser/actions/runs/40034260931) passed. This document records repository and CI evidence; dated D-drive observations below are historical snapshots, not statements about current local storage.

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
- DOM element references fail closed when their captured backend node is detached: `act` returns a stale-ref error directing the agent to take a new snapshot. Transient CDP failures propagate instead of silently rematching by role/name/occurrence. Source tests cover detached replacements and duplicate same-label controls; this package-level evidence does not verify a running Pip Dev browser.
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
- Historical native-lock source note: an earlier inspection of local `D:\src` reported `Input.setAgentInputLock`, `WebContentsImpl::AcquireNativeInputLock`, and Aura gates for mouse, keyboard, scroll, touch, and gesture input. That is a historical source observation only; it does not establish the present contents of any D-drive checkout. Native lock behavior has not been verified in a running Pip Dev Chromium browser, so physical input blocking, unlock delivery, and synthetic-input behavior remain unverified.
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

- Historical Windows Chromium build snapshot (2026-10-23): an incremental `ninja -j 2 -C D:\src\out\PipDevPortableSDK chrome` build was observed at 731/7,272 steps and stopped to reduce disk load; the Ninja/Clang/LLD process tree had exited at that snapshot. That report listed `chrome.exe`, `chrome.dll`, `mini_installer.exe`, and `initialexe/chrome.exe` as absent and GN args `is_debug=true`, `enable_updater=false`, and `enable_winsparkle=false`. These are historical observations only; this document makes no claim about present D-drive processes, files, or build configuration. The noted `chrome` target was a debug build, not a distributable installer. Chromium source, sysroot, build output, and logs are outside this repository.
- Upstream prebuilt test option checked 2026-10-23: the official [BrowserOS neo v0.51.1.2 release](https://github.com/browseros-ai/BrowserOS/releases/tag/browserclaw%2Fv0.51.1.2) included a Windows x64 installer (137,344,056 bytes) and ZIP (137,355,879 bytes), built on Chromium 155.0.8059.26. At that dated check, the archive had not been downloaded. If tested, it can check process startup, extension loading, MCP health, and `tools/list`; it predates Pip Dev's native input-lock command and branding, so it cannot validate page-scoped actions, crops, or Pip Dev branding. No current D-drive archive state is asserted.
- Human-help hand-back is guarded by the merged PR #39. The HTTP resolve request requires the current `requestId` and `browserTabId`; when the request opens, the server pins its browser tab, PageId, CDP TargetId, and connection epoch. Before resolving, it checks the supplied request/tab against that live request and verifies the same pinned page incarnation. Stale, duplicate, missing, or mismatched identity returns `resolved: false`, and stale cockpit requests are hidden. The page and target identifiers remain server-side. This is source-and-test evidence for the HTTP hand-back path; it does not prove native input enforcement in a running browser.
- A previously reported Aura test checks that mouse/key callbacks are handled while the native lock is held and that the lock state clears on release. It does not establish actual physical event delivery after unlock or DevTools synthetic-dispatch behavior. Neither CI #59 nor #60 launches Pip Dev Chromium or runs a physical-input runtime test.
- Automated source checks passed on both PR head CI #59 (`22b2550bf989a09db73396c3a8836105ddb90131`) and post-merge main CI #60 (`8d3ad0ca1631fc8970f0176de30125eeec110977`). The Windows job ran `cargo fmt --all -- --check`, launcher parsing and existing-artifact preflight checks, `cargo test -p browseros-core -p browseros-mcp -p claw-server --locked`, and Pip Dev extension tests. The codegen job verified generated Claw API files, API contract tests, and extension type-checking. These green runs clear the earlier note that the recent Rust/Bun tests, cancellation regressions, and rustfmt were still unverified for the merged source. They validate the selected repository packages, not a built Chromium browser or a packaged installer.
- The full Claw MCP browser contract suite is not run by this workflow: it requires a custom BrowserOS executable with Pip Dev's native lock command. Canvas/crop unit and MCP contract checks do not validate recognition of arbitrary live canvas content.
- Fresh coordinate actions require an `observationId` from `visual_targets`; tokens are bound to the page, target, CDP connection epoch, and current layout. The `run` helper exposes `page.visualTargets()` and passes the token through click/type/hover/drag helpers. Missing or stale tokens are rejected before dispatch. Rust unit and MCP contract coverage is included in the successful CI #59/#60 package test runs. The token does not detect pixel changes inside a same-sized canvas; callers still need to inspect crops and verify application state after actions.
- Historical packaging-configuration note: a prior local check of `out/PipDevPortableSDK/args.gn` reported `enable_updater=false` and `enable_winsparkle=false` and only the `chrome` target being built. That note does not describe present D-drive configuration. BrowserOS packaging requires `mini_installer.exe` and `WinSparkle.dll`; a distributable Pip Dev package has not been verified by CI. The Pip Dev artifact prefix is `PipDevSetup`, matching the Windows install guide; descriptor and context tests cover installer and DMG names.
- A prior local run reported six Rust canvas/crop regression tests passing. The current CI #59/#60 package-level Rust and MCP tests also pass, but neither result proves accurate interpretation of live canvas pixels, board state, or arbitrary image content. No current D-drive test state is asserted.

- Windows MCP smoke check on 2026-10-21 used the existing Rust server binary with official
  Chrome for Testing 155 headless: `/system/health` returned 200 and `tools/list` exposed
  22 tools, but `tabs(action=list)` failed with `CDP error: 'Browser.getTabs' wasn't found`.
  This confirms generic Chrome lacks the BrowserOS tab API; Pip Dev needs its patched
  Chromium build for browser operations.
- Historical Windows validation (2026-10-20): a Chromium 155 source sync completed and 402 BrowserOS patches applied. Microsoft SDK 10.0.28000.2270, MSVC 14.44, and matching ATL headers/libraries were staged into a D-contained portable sysroot. GN generated 35,573 targets, and `autoninja -C out\PipDevPortableSDK base` completed 2,175 build steps and produced `base.dll.lib`. This documents that dated core-build snapshot only; it does not establish the current D-drive checkout or artifacts. The full browser target, Pip Dev executable, installer, and runtime flows were not yet verified in that snapshot. Chromium checkout, sysroot, and GN output are outside this repository.
- The toolbar emergency-stop popup and extension routing are implemented. The popup stays reachable from browser chrome, and its background revalidates the active tab before polling or stopping fresh live sessions; it reports partial/failing stops and releases the lock only when no matching live session remains. The WXT extension test suite and type check pass in CI #59/#60; 34 focused takeover tests were also reported in earlier validation. Browser runtime behavior remains unverified because these checks do not build or launch the Windows Chromium target. Native command/input-gate behavior still needs runtime verification; until then, the bar must show the tab as unlocked. BrowserOS cancellation stops the whole agent session, which may be using other tabs too.
- Chromium's extension APIs cannot create or switch browser profiles or restore original
  history timestamps. Imported bookmarks are separated by Pip profile folders; history is added
  to the current profile. A valid Pip `settings.appearance.theme` value (`light`, `dark`, or
  `system`) is applied to the Pip Dev cockpit. Other recognized preferences remain local
  reference data and do not change Chromium browser settings. Website sessions are not
  transferred; users sign in again.
- Remaining runtime validation: once an appropriate Pip Dev Chromium target is available, verify Windows MCP/browser operation, DevTools, source maps, Network and Performance panels, DOM and canvas flows, tab lifecycle failures, and lock release. The repository CI workflow does not build or package Chromium; the launcher remains a development flow, and no distributable Pip Dev installer is verified.
- BrowserOS neo's upstream Go development supervisor depends on Unix process groups, `flock`,
  `lsof` and shell commands; use the PowerShell launcher on Windows.

## Windows development

Run `run-pip-dev-windows.ps1 -BrowserBinary "D:\path\to\BrowserOS.exe"` with a Windows Pip Dev/BrowserOS Chromium executable. The default launcher builds the BrowserOS neo Rust server, installs dependencies when needed, and starts the extension through WXT with a dedicated profile. It uses Bun 1.4.2 from PATH or invokes that pinned version through npm; Rust MSVC is required for this default build. `-InstallDependencies` forces a frozen-lockfile install, and `-BuildJobs` defaults to one to limit concurrent Cargo disk activity.

After a successful full launch, `-UseExistingArtifacts` is an explicit fast-start option. It requires Bun 1.4.2 directly on PATH, the existing `node_modules` directory, the browser `.exe`, and `packages/browseros-agent/target/debug/browseros-claw-server-rs.exe`. It checks their presence and skips Bun dependency installation, Rust/Cargo prerequisite checks, and Cargo compilation. It cannot verify that the server binary matches current source, so rerun the default launcher after Rust source or dependency changes to avoid a stale server. The option cannot be combined with `-InstallDependencies`.

This mode is not no-write: WXT, browser profile, runtime state, temporary files, configuration and logs still write to the project. It only avoids dependency installation and Cargo compilation. The default launcher uses an isolated Cargo home and build directory; Cargo outputs, WXT caches and runtime files still write to D. This remains a development loop, not an installer. The extension build and source tests are verified in CI, but the workflow does not produce a Pip Dev/BrowserOS Chromium executable, so full browser runtime remains unverified.

To create the migration file on Windows, run from the repository root:

```powershell
python tools\export-pip-dev-migration.py --output "$env:USERPROFILE\Downloads\pip-dev-migration.json"
```

The exporter reads `%APPDATA%\evergreen-browser` by default; use `--data-dir` if Pip is in
portable mode or stored elsewhere. Treat the JSON as personal browsing history before moving
it, then select it in Pip Dev Settings → Import Pip data.
