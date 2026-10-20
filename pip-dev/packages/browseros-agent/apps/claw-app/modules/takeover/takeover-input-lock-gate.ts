/**
 * @license
 * Copyright 2025 BrowserOS
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * Prevents a poll that began before an operator stop from reacquiring a tab's
 * input lock after the stop path has released it.
 */

export class TakeoverInputLockGate {
  private readonly revisions = new Map<number, number>()
  private readonly stoppingTabs = new Set<number>()
  private nextRevision = 0

  beginPoll(tabId: number): number {
    if (!this.revisions.has(tabId)) this.bump(tabId)
    return this.revisions.get(tabId)!
  }

  mayAcquire(tabId: number, pollRevision: number): boolean {
    return (
      this.revisions.has(tabId) &&
      !this.stoppingTabs.has(tabId) &&
      pollRevision === this.revisions.get(tabId)
    )
  }

  beginStop(tabId: number): void {
    this.stoppingTabs.add(tabId)
    this.bump(tabId)
  }

  finishStop(tabId: number): void {
    this.stoppingTabs.delete(tabId)
    if (this.revisions.has(tabId)) this.bump(tabId)
  }

  removeTab(tabId: number): void {
    this.stoppingTabs.delete(tabId)
    this.revisions.delete(tabId)
  }

  private bump(tabId: number): void {
    this.nextRevision += 1
    this.revisions.set(tabId, this.nextRevision)
  }
}
