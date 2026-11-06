import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import { parseHTML } from 'linkedom'
import type { InPageControlContext, TakeoverContext } from './takeover.types'
import {
  createTakeoverBar,
  TAKEOVER_BAR_CSS,
  type TakeoverBar,
} from './takeover-bar'

let window: ReturnType<typeof parseHTML>['window']
let root: HTMLElement
let bar: TakeoverBar | null = null

function context(over: Partial<TakeoverContext> = {}): TakeoverContext {
  return {
    mode: 'help',
    sessionId: 'session-1',
    requestId: 'help-1',
    agentLabel: 'CLAUDE-CODE',
    reason: 'Enter the login so the run can continue',
    resumeHint: 'play Castlevania',
    site: 'netflix.com',
    requestedAt: Date.now() - 134_000,
    ...over,
  }
}

function click(selector: string) {
  const node = root.querySelector(selector)
  if (!node) throw new Error(`missing ${selector}`)
  node.dispatchEvent(new window.Event('click', { bubbles: true }))
}

beforeEach(() => {
  const dom = parseHTML(
    '<!doctype html><html><body><div id="root"></div></body></html>',
  )
  window = dom.window
  root = dom.document.getElementById('root') as unknown as HTMLElement
  bar = null
})

afterEach(() => {
  bar?.destroy()
})

describe('takeover bar', () => {
  it('stays hidden until a context arrives', () => {
    bar = createTakeoverBar(root, { onHandBack: () => {} })
    expect((root.querySelector('.nt-root') as HTMLElement).hidden).toBe(true)
  })

  it('runs the waiting ticker only while a help request is visible', () => {
    const originalSetInterval = globalThis.setInterval
    const originalClearInterval = globalThis.clearInterval
    let started = 0
    let cleared = 0
    globalThis.setInterval = (() => {
      started += 1
      return started as unknown as ReturnType<typeof setInterval>
    }) as unknown as typeof setInterval
    globalThis.clearInterval = (() => {
      cleared += 1
    }) as typeof clearInterval
    try {
      bar = createTakeoverBar(root, { onHandBack: () => {} })
      expect(started).toBe(0)
      bar.update({
        mode: 'agent-control',
        sessionIds: ['session-1'],
        agentLabels: ['Codex'],
        sites: [],
        since: Date.now(),
      })
      expect(started).toBe(0)
      bar.update(context())
      expect(started).toBe(1)
      bar.update(null)
      expect(cleared).toBe(1)
    } finally {
      bar?.destroy()
      bar = null
      globalThis.setInterval = originalSetInterval
      globalThis.clearInterval = originalClearInterval
    }
  })

  it('leads with Pip Dev branding and shows the request', () => {
    bar = createTakeoverBar(root, {
      onHandBack: () => {},
      onStop: async () => ({ stopped: 0, total: 0 }),
      iconUrl: 'data:image/png;base64,AA==',
    })
    bar.update(context())

    expect((root.querySelector('.nt-root') as HTMLElement).hidden).toBe(false)
    expect(root.querySelector('.nt-brand')?.textContent).toBe('Pip Dev')
    expect(root.querySelector('.nt-dog img')).not.toBeNull()
    expect(root.querySelector('.nt-agent')?.textContent).toBe('CLAUDE-CODE')
    expect(root.querySelector('.nt-site')?.textContent).toContain('netflix.com')
    expect(root.querySelector('.nt-reason')?.textContent).toContain(
      'Enter the login',
    )
    expect(root.querySelector('.nt-resume')?.textContent).toContain(
      'play Castlevania',
    )
    expect(root.querySelector('.nt-timer')?.textContent).toBe('2:14')
  })

  it('hands back with the typed note', () => {
    let handed: string | undefined
    bar = createTakeoverBar(root, {
      onHandBack: (note) => {
        handed = note
      },
    })
    bar.update(context())
    const note = root.querySelector('.nt-note') as HTMLInputElement
    note.value = '  signed in  '
    click('.nt-handback')
    expect(handed).toBe('signed in')
  })

  it('minimizes to the pill and expands again', () => {
    bar = createTakeoverBar(root, { onHandBack: () => {} })
    bar.update(context())

    click('.nt-min')
    expect((root.querySelector('.nt-bar') as HTMLElement).hidden).toBe(true)
    expect((root.querySelector('.nt-pill-btn') as HTMLElement).hidden).toBe(
      false,
    )

    click('.nt-pill-btn')
    expect((root.querySelector('.nt-bar') as HTMLElement).hidden).toBe(false)
    expect((root.querySelector('.nt-pill-btn') as HTMLElement).hidden).toBe(
      true,
    )
  })

  it('forces [hidden] so the reset display rules cannot show both states', () => {
    // all:initial plus explicit display rules outrank the UA [hidden] default,
    // so the shadow CSS must force it or minimize leaves both states visible.
    expect(TAKEOVER_BAR_CSS).toContain('[hidden] { display: none !important; }')
  })

  it('hides again when the request clears', () => {
    bar = createTakeoverBar(root, { onHandBack: () => {} })
    bar.update(context())
    bar.update(null)
    expect((root.querySelector('.nt-root') as HTMLElement).hidden).toBe(true)
  })

  it('shows a cooperative stop control for a tab in an agent session', async () => {
    let stopCalls = 0
    bar = createTakeoverBar(root, {
      onHandBack: () => {},
      onStop: async () => {
        stopCalls += 1
        return { stopped: 1, total: 1 }
      },
    })
    bar.update({
      mode: 'agent-control',
      sessionIds: ['session-1'],
      agentLabels: ['Codex'],
      sites: ['example.com'],
      since: Date.now(),
    })

    expect(root.querySelector('.nt-reason')?.textContent).toContain(
      'có thể thao tác',
    )
    expect(root.querySelector('.nt-control-note')?.textContent).toContain(
      'Chưa khóa được chuột/bàn phím thật',
    )
    expect((root.querySelector('.nt-stop') as HTMLButtonElement).hidden).toBe(
      false,
    )
    expect(root.querySelector('.nt-warning')?.textContent).toContain(
      'hủy 1 phiên agent',
    )
    click('.nt-stop')
    await Promise.resolve()
    expect(stopCalls).toBe(1)
    expect(root.querySelector('.nt-status')?.textContent).toContain(
      'Đã gửi yêu cầu hủy phiên',
    )
  })

  it('shows help and stop actions together when both apply to one tab', () => {
    bar = createTakeoverBar(root, {
      onHandBack: () => {},
      onStop: async () => ({ stopped: 0, total: 0 }),
    })
    const combined: InPageControlContext = {
      mode: 'agent-control',
      sessionIds: ['session-1', 'session-2'],
      agentLabels: ['Codex', 'Claude'],
      sites: ['example.com'],
      since: Date.now(),
      helpRequest: context({ sessionId: 'session-2', reason: 'Please sign in' }),
    }
    bar.update(combined)

    expect(root.querySelector('.nt-title')?.textContent).toContain(
      'AI đang chờ bạn xử lý',
    )
    expect(root.querySelector('.nt-reason')?.textContent).toBe('Please sign in')
    expect(root.querySelector('.nt-control-note')?.textContent).toContain(
      'tab đã mở khóa',
    )
    expect((root.querySelector('.nt-note') as HTMLInputElement).hidden).toBe(
      false,
    )
    expect((root.querySelector('.nt-handback') as HTMLButtonElement).hidden).toBe(
      false,
    )
    expect((root.querySelector('.nt-stop') as HTMLButtonElement).textContent).toBe(
      'Dừng 2 phiên agent',
    )
    expect(root.querySelector('.nt-warning')?.textContent).toContain(
      'hủy 2 phiên agent',
    )
  })

  it('explains when another agent still locks the tab during a help request', () => {
    bar = createTakeoverBar(root, { onHandBack: () => {} })
    bar.update({
      mode: 'agent-control',
      sessionIds: ['session-active', 'session-help'],
      agentLabels: ['Codex', 'Claude'],
      sites: ['example.com'],
      since: Date.now(),
      helpRequest: context({ sessionId: 'session-help', reason: 'Please sign in' }),
      nativeInputLock: 'locked',
    })

    expect(root.querySelector('.nt-control-note')?.textContent).toContain(
      'Phiên agent khác vẫn đang điều khiển',
    )
  })

  it('shows a retry status when the server cannot confirm stop', () => {
    bar = createTakeoverBar(root, {
      onHandBack: () => {},
      onStop: async () => ({ stopped: 0, total: 1 }),
    })
    bar.update({
      mode: 'agent-control',
      sessionIds: ['session-1'],
      agentLabels: ['Codex'],
      sites: [],
      since: Date.now(),
      stopFailed: true,
    })

    expect(root.querySelector('.nt-status')?.textContent).toContain(
      'Chưa xác nhận được phiên đã dừng',
    )
    expect((root.querySelector('.nt-stop') as HTMLButtonElement).disabled).toBe(
      false,
    )
  })

  it('keeps the connection warning visible while an agent session is offline', () => {
    bar = createTakeoverBar(root, {
      onHandBack: () => {},
      onStop: async () => ({ stopped: 0, total: 0 }),
    })
    bar.update({
      mode: 'agent-control',
      sessionIds: ['session-1'],
      agentLabels: ['Codex'],
      sites: ['example.com'],
      since: Date.now(),
      connectionLost: true,
    })

    expect((root.querySelector('.nt-root') as HTMLElement).hidden).toBe(false)
    expect(root.querySelector('.nt-status')?.textContent).toContain(
      'Mất kết nối với Pip Dev',
    )
  })
})
