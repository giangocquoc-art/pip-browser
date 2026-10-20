/**
 * @license
 * Copyright 2025 BrowserOS
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * Owns a native per-tab input-lock lease through Chromium's debugger API. The
 * browser build implements Input.setAgentInputLock at the native event layer;
 * CDP input remains available to the agent. A failed/unsupported command is
 * surfaced to the UI and never reported as a successful lock.
 */

export interface NativeInputDebugger {
  attach(target: { tabId: number }, version: string): Promise<void>
  sendCommand(
    target: { tabId: number },
    method: string,
    commandParams?: object,
  ): Promise<unknown>
  detach(target: { tabId: number }): Promise<void>
  onDetach: {
    addListener(listener: (source: { tabId?: number }, reason: string) => void): void
  }
}

export type NativeInputLockStatus = 'locked' | 'unavailable' | 'unlocked'

interface LockLease {
  status: NativeInputLockStatus | 'locking'
  sessionKey: string
}

const debuggee = (tabId: number) => ({ tabId })

export class NativeInputLockManager {
  private readonly leases = new Map<number, LockLease>()
  private readonly queues = new Map<number, Promise<void>>()

  constructor(private readonly debuggerApi: NativeInputDebugger) {
    debuggerApi.onDetach.addListener((source) => {
      if (typeof source.tabId !== 'number') return
      const lease = this.leases.get(source.tabId)
      if (lease) lease.status = 'unavailable'
    })
  }

  status(tabId: number): NativeInputLockStatus {
    const status = this.leases.get(tabId)?.status
    return status === 'locked' || status === 'unavailable' ? status : 'unlocked'
  }

  async sync(tabId: number, sessionIds: string[]): Promise<NativeInputLockStatus> {
    const sessionKey = [...new Set(sessionIds)].sort().join('\0')
    if (!sessionKey) {
      await this.release(tabId)
      return 'unlocked'
    }

    return this.exclusive(tabId, async () => {
      const existing = this.leases.get(tabId)
      if (existing?.status === 'locked') {
        existing.sessionKey = sessionKey
        return 'locked'
      }
      if (existing?.status === 'unavailable' && existing.sessionKey === sessionKey) {
        return 'unavailable'
      }

      const lease: LockLease = { status: 'locking', sessionKey }
      this.leases.set(tabId, lease)
      let attached = false
      try {
        await this.debuggerApi.attach(debuggee(tabId), '1.3')
        attached = true
        await this.debuggerApi.sendCommand(
          debuggee(tabId),
          'Input.setAgentInputLock',
          { locked: true },
        )
        if (this.leases.get(tabId) !== lease || lease.status === 'unavailable') {
          throw new Error('debugger detached while enabling the input lock')
        }
        lease.status = 'locked'
        return 'locked'
      } catch {
        if (attached) {
          try {
            await this.debuggerApi.sendCommand(
              debuggee(tabId),
              'Input.setAgentInputLock',
              { locked: false },
            )
          } catch {}
          try {
            await this.debuggerApi.detach(debuggee(tabId))
          } catch {}
        }
        lease.status = 'unavailable'
        return 'unavailable'
      }
    })
  }

  async release(tabId: number): Promise<void> {
    await this.exclusive(tabId, async () => {
      const lease = this.leases.get(tabId)
      this.leases.delete(tabId)
      if (lease?.status !== 'locked' && lease?.status !== 'locking') return
      try {
        await this.debuggerApi.sendCommand(
          debuggee(tabId),
          'Input.setAgentInputLock',
          { locked: false },
        )
      } catch {
        // Detaching still drops the browser-owned lease if the explicit release
        // command is unavailable or the target is already gone.
      }
      try {
        await this.debuggerApi.detach(debuggee(tabId))
      } catch {}
    })
  }

  private async exclusive<T>(tabId: number, operation: () => Promise<T>): Promise<T> {
    const previous = this.queues.get(tabId) ?? Promise.resolve()
    const current = previous.catch(() => {}).then(operation)
    const settled = current.then(
      () => undefined,
      () => undefined,
    )
    this.queues.set(tabId, settled)
    try {
      return await current
    } finally {
      if (this.queues.get(tabId) === settled) this.queues.delete(tabId)
    }
  }
}
