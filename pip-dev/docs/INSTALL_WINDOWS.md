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

The launcher keeps Cargo's registry/cache in the project-level `.cargo-home`, puts Cargo build output in `packages/browseros-agent/.cargo-target`, and directs build temporary files to `packages/browseros-agent/.data/windows-dev/temp`. It restores the caller's `CARGO_HOME`, `TEMP`, and `TMP` values when it exits. Cargo builds use one job by default to reduce concurrent disk activity on an HDD; pass `-BuildJobs 2` if you prefer a faster build and can accept more disk activity. The job limit applies only while Cargo runs, and the previous `CARGO_BUILD_JOBS` value is restored even if the build fails. This isolated Cargo home does not inherit the Windows user's Cargo configuration, private registry credentials, or installed Cargo utilities; configure any private registry access in `.cargo-home` on the project drive before launching. Git ignores this cache directory. The launcher also uses the repository's pinned Bun version through npm when Bun is not already installed, starts the server on `127.0.0.1:9200` by default, and launches the browser extension with an isolated development profile. Omit `-InstallDependencies` after the first successful dependency install. To use another MCP port, pass `-ServerPort <port>` and configure the agent to the same port.

After a successful full launch has created the required artifacts, you can skip dependency installation and Cargo compilation on later runs:

```powershell
.\run-pip-dev-windows.ps1 -BrowserBinary "D:\path\to\PipDev.exe" -UseExistingArtifacts
```

This opt-in mode requires Bun 1.4.2 already on PATH, the existing `node_modules` directory, the server executable at `packages/browseros-agent/target/debug/browseros-claw-server-rs.exe`, and the selected browser `.exe`. It checks that these files exist, but cannot verify that the server binary matches the current Rust source. Rebuild with the default launcher after changing Rust code or dependencies to avoid running a stale server. Do not combine `-UseExistingArtifacts` with `-InstallDependencies`. The mode still writes the WXT build/cache, browser profile, runtime state, temporary files, configuration and logs to the project; it only skips dependency installation and Cargo build.

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
