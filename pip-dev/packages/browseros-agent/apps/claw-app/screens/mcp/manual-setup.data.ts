/** Pip Dev MCP setup guidance shared by the connection card and dialog. */

export const MCP_SERVER_NAME = 'pip_dev'

export const MANUAL_SETUP_DOCS_URL =
  'https://github.com/giangocquoc-art/pip-browser/blob/main/pip-dev/docs/PIP_DEV_AGENT_SETUP.md'

export const MANUAL_SETUP_CARD_LINE =
  "Use Pip Dev's local MCP endpoint with another compatible agent."

export interface ManualSetupStep {
  id: string
  title: string
  body: string
  note?: string
}

export const MANUAL_SETUP_STEPS: readonly ManualSetupStep[] = [
  {
    id: 'add-mcp',
    title: 'Add the Pip Dev MCP server',
    body: `Configure a Streamable HTTP MCP server named ${MCP_SERVER_NAME} and paste the Pip Dev endpoint shown above. The default development URL is http://127.0.0.1:9200/mcp.`,
    note: 'Pip Dev must be running on this PC. Keep the endpoint on loopback; it controls pages in your local browser.',
  },
]
