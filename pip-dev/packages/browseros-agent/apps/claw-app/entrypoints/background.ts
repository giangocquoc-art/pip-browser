import { registerDiagnostics } from '@browseros/diagnostics/extension'
/**
 * @license
 * Copyright 2025 BrowserOS
 * SPDX-License-Identifier: AGPL-3.0-or-later
 */

import { defineBackground } from 'wxt/utils/define-background'
import { resolveBrowserOSServerBaseUrl } from '@/modules/api/browseros-ports'
import { createRecordingsRelay } from '@/modules/recorder'
import type {
  TakeoverResolveMessage,
} from '@/modules/takeover/takeover.types'
import { createTakeoverBridge } from '@/modules/takeover/takeover-bridge'
import {
  NativeInputLockManager,
  nativeInputLockSessionIds,
} from '@/modules/takeover/native-input-lock'
import { TakeoverInputLockGate } from '@/modules/takeover/takeover-input-lock-gate'
import { handleTakeoverAction } from '@/modules/takeover/takeover-action'
import { registerExtensionUpdates } from '@/modules/updates/register'

/** Supplies Chrome's trusted tab/document identity to the durable recorder relay. */
export default defineBackground(() => {
  registerExtensionUpdates()
  registerDiagnostics('browseros-neo', resolveBrowserOSServerBaseUrl)
  const relay = createRecordingsRelay({
    resolveServerBaseUrl: resolveBrowserOSServerBaseUrl,
  })
  const requestResnapshot = (tabId: number) => {
    // Tabs can disappear between recovery detection and message delivery.
    try {
      void chrome.tabs
        .sendMessage(tabId, { type: 'recorder-resnapshot' })
        .catch(() => {})
    } catch {}
  }
  const requestStop = (tabId: number) => {
    // The tab's session has ended; stop recording it through the cleanup grace.
    try {
      void chrome.tabs
        .sendMessage(tabId, { type: 'recorder-stop' })
        .catch(() => {})
    } catch {}
  }

  relay.onTabRecoveredAfterLoss(requestResnapshot)
  relay.onTabRecordingRetired(requestStop)
  void relay.start().catch((error) => {
    console.warn('[browseros-claw replay] durable outbox startup failed', error)
  })

  // Answers per-tab agent/help status and relays human-help, stop requests.
  const takeover = createTakeoverBridge({
    resolveServerBaseUrl: resolveBrowserOSServerBaseUrl,
  })
  const nativeInputLocks = new NativeInputLockManager(chrome.debugger)
  const inputLockGate = new TakeoverInputLockGate()
  chrome.tabs.onRemoved.addListener((tabId) => {
    inputLockGate.removeTab(tabId)
    void nativeInputLocks.release(tabId)
  })
  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    const messageType = (message as { type?: unknown } | null)?.type
    if (
      messageType === 'takeover-action-poll' ||
      messageType === 'takeover-action-stop'
    ) {
      void handleTakeoverAction(message, sender, {
        extensionId: chrome.runtime.id,
        popupUrl: chrome.runtime.getURL('popup.html'),
        getActiveTab: async (windowId) => {
          const [activeTab] = await chrome.tabs.query({ active: true, windowId })
          return activeTab
        },
        pollForTab: async (tabId) => {
          const response = await takeover.pollForTab(tabId)
          if (response.active && response.context.mode === 'agent-control') {
            return {
              ...response,
              context: {
                ...response.context,
                nativeInputLock: nativeInputLocks.status(tabId),
              },
            }
          }
          return response
        },
        stopForTab: (tabId) => takeover.stopForTab(tabId),
        beginStop: (tabId) => inputLockGate.beginStop(tabId),
        finishStop: (tabId) => inputLockGate.finishStop(tabId),
        releaseLock: (tabId) => nativeInputLocks.release(tabId),
      })
        .then((response) => {
          sendResponse(response ?? { ok: false, reason: 'invalid-request' })
        })
        .catch(() => {
          sendResponse({ ok: false, reason: 'unavailable' })
        })
      return true
    }
    const takeoverMessage = message as { type?: unknown }
    if (takeoverMessage.type === 'takeover-poll') {
      const tabId = sender.tab?.id
      if (typeof tabId !== 'number') {
        sendResponse({ active: false })
        return false
      }
      const pollRevision = inputLockGate.beginPoll(tabId)
      void takeover
        .pollForTab(tabId)
        .then(async (response) => {
          if (response.active && response.context.mode === 'agent-control') {
            if (!inputLockGate.mayAcquire(tabId, pollRevision)) {
              sendResponse({
                ...response,
                context: {
                  ...response.context,
                  nativeInputLock: nativeInputLocks.status(tabId),
                },
              })
              return
            }
            const nativeInputLock = await nativeInputLocks.sync(
              tabId,
              nativeInputLockSessionIds(response.context),
            )
            sendResponse({
              ...response,
              context: { ...response.context, nativeInputLock },
            })
            return
          }
          await nativeInputLocks.release(tabId)
          if (
            !response.active &&
            'unavailable' in response &&
            response.unavailable
          ) {
            sendResponse({ ...response, nativeInputLock: 'unlocked' })
            return
          }
          sendResponse(response)
        })
        .catch(async () => {
          await nativeInputLocks.release(tabId)
          sendResponse({
            active: false,
            unavailable: true,
            nativeInputLock: 'unlocked',
          })
        })
      return true
    }
    if (takeoverMessage.type === 'takeover-resolve') {
      const { sessionId, note } = message as TakeoverResolveMessage
      const tabId = sender.tab?.id
      if (typeof tabId !== 'number') {
        sendResponse({ ok: false })
        return false
      }
      inputLockGate.beginStop(tabId)
      const refreshInputLock = async () => {
        try {
          const response = await takeover.refreshForTab(tabId)
          if (response.active && response.context.mode === 'agent-control') {
            await nativeInputLocks.sync(
              tabId,
              nativeInputLockSessionIds(response.context),
            )
          } else {
            await nativeInputLocks.release(tabId)
          }
        } catch {
          await nativeInputLocks.release(tabId)
        }
      }
      void takeover
        .resolve(tabId, sessionId, note, async (context) => {
          if (context) {
            // Restore the lock before waking the help-requesting agent.
            await nativeInputLocks.sync(tabId, context.sessionIds)
          }
        })
        .then(async (resolved) => {
          if (!resolved) await refreshInputLock()
          sendResponse({ ok: resolved })
        })
        .catch(async () => {
          await refreshInputLock()
          sendResponse({ ok: false })
        })
        .finally(() => inputLockGate.finishStop(tabId))
      return true
    }
    if (takeoverMessage.type === 'takeover-stop') {
      const tabId = sender.tab?.id
      if (typeof tabId !== 'number') {
        sendResponse({ ok: false, stopped: 0, total: 0 })
        return false
      }
      inputLockGate.beginStop(tabId)
      void takeover
        .stopForTab(tabId)
        .then(async ({ stopped, total }) => {
          if (total === 0 || stopped === total) {
            await nativeInputLocks.release(tabId)
          }
          inputLockGate.finishStop(tabId)
          sendResponse({ ok: total > 0 && stopped === total, stopped, total })
        })
        .catch(() => {
          inputLockGate.finishStop(tabId)
          sendResponse({ ok: false, stopped: 0, total: 0 })
        })
      return true
    }
    return false
  })

  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    const recorderMessage = message as {
      type?: unknown
      ndjson?: unknown
      hasGap?: unknown
    }
    const tabId = sender.tab?.id
    const documentId = sender.documentId
    if (
      recorderMessage.type !== 'recorder-events' ||
      typeof recorderMessage.ndjson !== 'string' ||
      typeof recorderMessage.hasGap !== 'boolean' ||
      typeof tabId !== 'number' ||
      typeof documentId !== 'string'
    ) {
      return false
    }
    void relay
      .post(tabId, documentId, recorderMessage.ndjson, recorderMessage.hasGap)
      .then(() => sendResponse({ persisted: true }))
      .catch(() => sendResponse({ persisted: false }))
    return true
  })
})
