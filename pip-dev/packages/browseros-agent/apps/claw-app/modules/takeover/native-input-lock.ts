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

import type { AgentControlContext } from './takeover.types'

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

/**
 * A session waiting for a human-help response is paused and must give the
 * browser tab back to that human. Keep locks held by any other live sessions
 * that still control the same tab.
 */
export function nativeInputLockSessionIds(
  context: AgentControlContext,
): string[] {
  const waitingSessionId = context.helpRequest?.sessionId
  return context.sessionIds.filter((sessionId) => sessionId !== waitingSessionId)
}

export type NativeInputLockStatus = 'locked' | 'unavailable' | 'unlocked'

interface LockLease {
  status: NativeInputLockStatus | 'locking' | 'releasing'
  sessionKey: string
  attached: boolean
}

const debuggee = (tabId: number) => ({ tabId })

export class NativeInputLockManager {
  private readonly leases = new Map<number, LockLease>()
  private readonly queues = new Map<number, Promise<void>>()

  constructor(private readonly debuggerApi: NativeInputDebugger) {
    debuggerApi.onDetach.addListener((source) => {
      if (typeof source.tabId !== 'number') return
      const lease = this.leases.get(source.tabId)
      if (lease) {
        lease.attached = false
        if (lease.status !== 'releasing') lease.status = 'unavailable'
      }
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

      const lease: LockLease = { status: 'locking', sessionKey, attached: false }
      this.leases.set(tabId, lease)
      let attached = false
      try {
        await this.debuggerApi.attach(debuggee(tabId), '1.3')
        attached = true
        lease.attached = true
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
            lease.attached = false
          } catch {
            lease.attached = true
          }
        }
        lease.status = 'unavailable'
        return 'unavailable'
      }
    })
  }

  async release(
    tabId: number,
    mayRelease: () => boolean = () => true,
  ): Promise<void> {
    await this.exclusive(tabId, async () => {
      const lease = this.leases.get(tabId)
      if (lease?.status === 'unavailable') {
        await this.releaseUnavailableLease(tabId, lease, mayRelease)
        return
      }
      if (lease?.status !== 'locked' && lease?.status !== 'locking') return
      if (!mayRelease()) return
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
      if (!mayRelease()) {
        await this.restoreAfterSupersededRelease(tabId, lease)
        return
      }
      lease.status = 'releasing'
      try {
        await this.debuggerApi.detach(debuggee(tabId))
        lease.attached = false
      } catch {
        // A failed detach does not prove Chromium dropped the native lease.
        // Keep the unavailable record so later reconciliations can retry.
        lease.status = 'unavailable'
      }
      if (!mayRelease()) {
        await this.restoreAfterSupersededDetach(tabId, lease)
        return
      }
      if (!lease.attached && this.leases.get(tabId) === lease) {
        this.leases.delete(tabId)
      }
    })
  }

  private async releaseUnavailableLease(
    tabId: number,
    lease: LockLease,
    mayRelease: () => boolean,
  ): Promise<void> {
    if (!mayRelease()) return
    if (!lease.attached) {
      if (this.leases.get(tabId) === lease) this.leases.delete(tabId)
      return
    }
    lease.status = 'releasing'
    try {
      await this.debuggerApi.detach(debuggee(tabId))
      lease.attached = false
    } catch {
      // Preserve the lease when its final attachment state is uncertain.
      lease.status = 'unavailable'
    }
    if (!mayRelease()) {
      await this.restoreAfterSupersededDetach(tabId, lease)
      return
    }
    if (!lease.attached && this.leases.get(tabId) === lease) {
      this.leases.delete(tabId)
    }
  }

  private async restoreAfterSupersededRelease(
    tabId: number,
    lease: LockLease,
  ): Promise<void> {
    try {
      await this.debuggerApi.sendCommand(
        debuggee(tabId),
        'Input.setAgentInputLock',
        { locked: true },
      )
      if (this.leases.get(tabId) === lease) {
        lease.status = 'locked'
        lease.attached = true
      }
    } catch {
      // The browser may now be unlocked. Retain the debugger connection and
      // lease so a later final reconciliation can clean it up; never detach
      // after failing to restore the native lock.
      lease.status = 'unavailable'
      lease.attached = true
      throw new Error('could not restore the agent input lock after a stale release')
    }
  }

  private async restoreAfterSupersededDetach(
    tabId: number,
    lease: LockLease,
  ): Promise<void> {
    lease.status = 'locking'
    try {
      if (!lease.attached) {
        await this.debuggerApi.attach(debuggee(tabId), '1.3')
        lease.attached = true
      }
      await this.debuggerApi.sendCommand(
        debuggee(tabId),
        'Input.setAgentInputLock',
        { locked: true },
      )
      if (this.leases.get(tabId) === lease) lease.status = 'locked'
    } catch {
      // CDP detach has already relinquished the old native lease. Keep any new
      // debugger attachment and report unavailable if a newer owner arrived
      // before we could recreate the lock.
      lease.status = 'unavailable'
      throw new Error('could not reacquire the agent input lock after a stale detach')
    }
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

/**
 * Require an acknowledged native lock before allowing a paused agent to resume.
 */
export async function requireAcknowledgedNativeInputLock(
  manager: Pick<NativeInputLockManager, 'sync'>,
  tabId: number,
  sessionIds: string[],
): Promise<void> {
  const status = await manager.sync(tabId, sessionIds)
  if (status !== 'locked') {
    throw new Error('native input lock was not acknowledged before hand-back')
  }
}

/**
 * Release a tab lease when its controlling session disappears and report the
 * manager's observed state even if CDP cleanup fails. Callers must not infer
 * that an attempted release succeeded merely because the session is offline.
 */
export async function releaseAndGetNativeInputLockStatus(
  manager: Pick<NativeInputLockManager, 'release' | 'status'>,
  tabId: number,
): Promise<NativeInputLockStatus> {
  try {
    await manager.release(tabId)
  } catch {
    // status() preserves unavailable when the native lock could not be safely
    // released or restored. Keep responding to the extension caller.
  }
  return manager.status(tabId)
}
