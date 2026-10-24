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

/** Runs only exact pending tuples and reconstructs shared owners from server state. */
export async function reconcilePendingAgentControlLeases(
  leases: AgentControlLeaseView[],
  locks: Pick<NativeInputLockManager, 'syncAgentControlLease'>,
  acknowledge: LeaseAcknowledge,
  readTakeoverOwners: TakeoverOwnerReader,
): Promise<ReconcileSummary> {
  let processed = 0
  let unavailable = 0

  for (const lease of leases) {
    if (
      (lease.status !== 'locking' && lease.status !== 'unlocking') ||
      !Number.isSafeInteger(lease.generation) ||
      lease.generation < 1 ||
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
    throw new Error(`agent-control bridge returned HTTP ${response.status}`)
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
  locks: Pick<NativeInputLockManager, 'syncAgentControlLease'>,
  readTakeoverOwners: TakeoverOwnerReader,
): Promise<AgentControlLeaseView[]> {
  const { baseUrl, token } = await credentials()
  const leases = await listLeases(baseUrl, token)
  await reconcilePendingAgentControlLeases(
    leases,
    locks,
    (lease, status) => acknowledgeLease(baseUrl, token, lease, status),
    readTakeoverOwners,
  )
  return listLeases(baseUrl, token)
}

export interface StopAgentControlResult {
  confirmed: boolean
  leases: AgentControlLeaseView[]
}

/**
 * Requests cleanup for every exact MCP lease on one tab, drives pending ACKs,
 * and reports success only after the server has removed every lease.
 */
export async function stopAgentControlForTab(
  tabId: number,
  locks: Pick<NativeInputLockManager, 'syncAgentControlLease'>,
  readTakeoverOwners: TakeoverOwnerReader,
): Promise<StopAgentControlResult> {
  const { baseUrl, token } = await credentials()
  const current = await listLeases(baseUrl, token)
  const owned = current.filter((lease) => lease.tabId === tabId)
  for (const lease of owned) {
    await requestJson<void>(
      `${baseUrl}${LEASES_PATH}/${encodeURIComponent(lease.leaseId)}/release`,
      token,
      {
        method: 'POST',
        body: JSON.stringify({
          generation: lease.generation,
          tabId: lease.tabId,
          targetId: lease.targetId,
        }),
      },
    )
  }

  const deadline = Date.now() + ACK_DEADLINE_MS
  let latest = owned
  do {
    latest = await listLeases(baseUrl, token)
    // A stop request is scoped to this tab; it must not reconcile another tab's lease.
    await reconcilePendingAgentControlLeases(
      latest.filter((lease) => lease.tabId === tabId),
      locks,
      (lease, status) => acknowledgeLease(baseUrl, token, lease, status),
      readTakeoverOwners,
    )
    latest = (await listLeases(baseUrl, token)).filter(
      (lease) => lease.tabId === tabId,
    )
    if (latest.length === 0) return { confirmed: true, leases: [] }
    if (Date.now() >= deadline) break
    await new Promise((resolve) => setTimeout(resolve, STOP_POLL_MS))
  } while (true)

  return { confirmed: false, leases: latest }
}
