/**
 * @license
 * Copyright 2025 BrowserOS
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * Messages between the in-page takeover bar (content script) and the background.
 * The content script never talks to the claw server directly: a page-origin fetch
 * would be a cross-origin request, so the background (which holds host
 * permissions) does the lookups and the resolve.
 */

/** What the bar needs to render, for the tab the message is answered on. */
export interface TakeoverContext {
  mode: 'help'
  sessionId: string
  requestId: string
  agentLabel: string
  reason: string
  resumeHint?: string
  site?: string
  /** The extension could not reach the session service during its last poll. */
  connectionLost?: boolean
  /** Epoch ms the request opened, for the waiting timer. */
  requestedAt: number
}

/** A live agent session currently has this browser tab in its controlled tab set. */
export interface AgentControlContext {
  mode: 'agent-control'
  sessionIds: string[]
  agentLabels: string[]
  sites: string[]
  since: number
  helpRequest?: TakeoverContext
  stopping?: boolean
  stopFailed?: boolean
  /** The extension could not reach the session service during its last poll. */
  connectionLost?: boolean
  /** Whether native per-tab input suppression is active in the browser build. */
  nativeInputLock?: 'locked' | 'unavailable' | 'unlocked'
}

export type InPageControlContext = TakeoverContext | AgentControlContext

/**
 * Background's answer to a `takeover-poll`, resolved from the live help request
 * for the sender's tab.
 */
export type TakeoverPollResponse =
  | { active: true; context: InPageControlContext }
  | { active: false }
  | {
      active: false
      unavailable: true
      nativeInputLock?: 'locked' | 'unavailable' | 'unlocked'
    }

/** A `takeover-resolve`: the human handed control back from the bar. */
export interface TakeoverResolveMessage {
  type: 'takeover-resolve'
  sessionId: string
  requestId: string
  note?: string
}

/** Stops the active agent sessions that currently include the sender's tab. */
export interface TakeoverStopMessage {
  type: 'takeover-stop'
}
