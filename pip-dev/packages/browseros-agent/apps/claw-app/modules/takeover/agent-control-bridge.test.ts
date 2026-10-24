import { describe, expect, it } from 'bun:test'
import {
  reconcilePendingAgentControlLeases,
  type AgentControlLeaseView,
} from './agent-control-bridge'
import type { AgentLeaseLockStatus } from './native-input-lock'

const lease = (
  status: AgentControlLeaseView['status'],
  operation: AgentControlLeaseView['operation'],
): AgentControlLeaseView => ({
  leaseId: 'lease-exact',
  pageId: 3,
  tabId: 17,
  targetId: 'target-incarnation-a',
  operation,
  status,
  expiresAtMs: Date.now() + 10_000,
})

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
    )

    expect(calls).toBe(0)
    expect(result).toEqual({ processed: 0, unavailable: 0 })
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
    )

    expect(acknowledged).toEqual(['unavailable'])
  })
})
