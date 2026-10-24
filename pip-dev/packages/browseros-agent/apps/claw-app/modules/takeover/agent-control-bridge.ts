import {
  readBrowserOSStringPref,
  resolveBrowserOSServerBaseUrl,
} from '@/modules/api/browseros-ports'
import type {
  AgentControlOwnerSnapshot,
  AgentLeaseLockStatus,
  NativeInputLockManager,
} from './native-input-lock'
import type { AgentControlLeaseSummary } from './takeover.types'

const TOKEN_PREF = 'browseros.server.agent_control_token'
const LEASES_PATH = '/api/v1/agent-control/leases'
const ACK_DEADLINE_MS = 8_000
const STOP_POLL_MS = 250

export type AgentControlLeaseView = AgentControlLeaseSummary

type AgentControlLocks = Pick<NativeInputLockManager, 'syncAgentControlLease'> &
  Partial<Pick<NativeInputLockManager, 'observeAgentControlLeaseSnapshot'>>

export type LeaseAcknowledge = (
  lease: AgentControlLeaseView,
  status: 'locked' | 'unlocked' | 'unavailable' | 'stale',
) => Promise<void>

export interface ReconcileSummary {
  processed: number
  unavailable: number
}

export type TakeoverOwnerReader = (
  tabId: number,
) => Promise<string[] | null>

function observeLeaseSnapshot(
  leases: AgentControlLeaseView[],
  locks: AgentControlLocks,
): void {
  const byTab = new Map<
    number,
    { snapshotRevision: number; agentLeases: Array<{ leaseId: string; targetId: string }> }
  >()
  for (const lease of leases) {
    if (
      !Number.isSafeInteger(lease.tabId) ||
      lease.tabId < 0 ||
      !Number.isSafeInteger(lease.snapshotRevision) ||
      lease.snapshotRevision < 1 ||
      !lease.leaseId ||
      !lease.targetId
    ) {
      continue
    }
    const snapshot = byTab.get(lease.tabId)
    if (!snapshot || lease.snapshotRevision > snapshot.snapshotRevision) {
      byTab.set(lease.tabId, {
        snapshotRevision: lease.snapshotRevision,
        agentLeases: [{ leaseId: lease.leaseId, targetId: lease.targetId }],
      })
    } else if (lease.snapshotRevision === snapshot.snapshotRevision) {
      snapshot.agentLeases.push({
        leaseId: lease.leaseId,
        targetId: lease.targetId,
      })
    }
  }
  for (const [tabId, snapshot] of byTab) {
    locks.observeAgentControlLeaseSnapshot?.(
      tabId,
      snapshot.snapshotRevision,
      snapshot.agentLeases,
    )
  }
}

/** Runs only exact pending tuples and reconstructs shared owners from server state. */
export async function reconcilePendingAgentControlLeases(
  leases: AgentControlLeaseView[],
  locks: AgentControlLocks,
  acknowledge: LeaseAcknowledge,
  readTakeoverOwners: TakeoverOwnerReader,
): Promise<ReconcileSummary> {
  observeLeaseSnapshot(leases, locks)
  let processed = 0
  let unavailable = 0

  for (const lease of leases) {
    if (
      (lease.status !== 'locking' && lease.status !== 'unlocking') ||
      !Number.isSafeInteger(lease.generation) ||
      lease.generation < 1 ||
      !Number.isSafeInteger(lease.snapshotRevision) ||
      lease.snapshotRevision < 1 ||
      !Number.isSafeInteger(lease.tabId) ||
      lease.tabId < 0 ||
      !lease.targetId
    ) {
      continue
    }

    const operation = lease.operation === 'lock' ? 'lock' : 'unlock'
    let result: AgentLeaseLockStatus
    try {
      const takeoverSessionIds = await readTakeoverOwners(lease.tabId)
      if (takeoverSessionIds === null) {
        result = 'unavailable'
      } else {
        const agentLeaseIds = leases
          .filter(
            (candidate) =>
              candidate.tabId === lease.tabId &&
              candidate.targetId === lease.targetId,
          )
          .map((candidate) => candidate.leaseId)
        if (!agentLeaseIds.includes(lease.leaseId)) {
          agentLeaseIds.push(lease.leaseId)
        }
        const ownerSnapshot: AgentControlOwnerSnapshot = {
          agentLeaseIds,
          takeoverSessionIds,
        }
        result = await locks.syncAgentControlLease(
          lease.tabId,
          lease.leaseId,
          lease.targetId,
          operation,
          lease.generation,
          lease.snapshotRevision,
          ownerSnapshot,
        )
      }
    } catch {
      result = 'unavailable'
    }

    const status =
      operation === 'lock'
        ? result === 'locked'
          ? 'locked'
          : result === 'stale'
            ? 'stale'
            : 'unavailable'
        : result === 'unlocked'
          ? 'unlocked'
          : result === 'stale'
            ? 'stale'
            : 'unavailable'

    // Do not ACK until CDP has completed and the manager reports the result.
    try {
      await acknowledge(lease, status)
      processed += 1
    } catch {
      unavailable += 1
    }
    if (status === 'unavailable') unavailable += 1
  }

  return { processed, unavailable }
}

async function credentials(): Promise<{ baseUrl: string; token: string }> {
  const [baseUrl, token] = await Promise.all([
    resolveBrowserOSServerBaseUrl(),
    readBrowserOSStringPref(TOKEN_PREF),
  ])
  if (!token) throw new Error('Pip Dev agent-control bridge secret is unavailable')
  return { baseUrl: baseUrl.replace(/\/$/, ''), token }
}

class AgentControlHttpError extends Error {
  constructor(readonly status: number) {
    super(`agent-control bridge returned HTTP ${status}`)
  }
}

export function refreshStopLease(
  initial: AgentControlLeaseView,
  latest: AgentControlLeaseView[],
): AgentControlLeaseView | undefined {
  return latest.find(
    (candidate) =>
      candidate.leaseId === initial.leaseId &&
      candidate.tabId === initial.tabId &&
      candidate.targetId === initial.targetId,
  )
}

async function requestJson<T>(
  url: string,
  token: string,
  init: RequestInit = {},
): Promise<T> {
  const response = await fetch(url, {
    ...init,
    cache: 'no-store',
    headers: {
      Authorization: `Bearer ${token}`,
      ...(init.body ? { 'Content-Type': 'application/json' } : {}),
      ...init.headers,
    },
  })
  if (!response.ok) {
    throw new AgentControlHttpError(response.status)
  }
  if (response.status === 204) return undefined as T
  return (await response.json()) as T
}

async function listLeases(
  baseUrl: string,
  token: string,
): Promise<AgentControlLeaseView[]> {
  const result = await requestJson<{ leases?: AgentControlLeaseView[] }>(
    `${baseUrl}${LEASES_PATH}`,
    token,
  )
  if (!Array.isArray(result.leases)) {
    throw new Error('agent-control bridge returned an invalid lease list')
  }
  return result.leases
}

async function acknowledgeLease(
  baseUrl: string,
  token: string,
  lease: AgentControlLeaseView,
  status: 'locked' | 'unlocked' | 'unavailable' | 'stale',
): Promise<void> {
  await requestJson<void>(
    `${baseUrl}${LEASES_PATH}/${encodeURIComponent(lease.leaseId)}/ack`,
    token,
    {
      method: 'POST',
      body: JSON.stringify({
        generation: lease.generation,
        tabId: lease.tabId,
        targetId: lease.targetId,
        status,
      }),
    },
  )
}

/** Reconciles one pending batch and returns a fresh status snapshot for UI. */
export async function pollAgentControlLeases(
  locks: AgentControlLocks,
  readTakeoverOwners: TakeoverOwnerReader,
): Promise<AgentControlLeaseView[]> {
  const { baseUrl, token } = await credentials()
  const leases = await listLeases(baseUrl, token)
  observeLeaseSnapshot(leases, locks)
  await reconcilePendingAgentControlLeases(
    leases,
    locks,
    (lease, status) => acknowledgeLease(baseUrl, token, lease, status),
    readTakeoverOwners,
  )
  const latest = await listLeases(baseUrl, token)
  observeLeaseSnapshot(latest, locks)
  return latest
}

export interface StopAgentControlResult {
  confirmed: boolean
  leases: AgentControlLeaseView[]
  restartRequired: boolean
}

/**
 * Requests cleanup for every exact MCP lease on one tab, drives pending ACKs,
 * and reports success only after the server has removed every lease.
 */
export async function stopAgentControlForTab(
  tabId: number,
  locks: AgentControlLocks,
  readTakeoverOwners: TakeoverOwnerReader,
): Promise<StopAgentControlResult> {
  const { baseUrl, token } = await credentials()
  const deadline = Date.now() + ACK_DEADLINE_MS
  const target = (await chrome.debugger.getTargets()).find(
    (candidate) => candidate.tabId === tabId && candidate.type === 'page',
  )
  if (!target?.id) throw new Error('The stopped tab target could not be verified')
  await requestJson<void>(
    `${baseUrl}/api/v1/agent-control/tabs/${tabId}/stop`,
    token,
    {
      method: 'POST',
      body: JSON.stringify({ targetId: target.id }),
    },
  )
  const current = await listLeases(baseUrl, token)
  observeLeaseSnapshot(current, locks)
  const owned = current.filter((lease) => lease.tabId === tabId && lease.targetId === target.id)
  for (const initial of owned) {
    let candidate = initial
    while (Date.now() < deadline) {
      try {
        await requestJson<void>(
          `${baseUrl}${LEASES_PATH}/${encodeURIComponent(candidate.leaseId)}/release`,
          token,
          {
            method: 'POST',
            body: JSON.stringify({
              generation: candidate.generation,
              tabId: candidate.tabId,
              targetId: candidate.targetId,
            }),
          },
        )
        break
      } catch (error) {
        if (!(error instanceof AgentControlHttpError) || error.status !== 409) {
          throw error
        }
        const refreshed = await listLeases(baseUrl, token)
        observeLeaseSnapshot(refreshed, locks)
        const next = refreshStopLease(initial, refreshed)
        if (!next || next.generation === candidate.generation) break
        // Stop intent follows this exact lease ID across its action generation,
        // but never releases a different lease that may have taken the tab later.
        candidate = next
      }
    }
  }

  let latest = owned
  do {
    latest = await listLeases(baseUrl, token)
    observeLeaseSnapshot(latest, locks)
    // A stop request is scoped to this tab; it must not reconcile another tab's lease.
    await reconcilePendingAgentControlLeases(
      latest.filter((lease) => lease.tabId === tabId && lease.targetId === target.id),
      locks,
      (lease, status) => acknowledgeLease(baseUrl, token, lease, status),
      readTakeoverOwners,
    )
    latest = await listLeases(baseUrl, token)
    observeLeaseSnapshot(latest, locks)
    latest = latest.filter((lease) => lease.tabId === tabId && lease.targetId === target.id)
    if (latest.length === 0) {
      const currentTarget = (await chrome.debugger.getTargets()).find(
        (candidate) => candidate.tabId === tabId && candidate.type === 'page',
      )
      if (currentTarget?.id !== target.id) {
        throw new Error('The stopped tab target changed; control return cannot be confirmed')
      }
      return { confirmed: true, leases: [], restartRequired: true }
    }
    if (Date.now() >= deadline) break
    await new Promise((resolve) => setTimeout(resolve, STOP_POLL_MS))
  } while (true)

  return { confirmed: false, leases: latest, restartRequired: true }
}
