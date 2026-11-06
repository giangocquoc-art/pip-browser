import { describe, expect, it } from 'bun:test'
import {
  handleTakeoverAction,
  type TakeoverActionDependencies,
} from './takeover-action'

function fixture(options?: {
  activeTabId?: number
  stopped?: number
  total?: number
}) {
  const calls: string[] = []
  const dependencies: TakeoverActionDependencies = {
    extensionId: 'pip-extension-id',
    popupUrl: 'chrome-extension://pip-extension-id/popup.html',
    getActiveTab: async (windowId) => {
      calls.push(`active:${windowId}`)
      return { id: options?.activeTabId ?? 17, windowId }
    },
    pollForTab: async (tabId) => {
      calls.push(`poll:${tabId}`)
      return { active: false }
    },
    stopForTab: async (tabId) => {
      calls.push(`stop:${tabId}`)
      return { stopped: options?.stopped ?? 1, total: options?.total ?? 1 }
    },
    beginStop: (tabId) => {
      calls.push(`begin:${tabId}`)
    },
    finishStop: (tabId) => {
      calls.push(`finish:${tabId}`)
    },
    reconcileLock: async (tabId) => {
      calls.push(`reconcile:${tabId}`)
    },
  }
  return { calls, dependencies }
}

const trustedPopup = {
  id: 'pip-extension-id',
  url: 'chrome-extension://pip-extension-id/popup.html',
}
const stopRequest = {
  type: 'takeover-action-stop',
  tabId: 17,
  windowId: 4,
}

describe('takeover toolbar action', () => {
  it('ignores unrelated extension messages', async () => {
    const { calls, dependencies } = fixture()
    const result = await handleTakeoverAction(
      { type: 'other-message' },
      trustedPopup,
      dependencies,
    )
    expect(result).toBeNull()
    expect(calls).toEqual([])
  })

  it('rejects page-origin messages before reading or stopping tabs', async () => {
    const { calls, dependencies } = fixture()
    await expect(
      handleTakeoverAction(stopRequest, { id: 'pip-extension-id', url: 'https://evil.test/' }, dependencies),
    ).resolves.toEqual({ ok: false, reason: 'untrusted-sender' })
    expect(calls).toEqual([])
  })

  it('will not stop a tab that is no longer active in the popup window', async () => {
    const { calls, dependencies } = fixture({ activeTabId: 18 })
    await expect(
      handleTakeoverAction(stopRequest, trustedPopup, dependencies),
    ).resolves.toMatchObject({ ok: false, action: 'stop', reason: 'inactive-tab' })
    expect(calls).toEqual(['active:4'])
  })

  it('revalidates tab focus after entering the stop gate', async () => {
    const { calls, dependencies } = fixture()
    let lookups = 0
    dependencies.getActiveTab = async (windowId) => {
      calls.push(`active:${windowId}`)
      lookups += 1
      return { id: lookups === 1 ? 17 : 18, windowId }
    }

    await expect(
      handleTakeoverAction(stopRequest, trustedPopup, dependencies),
    ).resolves.toMatchObject({ ok: false, action: 'stop', reason: 'inactive-tab' })
    expect(calls).toEqual([
      'active:4',
      'begin:17',
      'active:4',
      'reconcile:17',
      'finish:17',
    ])
  })

  it('cancels only the validated tab sessions and reconciles the lock on success', async () => {
    const { calls, dependencies } = fixture()
    await expect(
      handleTakeoverAction(stopRequest, trustedPopup, dependencies),
    ).resolves.toEqual({ ok: true, action: 'stop', stopped: 1, total: 1 })
    expect(calls).toEqual([
      'active:4',
      'begin:17',
      'active:4',
      'stop:17',
      'reconcile:17',
      'finish:17',
    ])
  })

  it('reconciles current ownership after partial cancellation', async () => {
    const { calls, dependencies } = fixture({ stopped: 1, total: 2 })
    await expect(
      handleTakeoverAction(stopRequest, trustedPopup, dependencies),
    ).resolves.toMatchObject({
      ok: false,
      action: 'stop',
      reason: 'partial-stop',
      stopped: 1,
      total: 2,
    })
    expect(calls).toContain('reconcile:17')
    expect(calls.at(-1)).toBe('finish:17')
  })

  it('reconciles stale locks when no live session still owns the tab', async () => {
    const { calls, dependencies } = fixture({ stopped: 0, total: 0 })
    await expect(
      handleTakeoverAction(stopRequest, trustedPopup, dependencies),
    ).resolves.toEqual({ ok: true, action: 'stop', stopped: 0, total: 0 })
    expect(calls).toContain('reconcile:17')
  })

  it('reconciles ownership after a cancellation error before reopening the gate', async () => {
    const { calls, dependencies } = fixture()
    dependencies.stopForTab = async (tabId) => {
      calls.push(`stop-error:${tabId}`)
      throw new Error('cancel failed')
    }

    await expect(
      handleTakeoverAction(stopRequest, trustedPopup, dependencies),
    ).resolves.toMatchObject({ ok: false, reason: 'unavailable' })
    expect(calls.slice(-2)).toEqual(['reconcile:17', 'finish:17'])
  })
})
