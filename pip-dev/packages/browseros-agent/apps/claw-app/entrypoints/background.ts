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
  releaseAndGetNativeInputLockStatus,
  requireAcknowledgedNativeInputLock,
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
  // Revisions close races among observed stop/help transitions. The server
  // cannot atomically reserve a tab with this CDP update, so a new owner created
  // after the fresh snapshot may exist briefly before the next poll locks it.
  // Detach also drops Chromium's lease before a raced stop can be observed, so
  // that narrow handoff re-locks immediately but is not an atomic browser/server
  // transaction. Eliminating both arrival windows needs server-side reservation.
  const reconcileInputLockIfFinalStop = async (tabId: number) => {
    while (inputLockGate.isOnlyStop(tabId)) {
      const revision = inputLockGate.revision(tabId)
      if (revision === undefined) return
      const stillFinalStop = () =>
        inputLockGate.isOnlyStop(tabId) &&
        inputLockGate.revision(tabId) === revision

      try {
        const response = await takeover.refreshForTab(tabId)
        // A new stop or help hand-back may have started while the server
        // snapshot was loading. Re-read after it finishes if this operation is
        // now the final stop; otherwise that stop will reconcile on completion.
        if (!stillFinalStop()) {
          if (inputLockGate.isOnlyStop(tabId)) continue
          return
        }
        if (response.active && response.context.mode === 'agent-control') {
          await nativeInputLocks.sync(
            tabId,
            nativeInputLockSessionIds(response.context),
          )
        } else {
          await nativeInputLocks.release(tabId, stillFinalStop)
        }
      } catch {
        if (stillFinalStop()) {
          try {
            await nativeInputLocks.release(tabId, stillFinalStop)
          } catch {
            // The native manager retains an unavailable lease if it cannot
            // restore the lock after a stale CDP release. The next poll reports
            // that state; do not claim a successful unlock here.
          }
        }
      }

      // A stop may begin while sync/release is awaiting CDP. Reconcile again
      // from a fresh snapshot when this caller is still the final stop, so an
      // earlier stop cannot leave a stale lease after an overlapping stop.
      if (stillFinalStop()) return
      if (!inputLockGate.isOnlyStop(tabId)) return
    }
  }
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
        reconcileLock: reconcileInputLockIfFinalStop,
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
          const nativeInputLock = await releaseAndGetNativeInputLockStatus(
            nativeInputLocks,
            tabId,
          )
          if (
            !response.active &&
            'unavailable' in response &&
            response.unavailable
          ) {
            sendResponse({ ...response, nativeInputLock })
            return
          }
          if (nativeInputLock !== 'unlocked') {
            sendResponse({ ...response, nativeInputLock })
            return
          }
          sendResponse(response)
        })
        .catch(async () => {
          const nativeInputLock = await releaseAndGetNativeInputLockStatus(
            nativeInputLocks,
            tabId,
          )
          sendResponse({
            active: false,
            unavailable: true,
            nativeInputLock,
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
      void takeover
        .resolve(
          tabId,
          sessionId,
          note,
          async (context) => {
            // A help-only session may not appear in agentControlForTab; the
            // validated request still pins its exact session to this sender tab.
            const owners = context?.sessionIds ?? [sessionId]
            await requireAcknowledgedNativeInputLock(nativeInputLocks, tabId, owners)
          },
          () => reconcileInputLockIfFinalStop(tabId),
        )
        .then(async (resolved) => {
          await reconcileInputLockIfFinalStop(tabId)
          sendResponse({ ok: resolved })
        })
        .catch(async () => {
          await reconcileInputLockIfFinalStop(tabId)
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
          await reconcileInputLockIfFinalStop(tabId)
          inputLockGate.finishStop(tabId)
          sendResponse({ ok: total > 0 && stopped === total, stopped, total })
        })
        .catch(async () => {
          await reconcileInputLockIfFinalStop(tabId)
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
