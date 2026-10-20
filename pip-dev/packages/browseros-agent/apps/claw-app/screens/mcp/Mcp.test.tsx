/**
 * Pins the editorial MCP page shape: compressed hero + single
 * endpoint URL strip, inline Connected-agents header with an
 * `N of M connected` mono chip, hairline row list of the 7 supported
 * harnesses.
 */

import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderToStaticMarkup } from 'react-dom/server'
import { MemoryRouter } from 'react-router'
import * as _connectionsHooks from '@/modules/api/connections.hooks'
import { CommandBlock } from './CommandBlock'
import { MANUAL_SETUP_DOCS_URL, MCP_SERVER_NAME } from './manual-setup.data'

const mcpBrowserosConnections = [
  {
    harness: 'Claude Code',
    installed: false,
    message: '',
  },
  {
    harness: 'Cursor',
    installed: true,
    configPath: '/tmp/cursor.json',
    message: 'Configured in Cursor.',
  },
  {
    harness: 'Codex',
    installed: false,
    message: '',
  },
  {
    harness: 'OpenCode',
    installed: false,
    message: '',
  },
  {
    harness: 'Antigravity',
    installed: false,
    message: '',
  },
  {
    harness: 'VS Code',
    installed: false,
    message: '',
  },
  {
    harness: 'Zed',
    installed: false,
    message: '',
  },
]

const connectionsHookResultKey = '__browserclawConnectionsHookResult'

function connectionsHookState() {
  return globalThis as Record<string, unknown>
}

function setConnectionsHookResult(result: unknown) {
  connectionsHookState()[connectionsHookResultKey] = result
}

function getConnectionsHookResult() {
  return (
    connectionsHookState()[connectionsHookResultKey] ?? {
      data: {
        items: mcpBrowserosConnections,
      },
      isPending: false,
      isError: false,
    }
  )
}

// Spread the real module so unrelated tests that import a different
// hook from connections.hooks still work: partial mock.module()
// replacements corrupt Bun's process-scoped module registry (see the
// 2026-07-17 test reliability audit).
mock.module('@/modules/api/connections.hooks', () => ({
  ..._connectionsHooks,
  useConnections: Object.assign(() => getConnectionsHookResult(), {
    getKey: () => ['cockpit', 'connections'],
  }),
  useConnectHarness: () => ({
    isPending: false,
    variables: undefined,
    mutateAsync: async () => ({ installed: true }),
  }),
  useDisconnectHarness: () => ({
    isPending: false,
    variables: undefined,
    mutateAsync: async () => ({ installed: false }),
  }),
}))

beforeEach(() => {
  setConnectionsHookResult({
    data: {
      items: mcpBrowserosConnections,
    },
    isPending: false,
    isError: false,
  })
})

afterEach(() => {
  setConnectionsHookResult({
    data: undefined,
    isPending: true,
    isError: false,
  })
})

const { Mcp } = await import('./Mcp')
const { HeroCard } = await import('./HeroCard')

function renderApp(): string {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  return renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <Mcp />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

describe('Mcp (editorial)', () => {
  it('renders the editorial hero without exposing the fallback endpoint before pref resolution', () => {
    const html = renderApp()
    expect(html).toContain('Pip Dev MCP')
    expect(html).toContain('pip_dev')
    expect(html).not.toContain('font-serif')
    expect(html).not.toContain('http://127.0.0.1:9200/mcp')
    expect(html).not.toContain('Copy')
  })

  it('renders the endpoint copy strip once the resolved URL is available', () => {
    const html = renderToStaticMarkup(
      <HeroCard url="http://127.0.0.1:9512/mcp" />,
    )

    expect(html).toContain('http://127.0.0.1:9512/mcp')
    expect(html).not.toContain('/mcp/claude-code')
    expect(html).not.toContain('/cockpit')
    expect(html).toContain('Copy')
  })

  it('does NOT render the removed CLI snippet block', () => {
    const html = renderApp()
    expect(html).not.toContain('CLI SNIPPET')
    // Guard against any CLI snippet resurfacing regardless of the
    // registered server name (`browseros` legacy or `BrowserClaw`
    // post-rename).
    expect(html).not.toContain('claude mcp add')
    expect(html).not.toContain('--transport http')
  })

  it('renders the Connected-agents header with the count chip', () => {
    const html = renderApp()
    expect(html).toContain('Connected agents')
    expect(html).toContain('1 of 7 connected')
  })

  it('renders one row per supported harness', () => {
    const html = renderApp()
    expect(html).toContain('Claude Code')
    expect(html).toContain('Cursor')
    expect(html).toContain('Codex')
    expect(html).toContain('OpenCode')
    expect(html).toContain('Antigravity')
    expect(html).toContain('VS Code')
    expect(html).toContain('Zed')
    expect(html).not.toContain('Hermes')
    expect(html).not.toContain('Gemini CLI')
    expect(html).not.toContain('OpenClaw')
  })

  it('does not advertise an upstream Claude Desktop artifact', () => {
    const html = renderApp()
    expect(html).not.toContain('Claude Desktop')
    expect(html).not.toContain('browseros-neo')
    expect(html).not.toContain('browseros-ai/browserclaw-claude-desktop')
    expect(html).not.toContain('.mcpb')
  })

  it('renders the row action voices in title case with no status dot', () => {
    const html = renderApp()
    expect(html).toMatch(/>Connect</)
    expect(html).toMatch(/>Connected</)
    expect(html).toMatch(/>Disconnect</)
    // The green word carries the state; the mock has no dot.
    expect(html).not.toContain('rounded-full bg-green')
  })

  it('does NOT render the removed floating footer paragraph', () => {
    const html = renderApp()
    expect(html).not.toContain('Hermes and OpenClaw run inside BrowserOS')
  })

  it('does NOT render the removed Built-in variant', () => {
    const html = renderApp()
    expect(html).not.toContain('Built-in')
    expect(html).not.toContain('built-in')
  })

  it('does NOT render the removed marketing subtitle from the old HeroCard', () => {
    const html = renderApp()
    expect(html).not.toContain(
      'Add BrowserOS as an MCP server in your AI agent',
    )
    expect(html).not.toContain('One endpoint, every harness.')
  })

  it('offers a manual route for agents that are not in the list', () => {
    const html = renderApp()
    expect(html).toContain('Any other agent')
    expect(html).toContain('Connect another agent to Pip Dev')
  })

  it('keeps the manual setup dialog closed until the card is activated', () => {
    const html = renderApp()
    expect(html).not.toContain('Add Pip Dev MCP server')
    expect(html).not.toContain(MANUAL_SETUP_DOCS_URL)
  })

  it('uses the Pip Dev server id in its setup guidance', () => {
    expect(MCP_SERVER_NAME).toBe('pip_dev')
    expect(MANUAL_SETUP_DOCS_URL).toContain('PIP_DEV_AGENT_SETUP.md')
  })

  it('holds the endpoint block open while the URL is still resolving', () => {
    // resolveCanonicalMcpEndpointUrl is async, so the dialog can open before
    // the endpoint exists. Removing the block would tell someone to add an
    // endpoint with nothing on screen to add, and would shift the layout
    // when it arrived.
    const html = renderToStaticMarkup(<CommandBlock command={null} />)
    expect(html).toContain('Resolving the endpoint')
    expect(html).toContain('animate-pulse')
    expect(html).toContain('disabled')
  })
})
