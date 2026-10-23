import { describe, expect, it } from 'bun:test'
import {
  NativeInputLockManager,
  nativeInputLockSessionIds,
  releaseAndGetNativeInputLockStatus,
  requireAcknowledgedNativeInputLock,
  type NativeInputDebugger,
} from './native-input-lock'

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  const promise = new Promise<T>((complete) => {
    resolve = complete
  })
  return { promise, resolve }
}

function makeDebugger() {
  const calls: Array<[string, number, string?]> = []
  let onDetach: ((source: { tabId?: number }, reason: string) => void) | undefined
  const api: NativeInputDebugger = {
    async attach({ tabId }) {
      calls.push(['attach', tabId])
    },
    async sendCommand({ tabId }, method) {
      calls.push([method, tabId])
    },
    async detach({ tabId }) {
      calls.push(['detach', tabId])
    },
    onDetach: {
      addListener(listener) {
        onDetach = listener
      },
    },
  }
  return { api, calls, detach: (tabId: number) => onDetach?.({ tabId }, 'target_closed') }
}

describe('NativeInputLockManager', () => {
  it('hands a paused help request to the human and relocks when control resumes', async () => {
    const { api, calls } = makeDebugger()
    const locks = new NativeInputLockManager(api)
    const helpContext = {
      mode: 'agent-control' as const,
      sessionIds: ['session-help'],
      agentLabels: ['Codex'],
      sites: ['example.com'],
      since: 1,
      helpRequest: {
        mode: 'help' as const,
        sessionId: 'session-help',
        agentLabel: 'Codex',
        reason: 'Enter the one-time code',
        requestedAt: 2,
      },
    }

    expect(nativeInputLockSessionIds(helpContext)).toEqual([])
    expect(await locks.sync(7, ['session-help'])).toBe('locked')
    expect(await locks.sync(7, nativeInputLockSessionIds(helpContext))).toBe(
      'unlocked',
    )
    expect(locks.status(7)).toBe('unlocked')

    const resumedContext = { ...helpContext, helpRequest: undefined }
    expect(nativeInputLockSessionIds(resumedContext)).toEqual(['session-help'])
    expect(await locks.sync(7, nativeInputLockSessionIds(resumedContext))).toBe(
      'locked',
    )
    expect(calls).toEqual([
      ['attach', 7],
      ['Input.setAgentInputLock', 7],
      ['Input.setAgentInputLock', 7],
      ['detach', 7],
      ['attach', 7],
      ['Input.setAgentInputLock', 7],
    ])
  })

  it('keeps locking a tab for other live sessions while one session waits for help', async () => {
    const { api, calls } = makeDebugger()
    const locks = new NativeInputLockManager(api)
    const context = {
      mode: 'agent-control' as const,
      sessionIds: ['session-active', 'session-help'],
      agentLabels: ['Codex', 'Claude'],
      sites: ['example.com'],
      since: 1,
      helpRequest: {
        mode: 'help' as const,
        sessionId: 'session-help',
        agentLabel: 'Claude',
        reason: 'Enter the one-time code',
        requestedAt: 2,
      },
    }

    expect(nativeInputLockSessionIds(context)).toEqual(['session-active'])
    expect(await locks.sync(7, nativeInputLockSessionIds(context))).toBe(
      'locked',
    )
    expect(calls).toEqual([
      ['attach', 7],
      ['Input.setAgentInputLock', 7],
    ])
  })

  it('locks only the controlled tab and keeps one debugger lease across polls', async () => {
    const { api, calls } = makeDebugger()
    const locks = new NativeInputLockManager(api)

    expect(await locks.sync(7, ['session-b', 'session-a'])).toBe('locked')
    expect(await locks.sync(7, ['session-a', 'session-b'])).toBe('locked')
    expect(locks.status(7)).toBe('locked')
    expect(locks.status(8)).toBe('unlocked')
    expect(calls).toEqual([
      ['attach', 7],
      ['Input.setAgentInputLock', 7],
    ])
  })

  it('releases the native lease and debugger when the agent no longer controls the tab', async () => {
    const { api, calls } = makeDebugger()
    const locks = new NativeInputLockManager(api)
    await locks.sync(7, ['session-a'])
    await locks.sync(7, [])

    expect(locks.status(7)).toBe('unlocked')
    expect(calls.slice(-2)).toEqual([
      ['Input.setAgentInputLock', 7],
      ['detach', 7],
    ])
  })

  it('keeps an unavailable lease after detach failures and clears it after a successful retry', async () => {
    const { api, calls } = makeDebugger()
    const locks = new NativeInputLockManager(api)
    let detachFailures = 2
    api.detach = async ({ tabId }) => {
      calls.push(['detach', tabId])
      if (detachFailures > 0) {
        detachFailures -= 1
        throw new Error('debugger detach failed')
      }
    }

    await locks.sync(7, ['session-a'])
    await locks.release(7)
    expect(locks.status(7)).toBe('unavailable')

    // The unavailable-lease cleanup path must also preserve the connection
    // when detach still fails so that a later poll can retry it.
    await locks.release(7)
    expect(locks.status(7)).toBe('unavailable')

    await locks.release(7)
    expect(locks.status(7)).toBe('unlocked')
    expect(calls.filter(([method]) => method === 'detach')).toHaveLength(3)
  })

  it('reports the observed lock state when best-effort release throws', async () => {
    const manager = {
      async release() {
        throw new Error('could not restore lock')
      },
      status() {
        return 'unavailable' as const
      },
    }

    await expect(releaseAndGetNativeInputLockStatus(manager, 7)).resolves.toBe(
      'unavailable',
    )
  })

  it('restores a lock when a newer stop starts during the CDP unlock', async () => {
    const { api, calls } = makeDebugger()
    const locks = new NativeInputLockManager(api)
    const unlockStarted = deferred<void>()
    const finishUnlock = deferred<void>()
    let canRelease = true
    let delayUnlock = true
    api.sendCommand = async ({ tabId }, method, params) => {
      calls.push([method, tabId])
      const locked = (params as { locked?: boolean } | undefined)?.locked
      if (method === 'Input.setAgentInputLock' && locked === false && delayUnlock) {
        delayUnlock = false
        unlockStarted.resolve(undefined)
        await finishUnlock.promise
      }
    }

    await locks.sync(7, ['session-a'])
    const releasing = locks.release(7, () => canRelease)
    await unlockStarted.promise
    canRelease = false
    finishUnlock.resolve(undefined)
    await releasing

    expect(calls.slice(-2)).toEqual([
      ['Input.setAgentInputLock', 7],
      ['Input.setAgentInputLock', 7],
    ])
    expect(locks.status(7)).toBe('locked')

    canRelease = true
    await locks.release(7, () => canRelease)
    expect(locks.status(7)).toBe('unlocked')
    expect(calls.slice(-2)).toEqual([
      ['Input.setAgentInputLock', 7],
      ['detach', 7],
    ])
  })

  it('reacquires a native lock when a newer stop starts during CDP detach', async () => {
    const { api, calls } = makeDebugger()
    const locks = new NativeInputLockManager(api)
    const detachStarted = deferred<void>()
    const finishDetach = deferred<void>()
    let canRelease = true
    api.detach = async ({ tabId }) => {
      calls.push(['detach', tabId])
      detachStarted.resolve(undefined)
      await finishDetach.promise
    }

    await locks.sync(7, ['session-a'])
    const releasing = locks.release(7, () => canRelease)
    await detachStarted.promise
    canRelease = false
    finishDetach.resolve(undefined)
    await releasing

    expect(calls.slice(-3)).toEqual([
      ['detach', 7],
      ['attach', 7],
      ['Input.setAgentInputLock', 7],
    ])
    expect(locks.status(7)).toBe('locked')
  })

  it('retains the debugger lease and reports unavailable if stale unlock cannot be reversed', async () => {
    const { api, calls } = makeDebugger()
    const locks = new NativeInputLockManager(api)
    const unlockStarted = deferred<void>()
    const finishUnlock = deferred<void>()
    let canRelease = true
    let delayUnlock = true
    api.sendCommand = async ({ tabId }, method, params) => {
      calls.push([method, tabId])
      const locked = (params as { locked?: boolean } | undefined)?.locked
      if (method !== 'Input.setAgentInputLock') return
      if (locked === false && delayUnlock) {
        delayUnlock = false
        unlockStarted.resolve(undefined)
        await finishUnlock.promise
        return
      }
      if (locked === true && delayUnlock === false && !canRelease) {
        throw new Error('relock failed')
      }
    }

    await locks.sync(7, ['session-a'])
    const releasing = locks.release(7, () => canRelease)
    await unlockStarted.promise
    canRelease = false
    finishUnlock.resolve(undefined)
    await expect(releasing).rejects.toThrow('could not restore the agent input lock')

    expect(locks.status(7)).toBe('unavailable')
    expect(calls).not.toContainEqual(['detach', 7])

    // Even the later cleanup detach must recover if another owner starts while
    // CDP is detaching the retained connection.
    canRelease = true
    const detachStarted = deferred<void>()
    const finishDetach = deferred<void>()
    let canCleanup = true
    let delayDetach = true
    api.detach = async ({ tabId }) => {
      calls.push(['detach', tabId])
      if (!delayDetach) return
      delayDetach = false
      detachStarted.resolve(undefined)
      await finishDetach.promise
    }
    const cleanup = locks.release(7, () => canCleanup)
    await detachStarted.promise
    canCleanup = false
    finishDetach.resolve(undefined)
    await cleanup

    expect(calls.slice(-3)).toEqual([
      ['detach', 7],
      ['attach', 7],
      ['Input.setAgentInputLock', 7],
    ])
    expect(locks.status(7)).toBe('locked')

    // A final reconciliation can then detach and clear the retained connection.
    await locks.release(7, () => canRelease)
    expect(locks.status(7)).toBe('unlocked')
    expect(calls.at(-1)).toEqual(['detach', 7])
  })

  it('never reports a lock when Chromium lacks the native command', async () => {
    const { api, calls } = makeDebugger()
    api.sendCommand = async ({ tabId }, method) => {
      calls.push([method, tabId])
      if (method === 'Input.setAgentInputLock') throw new Error('method not found')
    }
    const locks = new NativeInputLockManager(api)

    expect(await locks.sync(7, ['session-a'])).toBe('unavailable')
    expect(locks.status(7)).toBe('unavailable')
    expect(calls).toEqual([
      ['attach', 7],
      ['Input.setAgentInputLock', 7],
      ['Input.setAgentInputLock', 7],
      ['detach', 7],
    ])
  })

  it('reports browser debugger detachment instead of leaving a stale locked state', async () => {
    const { api, detach } = makeDebugger()
    const locks = new NativeInputLockManager(api)
    await locks.sync(7, ['session-a'])
    detach(7)

    expect(locks.status(7)).toBe('unavailable')
  })
})
