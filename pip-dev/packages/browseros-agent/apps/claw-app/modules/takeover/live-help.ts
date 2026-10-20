/**
 * @license
 * Copyright 2025 BrowserOS
 * SPDX-License-Identifier: AGPL-3.0-or-later
 */

import type { SessionSummary } from '@browseros/claw-api'
import type {
  AgentControlContext,
  TakeoverContext,
} from './takeover.types'

function hostOf(url: string | undefined): string | undefined {
  if (!url) return undefined
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return undefined
  }
}

/**
 * The takeover context for a Chrome tab, or null when no live session is parked
 * on that exact tab. Pairs with the tab pinned on the help request at request
 * time, so the bar lands on the blocked page even when the agent has many tabs.
 */
export function takeoverForTab(
  sessions: SessionSummary[],
  tabId: number,
): TakeoverContext | null {
  for (const session of sessions) {
    const help = session.live?.helpRequest
    if (help && help.browserTabId === tabId) {
      return {
        mode: 'help',
        sessionId: session.sessionId,
        agentLabel: session.label || session.slug,
        reason: help.reason,
        resumeHint: help.resumeHint,
        site: hostOf(help.url) ?? help.title,
        requestedAt: help.requestedAt,
      }
    }
  }
  return null
}

/**
 * Returns the live sessions whose current browser tab set contains this tab.
 * This is an emergency stop affordance, not a native input lock: cancelling a
 * session stops that whole agent session, including any of its other tabs.
 */
export function agentControlForTab(
  sessions: SessionSummary[],
  tabId: number,
): AgentControlContext | null {
  const matches = sessions.filter((session) =>
    session.live?.browserTabs.some((tab) => tab.browserTabId === tabId),
  )
  if (matches.length === 0) return null

  const labels = [...new Set(matches.map((session) => session.label || session.slug))]
  const sites = [
    ...new Set(
      matches.flatMap((session) =>
        (session.live?.browserTabs ?? [])
          .filter((tab) => tab.browserTabId === tabId)
          .map((tab) => hostOf(tab.url))
          .filter((host): host is string => Boolean(host)),
      ),
    ),
  ]

  return {
    mode: 'agent-control',
    sessionIds: matches.map((session) => session.sessionId),
    agentLabels: labels,
    sites,
    since: Math.min(...matches.map((session) => session.startedAt)),
  }
}
