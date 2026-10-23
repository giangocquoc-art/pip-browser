import { beforeEach, describe, expect, it, mock } from 'bun:test'
import {
  type HelpRequest,
  SessionStatus,
  type SessionSummary,
} from '@browseros/claw-api'
import { nativeInputLockSessionIds } from './native-input-lock'

let sessions: SessionSummary[] = []
let sessionsUnavailable = false
let listCalls = 0
const resolveCalls: Array<{
  sessionId: string
  requestId: string
  browserTabId: number
  note?: string
}> = []
const resolveOrder: string[] = []
let resolveResult = true
let resolveDeferred: Promise<{ resolved: boolean }> | null = null
let onResolveStarted: (() => void) | null = null
const cancelCalls: Array<{ sessionId: string }> = []

mock.module('@browseros/claw-api-client', () => ({
  ClawApiClient: class {
    async listSessions() {
      listCalls += 1
      if (sessionsUnavailable) throw new Error('session service unavailable')
      return { items: sessions }
    }
    async resolveHelp(request: {
      sessionId: string
      requestId: string
      browserTabId: number
      note?: string
    }) {
      resolveCalls.push(request)
      resolveOrder.push('server-resolve')
      onResolveStarted?.()
      if (resolveDeferred) return resolveDeferred
      return { resolved: resolveResult }
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

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: Error) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

async function currentLockOwners(
  bridge: ReturnType<typeof createTakeoverBridge>,
  tabId: number,
): Promise<string[]> {
  const response = await bridge.refreshForTab(tabId)
  if (!response.active || response.context.mode !== 'agent-control') return []
  return nativeInputLockSessionIds(response.context)
}

beforeEach(() => {
  sessions = []
  sessionsUnavailable = false
  listCalls = 0
  resolveCalls.length = 0
  resolveOrder.length = 0
  resolveResult = true
  resolveDeferred = null
  onResolveStarted = null
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
        requestId: 'help-1',
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
          requestId: 'help-2',
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
    await expect(bridge.resolve(77, 'session-1', 'help-1', 'signed in')).resolves.toBe(true)
    expect(resolveCalls).toEqual([
      {
        sessionId: 'session-1',
        requestId: 'help-1',
        browserTabId: 77,
        note: 'signed in',
      },
    ])

    await bridge.pollForTab(77)
    // Poll, fresh tab validation for resolve, then another read after the cache drops.
    expect(listCalls).toBe(3)
  })

  it('locks the controlled tab before resuming a human-help session', async () => {
    const live = session(77).live!
    sessions = [
      {
        ...session(77),
        live: {
          ...live,
          browserTabs: [
            {
              browserTabId: 77,
              url: 'https://example.com/login',
              title: 'Example login',
              toolCount: 1,
              recentTools: [],
            },
          ],
        },
      },
    ]
    const bridge = createTakeoverBridge({
      resolveServerBaseUrl: async () => 'http://127.0.0.1:9000',
    })

    await expect(
      bridge.resolve(77, 'session-1', 'help-1', 'signed in', async (control) => {
        expect(control?.sessionIds).toEqual(['session-1'])
        resolveOrder.push('native-lock')
      }),
    ).resolves.toBe(true)

    expect(resolveOrder).toEqual(['native-lock', 'server-resolve'])
  })

  it('unlocks a paused-only tab when the server declines the hand-back', async () => {
    sessions = [session(77)]
    resolveResult = false
    const bridge = createTakeoverBridge({
      resolveServerBaseUrl: async () => 'http://127.0.0.1:9000',
    })
    let lockOwners = ['session-1']
    let reconciliations = 0

    await expect(
      bridge.resolve(
        77,
        'session-1',
        'help-1',
        undefined,
        async () => {},
        async () => {
          reconciliations += 1
          lockOwners = await currentLockOwners(bridge, 77)
        },
      ),
    ).resolves.toBe(false)

    expect(reconciliations).toBe(1)
    expect(lockOwners).toEqual([])
  })

  it('does not resume when the native lock is unavailable or throws', async () => {
    sessions = [session(77)]
    const bridge = createTakeoverBridge({
      resolveServerBaseUrl: async () => 'http://127.0.0.1:9000',
    })
    let lockOwners = ['session-1']
    let reconciliations = 0

    for (const failure of ['unavailable', 'debugger threw']) {
      await expect(
        bridge.resolve(
          77,
          'session-1',
          'help-1',
          undefined,
          async () => {
            throw new Error(failure)
          },
          async () => {
            reconciliations += 1
            lockOwners = await currentLockOwners(bridge, 77)
          },
        ),
      ).rejects.toThrow(failure)
    }

    expect(resolveCalls).toEqual([])
    expect(reconciliations).toBe(2)
    expect(lockOwners).toEqual([])
  })

  it('reconciles current owners after a deferred resolve request rejects', async () => {
    const controlledTab = {
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
        live: { state: 'active' as const, browserTabs: [controlledTab] },
      },
      { ...session(77), sessionId: 'session-help' },
    ]
    const bridge = createTakeoverBridge({
      resolveServerBaseUrl: async () => 'http://127.0.0.1:9000',
    })
    const response = deferred<{ resolved: boolean }>()
    const started = deferred<void>()
    resolveDeferred = response.promise
    onResolveStarted = () => started.resolve()
    let lockOwners: string[] = []
    let reconciliations = 0
    const resolving = bridge.resolve(
      77,
      'session-help',
      'help-1',
      undefined,
      async (control) => {
        expect(control?.sessionIds).toEqual(['session-active'])
        lockOwners = control ? nativeInputLockSessionIds(control) : []
      },
      async () => {
        reconciliations += 1
        lockOwners = await currentLockOwners(bridge, 77)
      },
    )

    await started.promise
    expect(lockOwners).toEqual(['session-active'])
    expect(reconciliations).toBe(0)
    response.reject(new Error('resolve response lost'))

    await expect(resolving).rejects.toThrow('resolve response lost')
    expect(reconciliations).toBe(1)
    expect(lockOwners).toEqual(['session-active'])
  })

  it('does not resolve a newer help request using a stale request ID', async () => {
    sessions = [session(77, { requestId: 'help-2' })]
    const bridge = createTakeoverBridge({
      resolveServerBaseUrl: async () => 'http://127.0.0.1:9000',
    })

    await expect(bridge.resolve(77, 'session-1', 'help-1')).resolves.toBe(false)
    expect(resolveCalls).toEqual([])
  })

  it('does not resolve a help request from a different tab', async () => {
    sessions = [session(77)]
    const bridge = createTakeoverBridge({
      resolveServerBaseUrl: async () => 'http://127.0.0.1:9000',
    })

    await expect(bridge.resolve(42, 'session-1', 'help-1')).resolves.toBe(false)
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
