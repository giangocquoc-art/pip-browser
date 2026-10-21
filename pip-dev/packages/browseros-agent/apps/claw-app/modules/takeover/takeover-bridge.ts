/**
 * @license
 * Copyright 2025 BrowserOS
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * Background side of the in-page takeover bar. Content scripts on every tab poll
 * this; one short-lived cache coalesces those polls into a single live-sessions
 * read, and resolve goes out from here (the background holds host permissions,
 * so it avoids the page-origin CORS the content script would hit).
 */

import { SessionStatus, type SessionSummary } from '@browseros/claw-api'
import { ClawApiClient } from '@browseros/claw-api-client'
import { agentControlForTab, takeoverForTab } from './live-help'
import type {
  AgentControlContext,
  TakeoverPollResponse,
} from './takeover.types'

const CACHE_TTL_MS = 1200

export interface TakeoverBridgeOptions {
  resolveServerBaseUrl: () => Promise<string>
  now?: () => number
}

export interface TakeoverBridge {
  pollForTab: (tabId: number) => Promise<TakeoverPollResponse>
  refreshForTab: (tabId: number) => Promise<TakeoverPollResponse>
  resolve: (
    tabId: number,
    sessionId: string,
    note?: string,
    beforeResume?: (context: AgentControlContext | null) => Promise<void>,
  ) => Promise<boolean>
  stopForTab: (tabId: number) => Promise<{ stopped: number; total: number }>
}

export function createTakeoverBridge(
  options: TakeoverBridgeOptions,
): TakeoverBridge {
  const now = options.now ?? (() => Date.now())
  let cache: { at: number; sessions: SessionSummary[] } | null = null
  let inflight: Promise<SessionSummary[] | null> | null = null

  const client = async () =>
    new ClawApiClient(await options.resolveServerBaseUrl(), { fetch })

  const liveSessions = async (): Promise<SessionSummary[] | null> => {
    if (cache && now() - cache.at < CACHE_TTL_MS) return cache.sessions
    if (inflight) return inflight
    inflight = (async () => {
      try {
        const list = await (await client()).listSessions({
          status: SessionStatus.Live,
        })
        cache = { at: now(), sessions: list.items }
        return list.items
      } catch {
        // When the server is unavailable, hide the affordance instead of
        // presenting an active agent state from an old session snapshot.
        cache = null
        return null
      } finally {
        inflight = null
      }
    })()
    return inflight
  }

  const freshLiveSessions = async (): Promise<SessionSummary[]> => {
    const list = await (await client()).listSessions({ status: SessionStatus.Live })
    cache = { at: now(), sessions: list.items }
    return list.items
  }

  const contextForTab = (
    sessions: SessionSummary[],
    tabId: number,
  ): TakeoverPollResponse => {
    const helpRequest = takeoverForTab(sessions, tabId)
    const agentControl = agentControlForTab(sessions, tabId)
    const context = agentControl
      ? { ...agentControl, helpRequest: helpRequest ?? undefined }
      : helpRequest
    return context ? { active: true, context } : { active: false }
  }

  return {
    async pollForTab(tabId) {
      const sessions = await liveSessions()
      if (!sessions) return { active: false, unavailable: true }
      return contextForTab(sessions, tabId)
    },
    async refreshForTab(tabId) {
      return contextForTab(await freshLiveSessions(), tabId)
    },
    async resolve(tabId, sessionId, note, beforeResume) {
      const sessions = await freshLiveSessions()
      const helpRequest = takeoverForTab(sessions, tabId)
      if (!helpRequest || helpRequest.sessionId !== sessionId) return false

      // Lock the tab before waking the agent. The caller can therefore make
      // the hand-back atomic with respect to the human's page interaction.
      await beforeResume?.(agentControlForTab(sessions, tabId))
      const result = await (await client()).resolveHelp({ sessionId, note })
      if (!result.resolved) return false
      // Drop the cache so the next poll reflects the resolve and clears the bar.
      cache = null
      return true
    },
    async stopForTab(tabId) {
      // Never let a stale poll cache decide which sessions get cancelled.
      const sessions = await freshLiveSessions()
      const context = agentControlForTab(sessions, tabId)
      if (!context) return { stopped: 0, total: 0 }

      const results = await Promise.allSettled(
        context.sessionIds.map((sessionId) =>
          client().then((api) => api.cancelSession({ sessionId })),
        ),
      )
      cache = null
      return {
        stopped: results.filter((result) => result.status === 'fulfilled').length,
        total: context.sessionIds.length,
      }
    },
  }
}
