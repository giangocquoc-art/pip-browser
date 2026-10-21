import { describe, expect, it } from 'bun:test'
import { TakeoverInputLockGate } from './takeover-input-lock-gate'

describe('TakeoverInputLockGate', () => {
  it('rejects a poll captured before an operator stop', () => {
    const gate = new TakeoverInputLockGate()
    const stalePoll = gate.beginPoll(7)

    gate.beginStop(7)
    expect(gate.mayAcquire(7, stalePoll)).toBe(false)
    gate.finishStop(7)
    expect(gate.mayAcquire(7, stalePoll)).toBe(false)
  })

  it('rejects polls while stopping and permits a fresh poll afterward', () => {
    const gate = new TakeoverInputLockGate()
    gate.beginStop(7)
    const stoppingPoll = gate.beginPoll(7)
    expect(gate.mayAcquire(7, stoppingPoll)).toBe(false)

    gate.finishStop(7)
    const freshPoll = gate.beginPoll(7)
    expect(gate.mayAcquire(7, freshPoll)).toBe(true)
  })

  it('keeps a tab gated until every overlapping stop-like operation finishes', () => {
    const gate = new TakeoverInputLockGate()
    const stalePoll = gate.beginPoll(7)

    gate.beginStop(7)
    gate.beginStop(7)
    gate.finishStop(7)

    expect(gate.isOnlyStop(7)).toBe(true)
    expect(gate.mayAcquire(7, stalePoll)).toBe(false)
    const stillGatedPoll = gate.beginPoll(7)
    expect(gate.mayAcquire(7, stillGatedPoll)).toBe(false)

    gate.finishStop(7)
    const freshPoll = gate.beginPoll(7)
    expect(gate.mayAcquire(7, freshPoll)).toBe(true)
  })

  it('keeps stops and polls independent across tabs', () => {
    const gate = new TakeoverInputLockGate()
    const firstTabPoll = gate.beginPoll(7)
    const secondTabPoll = gate.beginPoll(8)
    gate.beginStop(7)

    expect(gate.mayAcquire(7, firstTabPoll)).toBe(false)
    expect(gate.mayAcquire(8, secondTabPoll)).toBe(true)
  })

  it('clears state when its tab closes', () => {
    const gate = new TakeoverInputLockGate()
    const poll = gate.beginPoll(7)
    gate.beginStop(7)
    gate.removeTab(7)

    // A closed tab's response must be ignored, even if Chromium later reuses
    // the numeric tab id for another page.
    expect(gate.mayAcquire(7, poll)).toBe(false)

    const reusedTabPoll = gate.beginPoll(7)
    expect(gate.mayAcquire(7, reusedTabPoll)).toBe(true)
    expect(gate.mayAcquire(7, poll)).toBe(false)
  })
})
