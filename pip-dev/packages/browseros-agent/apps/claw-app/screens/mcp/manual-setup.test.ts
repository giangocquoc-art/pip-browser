import { describe, expect, it } from 'bun:test'
import {
  MANUAL_SETUP_DOCS_URL,
  MANUAL_SETUP_STEPS,
  MCP_SERVER_NAME,
} from './manual-setup.data'

describe('Pip Dev MCP setup guidance', () => {
  it('uses the same MCP server name as the Pip Dev plugin', () => {
    expect(MCP_SERVER_NAME).toBe('pip_dev')
  })

  it('shows a single local Streamable HTTP setup step without upstream artifacts', () => {
    expect(MANUAL_SETUP_STEPS).toHaveLength(1)
    expect(MANUAL_SETUP_STEPS[0]?.id).toBe('add-mcp')
    expect(MANUAL_SETUP_STEPS[0]?.body).toContain('http://127.0.0.1:9200/mcp')
    expect(MANUAL_SETUP_STEPS[0]?.body).toContain('pip_dev')
    expect(MANUAL_SETUP_STEPS.join(' ')).not.toContain('browseros-neo')
    expect(MANUAL_SETUP_STEPS.join(' ')).not.toContain('npx skills add')
    expect(MANUAL_SETUP_DOCS_URL).toContain('giangocquoc-art/pip-browser')
    expect(MANUAL_SETUP_DOCS_URL).toContain('PIP_DEV_AGENT_SETUP.md')
  })
})
