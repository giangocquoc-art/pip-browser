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
export type AgentLeaseLockStatus = NativeInputLockStatus | 'stale'

/** Complete same-tab ownership state read from the authenticated local services. */
export interface AgentControlOwnerSnapshot {
  agentLeaseIds: string[]
  takeoverSessionIds: string[]
}

interface LockLease {
  status: NativeInputLockStatus | 'locking' | 'releasing'
  sessionKey: string
  attached: boolean
  retryAfterDetach: boolean
}

const debuggee = (tabId: number) => ({ tabId })

/**
 * Combines the existing takeover sessions and MCP lease owners before touching
 * Chromium. A stop/reconcile for one owner therefore cannot unlock another.
 */
export class NativeInputLockManager {
  private readonly leases = new Map<number, LockLease>()
  private readonly takeoverOwners = new Map<number, Set<string>>()
  private readonly agentLeases = new Map<number, Map<string, string>>()
  private readonly queues = new Map<number, Promise<void>>()

  constructor(private readonly debuggerApi: NativeInputDebugger) {
    debuggerApi.onDetach.addListener((source) => {
      if (typeof source.tabId !== 'number') return
      const lease = this.leases.get(source.tabId)
      if (lease) {
        lease.retryAfterDetach ||= lease.status === 'locked'
        lease.attached = false
        if (lease.status !== 'releasing') lease.status = 'unavailable'
      }
    })
  }

  status(tabId: number): NativeInputLockStatus {
    const status = this.leases.get(tabId)?.status
    return status === 'locked' || status === 'unavailable' ? status : 'unlocked'
  }

  /** Replaces only takeover owners; active MCP lease owners remain in force. */
  async sync(tabId: number, sessionIds: string[]): Promise<NativeInputLockStatus> {
    return this.exclusive(tabId, async () => {
      const unique = new Set(sessionIds.filter(Boolean))
      if (unique.size) this.takeoverOwners.set(tabId, unique)
      else this.takeoverOwners.delete(tabId)
      const status = await this.syncCombined(tabId)
      return status === 'stale' ? this.status(tabId) : status
    })
  }

  /**
   * Acquires/releases one MCP lease without activating a tab. Lock acquisition
   * verifies the current CDP target id before issuing Input.setAgentInputLock.
   */
  async syncAgentControlLease(
    tabId: number,
    leaseId: string,
    targetId: string,
    operation: 'lock' | 'unlock',
    ownerSnapshot?: AgentControlOwnerSnapshot,
  ): Promise<AgentLeaseLockStatus> {
    return this.exclusive(tabId, async () => {
      if (ownerSnapshot) {
        const takeoverOwners = new Set(ownerSnapshot.takeoverSessionIds.filter(Boolean))
        if (takeoverOwners.size) this.takeoverOwners.set(tabId, takeoverOwners)
        else this.takeoverOwners.delete(tabId)

        const agentOwners = new Map<string, string>()
        for (const id of new Set(ownerSnapshot.agentLeaseIds.filter(Boolean))) {
          agentOwners.set(id, targetId)
        }
        if (agentOwners.size) this.agentLeases.set(tabId, agentOwners)
        else this.agentLeases.delete(tabId)
      }

      const agentOwners = this.agentLeases.get(tabId) ?? new Map<string, string>()
      const existingTargetId = agentOwners.get(leaseId)
      if (existingTargetId && existingTargetId !== targetId) return 'stale'

      if (operation === 'unlock') {
        // The authenticated server snapshot is required to rehydrate an owner
        // after worker restart. Without it, an absent map is not proof of release.
        if (!existingTargetId) return 'unavailable'
        agentOwners.delete(leaseId)
        if (agentOwners.size) this.agentLeases.set(tabId, agentOwners)
        else this.agentLeases.delete(tabId)

        const hasOtherOwners =
          agentOwners.size > 0 || (this.takeoverOwners.get(tabId)?.size ?? 0) > 0
        if (!hasOtherOwners && !this.leases.has(tabId)) {
          return this.unlockExactTarget(tabId, targetId)
        }

        const status = await this.syncCombined(tabId, targetId)
        // Removing one owner must preserve and verify every other owner.
        if (status === 'unavailable' || status === 'stale') return status
        return 'unlocked'
      }

      if (!targetId) return 'stale'
      const currentLease = this.leases.get(tabId)
      let attachedHere = false
      try {
        if (!currentLease?.attached) {
          await this.debuggerApi.attach(debuggee(tabId), '1.3')
          attachedHere = true
        }
        const actualTargetId = await this.currentTargetId(tabId)
        if (!actualTargetId || actualTargetId !== targetId) {
          if (attachedHere) await this.debuggerApi.detach(debuggee(tabId))
          return 'stale'
        }
      } catch {
        if (attachedHere) {
          try {
            await this.debuggerApi.detach(debuggee(tabId))
          } catch {}
        }
        return 'unavailable'
      }

      // Keep the verified attachment for the native lock command. No chrome.tabs
      // activation/navigation call exists in this path, so background tabs stay put.
      if (attachedHere && !currentLease) {
        this.leases.set(tabId, {
          status: 'locking',
          sessionKey: '',
          attached: true,
          retryAfterDetach: false,
        })
      } else if (attachedHere && currentLease) {
        currentLease.attached = true
      }
      agentOwners.set(leaseId, targetId)
      this.agentLeases.set(tabId, agentOwners)
      return this.syncCombined(tabId, targetId)
    })
  }

  /**
   * Clears takeover ownership only. An MCP lease or another takeover owner can
   * keep the native lock held until its own exact release arrives.
   */
  async release(
    tabId: number,
    mayRelease: () => boolean = () => true,
  ): Promise<void> {
    await this.exclusive(tabId, async () => {
      this.takeoverOwners.delete(tabId)
      await this.syncCombined(tabId, undefined, mayRelease)
    })
  }

  /** Tab removal ends every owner, unlike takeover Stop which clears one owner set. */
  async removeTab(tabId: number): Promise<void> {
    await this.exclusive(tabId, async () => {
      this.takeoverOwners.delete(tabId)
      this.agentLeases.delete(tabId)
      await this.releaseNative(tabId)
    })
  }

  private async syncCombined(
    tabId: number,
    expectedTargetId?: string,
    mayRelease: () => boolean = () => true,
  ): Promise<NativeInputLockStatus | 'stale'> {
    const owners = [
      ...(this.takeoverOwners.get(tabId) ?? []),
      ...[...(this.agentLeases.get(tabId)?.keys() ?? [])].map((id) => `agent:${id}`),
    ]
    const sessionKey = [...new Set(owners)].sort().join('\0')
    if (!sessionKey) {
      await this.releaseNative(tabId, mayRelease)
      return this.status(tabId)
    }

    const existing = this.leases.get(tabId)
    if (existing?.status === 'locked') {
      if (expectedTargetId) {
        try {
          if (await this.currentTargetId(tabId) !== expectedTargetId) return 'stale'
        } catch {
          return 'unavailable'
        }
      }
      existing.sessionKey = sessionKey
      return 'locked'
    }
    const retryDetachedLease =
      existing?.status === 'unavailable' &&
      existing.sessionKey === sessionKey &&
      existing.retryAfterDetach
    if (
      existing?.status === 'unavailable' &&
      existing.sessionKey === sessionKey &&
      !retryDetachedLease
    ) {
      return 'unavailable'
    }

    const lease: LockLease = existing ?? {
      status: 'locking',
      sessionKey,
      attached: false,
      retryAfterDetach: Boolean(retryDetachedLease),
    }
    lease.status = 'locking'
    lease.sessionKey = sessionKey
    this.leases.set(tabId, lease)
    let attachedHere = false
    try {
      if (!lease.attached) {
        await this.debuggerApi.attach(debuggee(tabId), '1.3')
        attachedHere = true
        lease.attached = true
      }
      if (expectedTargetId) {
        const actualTargetId = await this.currentTargetId(tabId)
        if (!actualTargetId || actualTargetId !== expectedTargetId) {
          if (attachedHere) {
            await this.debuggerApi.detach(debuggee(tabId))
            lease.attached = false
            if (this.leases.get(tabId) === lease && !lease.sessionKey) {
              this.leases.delete(tabId)
            }
          }
          return 'stale'
        }
      }
      await this.debuggerApi.sendCommand(
        debuggee(tabId),
        'Input.setAgentInputLock',
        { locked: true },
      )
      if (this.leases.get(tabId) !== lease || this.leases.get(tabId)?.status === 'unavailable') {
        throw new Error('debugger detached while enabling the input lock')
      }
      lease.status = 'locked'
      lease.retryAfterDetach = false
      return 'locked'
    } catch {
      if (attachedHere) {
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
  }

  /**
   * Recovers cleanup after the extension worker lost its in-memory lock record.
   * The caller has already reconciled the complete server/takeover owner snapshot;
   * this path is used only when that snapshot leaves no remaining owner.
   */
  private async unlockExactTarget(
    tabId: number,
    expectedTargetId: string,
  ): Promise<AgentLeaseLockStatus> {
    let attachedHere = false
    try {
      let actualTargetId: string | null
      try {
        actualTargetId = await this.currentTargetId(tabId)
      } catch {
        await this.debuggerApi.attach(debuggee(tabId), '1.3')
        attachedHere = true
        actualTargetId = await this.currentTargetId(tabId)
      }
      if (!actualTargetId || actualTargetId !== expectedTargetId) {
        if (attachedHere) await this.debuggerApi.detach(debuggee(tabId))
        return 'stale'
      }

      await this.debuggerApi.sendCommand(
        debuggee(tabId),
        'Input.setAgentInputLock',
        { locked: false },
      )
      // Explicit release plus a successful detach confirms both the native
      // lock command and browser-owned debugger lease are gone.
      await this.debuggerApi.detach(debuggee(tabId))
      this.leases.delete(tabId)
      return 'unlocked'
    } catch {
      if (attachedHere) {
        try {
          await this.debuggerApi.detach(debuggee(tabId))
        } catch {
          this.leases.set(tabId, {
            status: 'unavailable',
            sessionKey: '',
            attached: true,
            retryAfterDetach: false,
          })
        }
      }
      return 'unavailable'
    }
  }

  private async currentTargetId(tabId: number): Promise<string | null> {
    const result = await this.debuggerApi.sendCommand(
      debuggee(tabId),
      'Target.getTargetInfo',
    )
    if (!result || typeof result !== 'object') return null
    const value = result as {
      targetId?: unknown
      targetInfo?: { targetId?: unknown }
    }
    const targetId = value.targetInfo?.targetId ?? value.targetId
    return typeof targetId === 'string' ? targetId : null
  }

  private async releaseNative(
    tabId: number,
    mayRelease: () => boolean = () => true,
  ): Promise<void> {
    const lease = this.leases.get(tabId)
    if (lease?.status === 'unavailable') {
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
