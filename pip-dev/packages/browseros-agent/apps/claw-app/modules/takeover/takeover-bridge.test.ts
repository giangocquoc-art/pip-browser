import { beforeEach, describe, expect, it, mock } from 'bun:test'
import {
  type HelpRequest,
  SessionStatus,
  type SessionSummary,
} from '@browseros/claw-api'

let sessions: SessionSummary[] = []
let sessionsUnavailable = false
let listCalls = 0
const resolveCalls: Array<{ sessionId: string; note?: string }> = []
const cancelCalls: Array<{ sessionId: string }> = []

mock.module('@browseros/claw-api-client', () => ({
  ClawApiClient: class {
    async listSessions() {
      listCalls += 1
      if (sessionsUnavailable) throw new Error('session service unavailable')
      return { items: sessions }
    }
    async resolveHelp(request: { sessionId: string; note?: string }) {
      resolveCalls.push(request)
      return { resolved: true }
    }
    async cancelSession(request: { sessionId: string }) {
      cancelCalls.push(request)
      return { cancelled: true }
    }
  },
}))

const { createTakeoverBridge } = await import('./takeover-bridge')

function session(
  tabId: number,
  over: Partial<HelpRequest> = {},
): SessionSummary {
  const help: HelpRequest = {
    requestId: 'help-1',
    reason: 'Enter the login',
    browserTabId: tabId,
    requestedAt: 5_000,
    ...over,
  }
  return {
    sessionId: 'session-1',
    slug: 'claude-code',
    label: 'Claude-code',
    name: 'Outreach',
    durationMs: 0,
    dispatchCount: 3,
    errorCount: 0,
    toolSequence: [],
    status: SessionStatus.Live,
    startedAt: 1_000,
    live: { state: 'idle', browserTabs: [], helpRequest: help },
  }
}

beforeEach(() => {
  sessions = []
  sessionsUnavailable = false
  listCalls = 0
  resolveCalls.length = 0
  cancelCalls.length = 0
})

describe('takeover bridge', () => {
  it('reports the blocked tab as active and others as inactive', async () => {
    sessions = [session(77)]
    const bridge = createTakeoverBridge({
      resolveServerBaseUrl: async () => 'http://127.0.0.1:9000',
    })

    expect(await bridge.pollForTab(77)).toEqual({
      active: true,
      context: {
        mode: 'help',
        sessionId: 'session-1',
        agentLabel: 'Claude-code',
        reason: 'Enter the login',
        resumeHint: undefined,
        site: undefined,
        requestedAt: 5_000,
      },
    })
    expect(await bridge.pollForTab(42)).toEqual({ active: false })
  })

  it('combines an accessible agent tab with a help request on the same tab', async () => {
    const activeTab = {
      browserTabId: 77,
      url: 'https://example.com/page',
      title: 'Example',
      toolCount: 1,
      recentTools: [],
    }
    sessions = [
      {
        ...session(99),
        sessionId: 'session-active',
        live: { state: 'active', browserTabs: [activeTab] },
      },
      {
        ...session(77, { requestId: 'help-2' }),
        sessionId: 'session-help',
      },
    ]
    const bridge = createTakeoverBridge({
      resolveServerBaseUrl: async () => 'http://127.0.0.1:9000',
    })

    expect(await bridge.pollForTab(77)).toEqual({
      active: true,
      context: {
        mode: 'agent-control',
        sessionIds: ['session-active'],
        agentLabels: ['Claude-code'],
        sites: ['example.com'],
        since: 1_000,
        helpRequest: {
          mode: 'help',
          sessionId: 'session-help',
          agentLabel: 'Claude-code',
          reason: 'Enter the login',
          resumeHint: undefined,
          site: undefined,
          requestedAt: 5_000,
        },
      },
    })
  })

  it('coalesces concurrent polls within the cache window into one read', async () => {
    sessions = [session(77)]
    let clock = 10_000
    const bridge = createTakeoverBridge({
      resolveServerBaseUrl: async () => 'http://127.0.0.1:9000',
      now: () => clock,
    })

    await Promise.all([bridge.pollForTab(77), bridge.pollForTab(42)])
    clock += 500 // still inside the TTL
    await bridge.pollForTab(77)
    expect(listCalls).toBe(1)

    clock += 2_000 // past the TTL
    await bridge.pollForTab(77)
    expect(listCalls).toBe(2)
  })

  it('reports a disconnected session service separately from no active agent', async () => {
    const tab = {
      browserTabId: 77,
      url: 'https://example.com',
      title: 'Example',
      toolCount: 1,
      recentTools: [],
    }
    sessions = [{
      ...session(99),
      live: { state: 'active', browserTabs: [tab] },
    }]
    let clock = 10_000
    const bridge = createTakeoverBridge({
      resolveServerBaseUrl: async () => 'http://127.0.0.1:9000',
      now: () => clock,
    })

    expect((await bridge.pollForTab(77)).active).toBe(true)
    sessionsUnavailable = true
    clock += 1_300
    await expect(bridge.pollForTab(77)).resolves.toEqual({
      active: false,
      unavailable: true,
    })
  })

  it('resolves through the client and clears the cache', async () => {
    sessions = [session(77)]
    const bridge = createTakeoverBridge({
      resolveServerBaseUrl: async () => 'http://127.0.0.1:9000',
    })

    await bridge.pollForTab(77)
    await expect(bridge.resolve(77, 'session-1', 'signed in')).resolves.toBe(true)
    expect(resolveCalls).toEqual([
      { sessionId: 'session-1', note: 'signed in' },
    ])

    await bridge.pollForTab(77)
    // Poll, fresh tab validation for resolve, then another read after the cache drops.
    expect(listCalls).toBe(3)
  })

  it('does not resolve a help request from a different tab', async () => {
    sessions = [session(77)]
    const bridge = createTakeoverBridge({
      resolveServerBaseUrl: async () => 'http://127.0.0.1:9000',
    })

    await expect(bridge.resolve(42, 'session-1')).resolves.toBe(false)
    expect(resolveCalls).toEqual([])
  })

  it('stops all live sessions that currently include the sender tab', async () => {
    const tab = {
      browserTabId: 77,
      url: 'https://example.com',
      title: 'Example',
      toolCount: 1,
      recentTools: [],
    }
    sessions = [
      session(10, { requestId: 'other-tab' }),
      {
        ...session(77),
        live: { state: 'active', browserTabs: [tab] },
      },
    ]
    const bridge = createTakeoverBridge({
      resolveServerBaseUrl: async () => 'http://127.0.0.1:9000',
    })

    await expect(bridge.stopForTab(77)).resolves.toEqual({
      stopped: 1,
      total: 1,
    })
    expect(cancelCalls).toEqual([{ sessionId: 'session-1' }])
  })
})
