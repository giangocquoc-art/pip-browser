import { describe, expect, it } from 'bun:test'
import {
  reconcilePendingAgentControlLeases,
  refreshStopLease,
  type AgentControlLeaseView,
} from './agent-control-bridge'
import {
  NativeInputLockManager,
  type AgentLeaseLockStatus,
  type NativeInputDebugger,
} from './native-input-lock'

const noTakeoverOwners = async (): Promise<string[]> => []

const lease = (
  status: AgentControlLeaseView['status'],
  operation: AgentControlLeaseView['operation'],
): AgentControlLeaseView => ({
  leaseId: 'lease-exact',
  generation: 1,
  snapshotRevision: 1,
  pageId: 3,
  tabId: 17,
  targetId: 'target-incarnation-a',
  operation,
  status,
  expiresAtMs: Date.now() + 10_000,
})

function makeLockDebugger(targetId: string) {
  const calls: Array<{ method: string; params?: object }> = []
  const attached = new Set<number>()
  let detachListener:
    | ((source: { tabId?: number }, reason: string) => void)
    | undefined
  const api: NativeInputDebugger = {
    async attach({ tabId }) {
      attached.add(tabId)
      calls.push({ method: `attach:${tabId}` })
    },
    async sendCommand({ tabId }, method, params) {
      calls.push({ method, params })
      if (!attached.has(tabId)) throw new Error('debugger is not attached')
      if (method === 'Target.getTargetInfo') {
        return { targetInfo: { targetId } }
      }
    },
    async detach({ tabId }) {
      attached.delete(tabId)
      calls.push({ method: `detach:${tabId}` })
    },
    onDetach: {
      addListener(listener) {
        detachListener = listener
      },
    },
  }
  return { api, calls, detach: (tabId: number) => detachListener?.({ tabId }, 'target_closed') }
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  const promise = new Promise<T>((complete) => {
    resolve = complete
  })
  return { promise, resolve }
}

describe('agent-control extension bridge', () => {
  it('acknowledges the exact tuple only after native lock confirmation', async () => {
    const order: string[] = []
    let finishLock!: (status: AgentLeaseLockStatus) => void
    const lockFinished = new Promise<AgentLeaseLockStatus>((resolve) => {
      finishLock = resolve
    })
    const locks = {
      syncAgentControlLease: async (
        tabId: number,
        leaseId: string,
        targetId: string,
        operation: 'lock' | 'unlock',
      ) => {
        order.push(`lock:${tabId}:${leaseId}:${targetId}:${operation}`)
        return lockFinished
      },
    }
    const acknowledgements: string[] = []
    const running = reconcilePendingAgentControlLeases(
      [lease('locking', 'lock')],
      locks,
      async (_lease, status) => {
        order.push(`ack:${status}`)
        acknowledgements.push(status)
      },
      noTakeoverOwners,
    )

    await Promise.resolve()
    expect(acknowledgements).toEqual([])
    finishLock('locked')
    await running

    expect(order).toEqual([
      'lock:17:lease-exact:target-incarnation-a:lock',
      'ack:locked',
    ])
  })

  it('acks stale target as stale rather than adopting the shared native lock state', async () => {
    const acknowledged: string[] = []
    const result = await reconcilePendingAgentControlLeases(
      [lease('locking', 'lock')],
      {
        syncAgentControlLease: async () => 'stale',
      },
      async (_lease, status) => {
        acknowledged.push(status)
      },
      noTakeoverOwners,
    )

    expect(acknowledged).toEqual(['stale'])
    expect(result).toEqual({ processed: 1, unavailable: 0 })
  })

  it('does not poll or acknowledge locked and failed leases', async () => {
    let calls = 0
    const result = await reconcilePendingAgentControlLeases(
      [lease('locked', 'lock'), lease('failed', 'unlock')],
      {
        syncAgentControlLease: async () => {
          calls += 1
          return 'locked'
        },
      },
      async () => {
        calls += 1
      },
      noTakeoverOwners,
    )

    expect(calls).toBe(0)
    expect(result).toEqual({ processed: 0, unavailable: 0 })
  })

  it('ignores a delayed old lease snapshot after a newer lease locks the same tab', async () => {
    const { api, calls } = makeLockDebugger('target-incarnation-a')
    const locks = new NativeInputLockManager(api)
    const delayedOwners = deferred<string[] | null>()
    const acknowledgements: string[] = []
    const oldLease = {
      ...lease('unlocking', 'unlock'),
      leaseId: 'lease-a',
      snapshotRevision: 1,
    }
    const oldReconcile = reconcilePendingAgentControlLeases(
      [oldLease],
      locks,
      async (_lease, status) => acknowledgements.push(`old:${status}`),
      () => delayedOwners.promise,
    )

    const newLease = {
      ...lease('locking', 'lock'),
      leaseId: 'lease-b',
      snapshotRevision: 2,
    }
    await reconcilePendingAgentControlLeases(
      [newLease],
      locks,
      async (_lease, status) => acknowledgements.push(`new:${status}`),
      noTakeoverOwners,
    )
    expect(acknowledgements).toEqual(['new:locked'])
    const callsAfterNewLock = [...calls]

    delayedOwners.resolve([])
    await oldReconcile

    expect(acknowledgements).toEqual(['new:locked', 'old:stale'])
    expect(calls).toEqual(callsAfterNewLock)
    expect(calls.some((call) => call.method === 'Input.setAgentInputLock' &&
      (call.params as { locked?: boolean } | undefined)?.locked === false)).toBe(false)
  })

  it('refreshes a stale Stop generation only for the same lease incarnation', () => {
    const initial = lease('unlocking', 'unlock')
    const refreshed = {
      ...initial,
      generation: 2,
      snapshotRevision: 9,
    }
    expect(refreshStopLease(initial, [refreshed])).toEqual(refreshed)
    expect(
      refreshStopLease(initial, [
        { ...refreshed, leaseId: 'different-owner-lease' },
      ]),
    ).toBeUndefined()
    expect(
      refreshStopLease(initial, [
        { ...refreshed, targetId: 'replacement-target' },
      ]),
    ).toBeUndefined()
  })

  it('reconstructs the exact shared owner snapshot for a hidden target lease', async () => {
    const sameTargetPeer = { ...lease('locked', 'lock'), leaseId: 'lease-peer' }
    const otherTarget = {
      ...lease('locked', 'lock'),
      leaseId: 'lease-other-target',
      targetId: 'replacement-target',
    }
    let observed:
      | { agentLeaseIds: string[]; takeoverSessionIds: string[] }
      | undefined
    await reconcilePendingAgentControlLeases(
      [lease('unlocking', 'unlock'), sameTargetPeer, otherTarget],
      {
        syncAgentControlLease: async (_tabId, _id, _target, _operation, _generation, _revision, owners) => {
          observed = owners
          return 'unlocked'
        },
      },
      async () => {},
      async (tabId) => {
        expect(tabId).toBe(17)
        return ['takeover-session']
      },
    )

    expect(observed).toEqual({
      agentLeaseIds: ['lease-exact', 'lease-peer'],
      takeoverSessionIds: ['takeover-session'],
    })
  })

  it('fails closed when takeover ownership cannot be refreshed', async () => {
    let nativeCalls = 0
    const acknowledged: string[] = []
    await reconcilePendingAgentControlLeases(
      [lease('unlocking', 'unlock')],
      {
        syncAgentControlLease: async () => {
          nativeCalls += 1
          return 'unlocked'
        },
      },
      async (_lease, status) => {
        acknowledged.push(status)
      },
      async () => null,
    )

    expect(nativeCalls).toBe(0)
    expect(acknowledged).toEqual(['unavailable'])
  })

  it('reports unlock unavailable until CDP cleanup is confirmed', async () => {
    const acknowledged: string[] = []
    await reconcilePendingAgentControlLeases(
      [lease('unlocking', 'unlock')],
      {
        syncAgentControlLease: async () => 'unavailable',
      },
      async (_lease, status) => {
        acknowledged.push(status)
      },
      noTakeoverOwners,
    )

    expect(acknowledged).toEqual(['unavailable'])
  })
})
