/**
 * @license
 * Copyright 2025 BrowserOS
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * Renders session status and human-help controls on the exact tab an agent
 * can access, so the human can respond without leaving the page. The content
 * script never reaches the claw server directly (a page-origin fetch would be
 * cross-origin); it asks the background, which holds host permissions, whether
 * its own tab is the blocked one and relays the hand-back.
 */

import { createShadowRootUi } from 'wxt/utils/content-script-ui/shadow-root'
import { defineContentScript } from 'wxt/utils/define-content-script'
import type {
  InPageControlContext,
  TakeoverPollResponse,
} from '@/modules/takeover/takeover.types'
import {
  createTakeoverBar,
  TAKEOVER_BAR_CSS,
  type TakeoverBar,
} from '@/modules/takeover/takeover-bar'

const ACTIVE_POLL_INTERVAL_MS = 3000
// A foreground page also wakes pending native lock leases for hidden target tabs.
// Keep this interval within the MCP lock ACK deadline; hidden content scripts pause.
const IDLE_POLL_INTERVAL_MS = 3000

export default defineContentScript({
  matches: ['<all_urls>'],
  runAt: 'document_idle',
  allFrames: false,
  async main(ctx) {
    let currentContext: InPageControlContext | null = null
    let pollRevision = 0
    let invalidated = false
    let stopPendingAt = 0
    let stopPendingIds = new Set<string>()
    let stopFailed = false
    let pollTimer: ReturnType<typeof setTimeout> | undefined

    const helpRequest = () =>
      currentContext?.mode === 'help'
        ? currentContext
        : currentContext?.helpRequest

    const ui = await createShadowRootUi<TakeoverBar>(ctx, {
      name: 'browseros-neo-takeover',
      position: 'overlay',
      alignment: 'bottom-left',
      zIndex: 2147483000,
      anchor: 'body',
      css: TAKEOVER_BAR_CSS,
      onMount(container) {
        return createTakeoverBar(container, {
          iconUrl: chrome.runtime.getURL('icon/128.png'),
          async onHandBack(note) {
            const help = helpRequest()
            if (!help) return false
            const result = (await chrome.runtime.sendMessage({
              type: 'takeover-resolve',
              sessionId: help.sessionId,
              requestId: help.requestId,
              note: note || undefined,
            })) as { ok?: boolean } | undefined
            return result?.ok === true
          },
          onStop: async () => {
            stopPendingIds = new Set(
              currentContext?.mode === 'agent-control'
                ? currentContext.sessionIds
                : [],
            )
            stopPendingAt = Date.now()
            stopFailed = false
            pollRevision += 1
            if (currentContext?.mode === 'agent-control') {
              currentContext = { ...currentContext, stopping: true }
              ui.mounted?.update(currentContext)
            }
            const result = (await chrome.runtime.sendMessage({
              type: 'takeover-stop',
            })) as { ok?: boolean; stopped?: number; total?: number } | undefined
            const stopped = Number(result?.stopped) || 0
            const total = Number(result?.total) || 0
            if (stopped === 0) {
              stopPendingIds.clear()
              stopFailed = true
            }
            if (stopped > 0) void poll()
            return { stopped, total }
          },
        })
      },
      onRemove(mounted) {
        mounted?.destroy()
      },
    })
    ui.mount()

    const scheduleNextPoll = (delay: number) => {
      if (pollTimer !== undefined) clearTimeout(pollTimer)
      pollTimer = undefined
      if (invalidated || document.visibilityState !== 'visible') return
      pollTimer = setTimeout(() => {
        pollTimer = undefined
        void poll()
      }, delay)
    }

    const poll = async () => {
      // Backgrounded tabs do not need the bar; the human focuses the blocked
      // tab to act, which triggers an immediate poll via visibilitychange.
      if (pollTimer !== undefined) clearTimeout(pollTimer)
      pollTimer = undefined
      const revision = ++pollRevision
      if (invalidated || document.visibilityState !== 'visible') return
      let response: TakeoverPollResponse | undefined
      try {
        response = (await chrome.runtime.sendMessage({
          type: 'takeover-poll',
        })) as TakeoverPollResponse | undefined
      } catch {
        response = undefined
      }
      if (
        revision !== pollRevision ||
        invalidated ||
        document.visibilityState !== 'visible'
      ) {
        return
      }
      let nextContext: InPageControlContext | null = null
      if (!response || ('unavailable' in response && response.unavailable)) {
        // Keep the last known session visible when polling fails. Hiding it here
        // would falsely suggest control ended when the extension is disconnected.
        if (currentContext) {
          nextContext = { ...currentContext, connectionLost: true }
          if (nextContext.mode === 'agent-control') {
            nextContext = {
              ...nextContext,
              nativeInputLock: response?.nativeInputLock ?? 'unavailable',
            }
          }
          if (
            nextContext.mode === 'agent-control' &&
            stopPendingIds.size > 0
          ) {
            stopFailed = true
            nextContext = { ...nextContext, stopFailed: true }
          }
        }
      } else {
        nextContext = response.active ? response.context : null
        if (stopPendingIds.size > 0) {
          if (nextContext?.mode === 'agent-control') {
            const remaining = nextContext.sessionIds.filter((id) =>
              stopPendingIds.has(id),
            )
            if (remaining.length === 0) {
              stopPendingIds.clear()
              stopFailed = false
            } else if (Date.now() - stopPendingAt < 12_000) {
              nextContext = { ...nextContext, stopping: true }
            } else {
              stopPendingIds.clear()
              stopFailed = true
              nextContext = { ...nextContext, stopFailed: true }
            }
          } else {
            stopPendingIds.clear()
            stopFailed = false
          }
        } else if (stopFailed && nextContext?.mode === 'agent-control') {
          nextContext = { ...nextContext, stopFailed: true }
        }
      }
      currentContext = nextContext
      ui.mounted?.update(currentContext)
      scheduleNextPoll(
        currentContext ? ACTIVE_POLL_INTERVAL_MS : IDLE_POLL_INTERVAL_MS,
      )
    }

    ctx.addEventListener(document, 'visibilitychange', () => {
      if (document.visibilityState === 'visible') {
        void poll()
      } else if (pollTimer !== undefined) {
        clearTimeout(pollTimer)
        pollTimer = undefined
      }
    })
    ctx.onInvalidated(() => {
      invalidated = true
      pollRevision += 1
      if (pollTimer !== undefined) clearTimeout(pollTimer)
      pollTimer = undefined
      ui.remove()
    })
    void poll()
  },
})
