import {
  readBrowserOSStringPref,
  resolveBrowserOSServerBaseUrl,
} from '@/modules/api/browseros-ports'
import type {
  AgentLeaseLockStatus,
  NativeInputLockManager,
} from './native-input-lock'

const TOKEN_PREF = 'browseros.server.agent_control_token'
const LEASES_PATH = '/api/v1/agent-control/leases'
const ACK_DEADLINE_MS = 8_000
const STOP_POLL_MS = 250

export interface AgentControlLeaseView {
  leaseId: string
  pageId: number
  tabId: number
  targetId: string
  operation: 'lock' | 'unlock'
  status: 'locking' | 'locked' | 'unlocking' | 'failed'
  error?: string | null
  expiresAtMs: number
}

export type LeaseAcknowledge = (
  lease: AgentControlLeaseView,
  status: 'locked' | 'unlocked' | 'unavailable' | 'stale',
) => Promise<void>

export interface ReconcileSummary {
  processed: number
  unavailable: number
}

/** Runs only the exact pending tuples returned by the authenticated lease API. */
export async function reconcilePendingAgentControlLeases(
  leases: AgentControlLeaseView[],
  locks: Pick<NativeInputLockManager, 'syncAgentControlLease'>,
  acknowledge: LeaseAcknowledge,
): Promise<ReconcileSummary> {
  let processed = 0
  let unavailable = 0

  for (const lease of leases) {
    if (
      (lease.status !== 'locking' && lease.status !== 'unlocking') ||
      !Number.isSafeInteger(lease.tabId) ||
      lease.tabId < 0 ||
      !lease.targetId
    ) {
      continue
    }

    const operation = lease.operation === 'lock' ? 'lock' : 'unlock'
    let result: AgentLeaseLockStatus
    try {
      result = await locks.syncAgentControlLease(
        lease.tabId,
        lease.leaseId,
        lease.targetId,
        operation,
      )
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
): Promise<AgentControlLeaseView[]> {
  const { baseUrl, token } = await credentials()
  const leases = await listLeases(baseUrl, token)
  await reconcilePendingAgentControlLeases(
    leases,
    locks,
    (lease, status) => acknowledgeLease(baseUrl, token, lease, status),
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
    await reconcilePendingAgentControlLeases(
      latest,
      locks,
      (lease, status) => acknowledgeLease(baseUrl, token, lease, status),
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
