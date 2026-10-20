import { describe, expect, it } from 'bun:test'
import {
  NativeInputLockManager,
  type NativeInputDebugger,
} from './native-input-lock'

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
