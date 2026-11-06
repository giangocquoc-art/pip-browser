# Install Pip Dev on Windows

The repository currently contains Pip Dev's source and development launcher, not a finished Windows browser installer. Pip Dev does not have a public installer yet. Its planned versioned installer name is `PipDevSetup_v0.1.0_x64_installer.exe`; this is separate from the existing Pip browser installer, named `PipSetup`.

## Install the Codex plugin from GitHub

This installs Pip Dev Browser and its local MCP connection; it does not install the browser executable. From any folder, run:

```powershell
codex plugin marketplace add giangocquoc-art/pip-browser --sparse .agents/plugins --sparse pip-dev/plugins/pip-dev-browser
codex plugin add pip-dev-browser --marketplace pip-dev-plugins
codex plugin list
```

Start Pip Dev on the same PC before asking Codex to use its browser tools. Its local MCP server listens at `http://127.0.0.1:9200/mcp` by default.

Pip Dev's MCP server name is `pip_dev`. This repository does not provide a Pip Dev Claude Desktop `.mcpb` installer; use the local Streamable HTTP endpoint with a compatible MCP client. See the [agent connection guide](PIP_DEV_AGENT_SETUP.md) for client configuration and the Codex plugin.

## Run the development build

The Windows development launcher requires a Windows Pip Dev/Chromium `.exe`. From the `pip-dev` directory, run:

```powershell
.\run-pip-dev-windows.ps1 -BrowserBinary "D:\path\to\PipDev.exe" -InstallDependencies
```

The launcher keeps Cargo's registry/cache in the project-level `.cargo-home`, puts intermediate Cargo build artifacts in `packages/browseros-agent/.cargo-target`, and keeps Bun, npm, and temporary files in `packages/browseros-agent/.data/windows-dev`. It restores the caller's `CARGO_HOME`, `NPM_CONFIG_CACHE`, Bun cache variables, `TEMP`, and `TMP` when it exits. This isolated Cargo home does not inherit the Windows user's Cargo configuration, private registry credentials, or installed Cargo utilities; configure any private registry access in `.cargo-home` on the project drive before launching. Git ignores these local caches. The launcher also uses the repository's pinned Bun version through npm when Bun is not already installed, starts the server on `127.0.0.1:9200` by default, and launches the browser extension with an isolated development profile. Omit `-InstallDependencies` after the first successful dependency install. To use another MCP port, pass `-ServerPort <port>` and configure the agent to the same port.

The launcher limits Cargo to one parallel job by default to reduce disk contention during a first or incremental build on an HDD. Pass `-BuildJobs 2` only if you prefer a faster build and can accept more disk activity. The limit applies only to this Cargo build and the caller's `CARGO_BUILD_JOBS` value is restored afterward, including if the build fails. Existing Cargo caches and build artifacts remain under the project on the selected drive.

When the required artifacts already exist, skip Cargo and dependency installation:

```powershell
.\run-pip-dev-windows.ps1 -BrowserBinary "D:\path\to\PipDev.exe" -UseExistingArtifacts
```

This mode requires Bun 1.4.2 on PATH, existing `node_modules`, a prebuilt MCP server at
`packages/browseros-agent/target/debug/browseros-claw-server-rs.exe`, and the browser executable.
It checks those prerequisites, then skips `bun install` and Cargo build. It cannot confirm that
the server matches the current Rust source; use the normal launcher after changing Rust code or
dependencies. It still starts WXT, so extension compilation, cache/profile/runtime files and logs
may be written under the project. It is a lower-I/O smoke path, not a no-write mode. Do not combine
`-UseExistingArtifacts` with `-InstallDependencies`.

This launcher does not build the Chromium browser executable or produce a distributable installer. Those are separate release-build steps. See [Pip Dev implementation status](../PIP_DEV_STATUS.md) for the current Windows build status and [agent connection setup](PIP_DEV_AGENT_SETUP.md) to connect Codex or another MCP client.

When using a custom `-ServerPort`, update the MCP URL after installing the Codex plugin so it matches the running server. For example, if the launcher uses `-ServerPort 9201`, run:

```powershell
codex mcp add pip_dev --url http://127.0.0.1:9201/mcp
codex mcp list
```

This sets Codex's `pip_dev` endpoint to the custom port. For another MCP client, replace `9200` in its configured URL with the selected port.

## Connect Codex

From the repository root, add the repo marketplace and install **Pip Dev Browser**:

```powershell
codex plugin marketplace add .
codex plugin add pip-dev-browser --marketplace pip-dev-plugins
codex plugin list
```

For a direct MCP connection without the plugin:

```powershell
codex mcp add pip_dev --url http://127.0.0.1:9200/mcp
codex mcp list
```
