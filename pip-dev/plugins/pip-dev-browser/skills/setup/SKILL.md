---
name: setup
description: Connect the agent to Pip Dev's local browser MCP and verify it is available.
---

Check that Pip Dev is running before using its browser tools. The Windows development launcher defaults to `http://127.0.0.1:9200/mcp`; Pip Dev may use another port when started with a custom configuration. Use the endpoint reported by Pip Dev if it differs.

For a custom port, change the plugin's `mcp.json` URL before installation, or add the MCP endpoint directly with that port.

In Codex CLI, verify the connection with `codex mcp list`. If you installed this plugin through a local/repo marketplace and `pip_dev` is not listed, add the endpoint with:

```sh
codex mcp add pip_dev --url http://127.0.0.1:9200/mcp
codex mcp list
```

Restart or refresh the agent after changing its MCP configuration. If Pip Dev is stopped, start it first; the plugin does not launch the browser or server by itself.
