# Connect Codex and other agents to Pip Dev

Pip Dev is the browser application. **MCP is the connection protocol** that lets Codex or another compatible agent read and control its tabs. The Codex plugin in this repository packages that MCP connection and a reusable browser workflow; installing the plugin does not install or start Pip Dev.

## Start Pip Dev

Install or build Pip Dev for Windows, then start it and open the pages the agent should use. The MCP service must be running. Its default development address is:

```text
http://127.0.0.1:9200/mcp
```

If Pip Dev displays a different MCP address, use that address in the steps below. Keep the endpoint on loopback; it is a local browser-control interface, not a public service.

If you start the Windows launcher with a custom `-ServerPort`, update the port in the plugin's `mcp.json` before installing it, or use the direct Codex MCP command below with that port.

For manual client configuration, use the MCP server name **`pip_dev`** and the
Streamable HTTP endpoint shown by Pip Dev. The default development endpoint is
`http://127.0.0.1:9200/mcp`. Pip Dev supplies browser-use instructions in the MCP
initialize response, so a separate `browseros-neo` skill or `.mcpb` extension is
not part of Pip Dev setup. Codex users can install the Pip Dev plugin for its
additional browser workflow skill.

## Install in Codex

### Repo marketplace

For a local checkout, run these commands from the Pip repository root:

```sh
codex plugin marketplace add .
codex plugin add pip-dev-browser --marketplace pip-dev-plugins
codex plugin list
```

The marketplace entry lives at [`.agents/plugins/marketplace.json`](../../.agents/plugins/marketplace.json); the plugin package is under [`plugins/pip-dev-browser`](../plugins/pip-dev-browser). This repository is a GitHub repo marketplace and does not add Pip Dev to a public Codex or ChatGPT plugin directory.

Install the repo marketplace from any machine with Codex CLI. The command below points to `giangocquoc-art/pip-browser`:

```sh
codex plugin marketplace add giangocquoc-art/pip-browser --sparse .agents/plugins --sparse pip-dev/plugins/pip-dev-browser
codex plugin add pip-dev-browser --marketplace pip-dev-plugins
codex plugin list
```

The two sparse paths include both the marketplace catalog and the plugin package it points to. If Pip Dev moves to a different GitHub repository, replace `giangocquoc-art/pip-browser` with that repository's `OWNER/REPO` value.

After installing, restart or refresh Codex and confirm the `pip_dev` MCP server is available. If the plugin did not add the MCP connection for the installed Codex client, run:

```sh
codex mcp add pip_dev --url http://127.0.0.1:9200/mcp
codex mcp list
```

To remove the manual connection later, run `codex mcp remove pip_dev`.

### Direct MCP connection

For Codex CLI, Codex IDE, or another MCP client, connect its Streamable HTTP transport to `http://127.0.0.1:9200/mcp`. Codex CLI can add it with `codex mcp add` as above. For a client that uses a JSON config, the equivalent entry is:

```json
{
  "mcpServers": {
    "pip_dev": {
      "type": "http",
      "url": "http://127.0.0.1:9200/mcp"
    }
  }
}
```

Keep the server name `pip_dev` if you want the bundled Pip Dev browser workflow to refer to it consistently. Other MCP clients may call the transport `streamable-http` instead of `http`; follow the client's own configuration format.

## Use it

Ask the agent to inspect the intended Pip Dev tab, for example:

> Inspect the localhost page open in Pip Dev and tell me which button submits the form. Do not submit it.

The agent should identify the tab, read its structured state, and use fresh element references. `visual_targets` returns the current `observationId` and `regions` in MCP structured output (while retaining its text region list for older clients). For canvas-only content, pass that token to request a crop fully inside one of the returned regions; arbitrary viewport crops are rejected. It should verify important state after an action and stop when the result is uncertain. Follow the stop/release control in Pip Dev when you want your tab back.

## Marketplace scope

This repository includes a **GitHub repo marketplace** for Codex CLI. It is not a listing in the public Codex/ChatGPT plugin directory.

Public-directory submission is separate. OpenAI's [desktop-only submission process](https://developers.openai.com/plugins/deploy/desktop-only) requires submission access, a packaged local `stdio` launcher with its dependencies, and review. Pip Dev currently uses loopback Streamable HTTP, so it is not ready for that listing. Use this GitHub repo marketplace for Codex or connect the local MCP endpoint directly.

## Privacy

The MCP connection controls pages in the local Pip Dev browser. Page content and focused crops are available to the connected agent; a client may send content to its model provider. Avoid connecting an agent you do not trust. Crops are on-demand and are not saved by Pip Dev by default.
