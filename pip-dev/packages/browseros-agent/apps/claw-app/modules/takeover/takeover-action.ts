/**
 * @license
 * Copyright 2025 BrowserOS
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * Routes the browser-toolbar emergency stop to the popup's captured active tab.
 * This control remains reachable when Chromium suppresses page input for an agent.
 */

import type { TakeoverPollResponse } from './takeover.types'

export interface TakeoverActionRequest {
  type: 'takeover-action-poll' | 'takeover-action-stop'
  tabId: number
  windowId: number
}

export interface TakeoverActionSender {
  id?: string
  url?: string
}

export interface TakeoverActionDependencies {
  extensionId: string
  popupUrl: string
  getActiveTab: (
    windowId: number,
  ) => Promise<{ id?: number; windowId?: number } | undefined>
  pollForTab: (tabId: number) => Promise<TakeoverPollResponse>
  stopForTab: (tabId: number) => Promise<{ stopped: number; total: number }>
  stopAgentControlForTab?: (tabId: number) => Promise<{
    confirmed: boolean
    pending: number
    restartRequired: boolean
  }>
  beginStop: (tabId: number) => void
  finishStop: (tabId: number) => void
  /** Re-read live ownership and keep/release the lock to match the latest state. */
  reconcileLock: (tabId: number) => Promise<void>
}

export type TakeoverActionResponse =
  | { ok: true; action: 'poll'; response: TakeoverPollResponse }
  | {
      ok: true
      action: 'stop'
      stopped: number
      total: number
      agentControlConfirmed?: boolean
      agentControlRestartRequired?: boolean
    }
  | {
      ok: false
      action?: 'poll' | 'stop'
      reason:
        | 'untrusted-sender'
        | 'invalid-request'
        | 'inactive-tab'
        | 'unavailable'
        | 'partial-stop'
      stopped?: number
      total?: number
      agentControlConfirmed?: boolean
      agentControlPending?: number
    }

export function isTakeoverActionRequest(
  value: unknown,
): value is TakeoverActionRequest {
  if (!value || typeof value !== 'object') return false
  const request = value as Partial<TakeoverActionRequest>
  return (
    (request.type === 'takeover-action-poll' ||
      request.type === 'takeover-action-stop') &&
    Number.isSafeInteger(request.tabId) &&
    Number(request.tabId) >= 0 &&
    Number.isSafeInteger(request.windowId) &&
    Number(request.windowId) >= 0
  )
}

export function takeoverStopSuccessMessage(result: {
  stopped: number
  total: number
  agentControlRestartRequired?: boolean
}): string {
  const message = result.total === 0
    ? 'Không còn phiên agent trên tab này; đã xác nhận trả quyền thao tác.'
    : `Đã dừng ${result.stopped}/${result.total} phiên agent trên tab này; đã xác nhận mở khóa.`
  return result.agentControlRestartRequired
    ? `${message} Khởi động lại Pip Dev để cho phép agent điều khiển lại tab.`
    : message
}

export async function handleTakeoverAction(
  message: unknown,
  sender: TakeoverActionSender,
  dependencies: TakeoverActionDependencies,
): Promise<TakeoverActionResponse | null> {
  if (
    !message ||
    typeof message !== 'object' ||
    !('type' in message) ||
    ((message as { type?: unknown }).type !== 'takeover-action-poll' &&
      (message as { type?: unknown }).type !== 'takeover-action-stop')
  ) {
    return null
  }

  if (
    sender.id !== dependencies.extensionId ||
    sender.url !== dependencies.popupUrl
  ) {
    return { ok: false, reason: 'untrusted-sender' }
  }
  if (!isTakeoverActionRequest(message)) {
    return { ok: false, reason: 'invalid-request' }
  }

  const { type, tabId, windowId } = message
  const targetIsActive = async () => {
    const activeTab = await dependencies.getActiveTab(windowId)
    return activeTab?.id === tabId && activeTab.windowId === windowId
  }

  try {
    if (!(await targetIsActive())) {
      return {
        ok: false,
        action: type === 'takeover-action-poll' ? 'poll' : 'stop',
        reason: 'inactive-tab',
      }
    }

    if (type === 'takeover-action-poll') {
      return {
        ok: true,
        action: 'poll',
        response: await dependencies.pollForTab(tabId),
      }
    }

    dependencies.beginStop(tabId)
    try {
      // Revalidate after entering the stop gate so a stale popup cannot cancel
      // sessions for a tab the user has already switched away from.
      if (!(await targetIsActive())) {
        return { ok: false, action: 'stop', reason: 'inactive-tab' }
      }
      const { stopped, total } = await dependencies.stopForTab(tabId)
      const agentControl = dependencies.stopAgentControlForTab
        ? await dependencies.stopAgentControlForTab(tabId)
        : undefined
      if ((total > 0 && stopped !== total) || agentControl?.confirmed === false) {
        return {
          ok: false,
          action: 'stop',
          reason: 'partial-stop',
          stopped,
          total,
          ...(agentControl
            ? {
                agentControlConfirmed: agentControl.confirmed,
                agentControlPending: agentControl.pending,
              }
            : {}),
        }
      }
      return {
        ok: true,
        action: 'stop',
        stopped,
        total,
        ...(agentControl
          ? {
              agentControlConfirmed: true,
              agentControlRestartRequired: agentControl.restartRequired,
            }
          : {}),
      }
    } finally {
      try {
        await dependencies.reconcileLock(tabId)
      } finally {
        dependencies.finishStop(tabId)
      }
    }
  } catch {
    return {
      ok: false,
      action: type === 'takeover-action-poll' ? 'poll' : 'stop',
      reason: 'unavailable',
    }
  }
}
