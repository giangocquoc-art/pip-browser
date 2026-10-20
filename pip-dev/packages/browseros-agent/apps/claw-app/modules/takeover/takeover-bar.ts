/**
 * @license
 * Copyright 2025 BrowserOS
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * The in-page takeover bar: a shadow-DOM control the human uses to hand control
 * back to a parked agent or stop sessions that have access to this tab. Built with plain DOM so
 * the content script stays small and keeps Pip Dev branding on third-party pages.
 */

import type { InPageControlContext } from './takeover.types'

export interface TakeoverBarHandlers {
  onHandBack: (note: string) => void | Promise<boolean>
  onStop?: () => Promise<{ stopped: number; total: number }>
  iconUrl?: string
}

export interface TakeoverBar {
  /** Show the bar for this context, or hide it when null. */
  update: (context: InPageControlContext | null) => void
  destroy: () => void
}

// Pip orange leads. The shadow root resets inherited styles, so fonts and
// colors are set explicitly.
export const TAKEOVER_BAR_CSS = `
:host { all: initial; }
/* The all:initial reset plus explicit display rules below outrank the UA
   [hidden] default, so force it or minimize leaves both states on screen. */
[hidden] { display: none !important; }
.nt-root { position: fixed; left: 16px; right: 16px; bottom: 16px; z-index: 2147483000;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif; color: #0b1b33; }
.nt-bar { background: #fff; border: 1px solid rgba(255,106,0,0.2); border-top: 3px solid #ff6a00;
  border-radius: 12px; box-shadow: 0 10px 30px rgba(2,22,51,0.30); overflow: hidden; max-width: 760px; margin: 0 auto; }
.nt-head { display: flex; align-items: center; gap: 10px; padding: 9px 12px; background: #fff1e6; border-bottom: 1px solid rgba(255,106,0,0.18); }
.nt-dog { width: 22px; height: 22px; flex: 0 0 auto; display: flex; align-items: center; justify-content: center;
  border-radius: 6px; background: #fff; color: #ff6a00; font-weight: 900; font-size: 12px; overflow: hidden; }
.nt-dog img { width: 22px; height: 22px; object-fit: cover; display: block; }
.nt-brand { font-weight: 800; font-size: 12.5px; letter-spacing: -0.01em; color: #ff6a00; }
.nt-spacer { flex: 1 1 auto; }
.nt-pill { display: inline-flex; align-items: center; gap: 6px; background: #ff6a00; color: #fff; white-space: nowrap;
  border-radius: 999px; padding: 3px 9px; font-size: 10.5px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase; }
.nt-pill .nt-timer { font-weight: 600; letter-spacing: 0; font-variant-numeric: tabular-nums; }
.nt-min { width: 26px; height: 26px; border: 1px solid rgba(255,106,0,0.18); border-radius: 7px; background: #fff;
  color: #6b7b85; cursor: pointer; font-size: 15px; line-height: 1; display: flex; align-items: center; justify-content: center; padding: 0; }
.nt-min:hover { background: #fff1e6; }
.nt-body { padding: 12px 14px 14px; }
.nt-title { font-weight: 700; font-size: 14px; }
.nt-title .nt-agent { color: #0b1b33; }
.nt-title .nt-site { color: #d95700; }
.nt-reason { font-size: 13px; color: #41525c; margin-top: 3px; }
.nt-control-note { font-size: 12px; color: #76502c; margin-top: 5px; }
.nt-resume { font-size: 12.5px; color: #6b7b85; margin-top: 2px; }
.nt-actions { display: flex; gap: 10px; align-items: center; margin-top: 12px; flex-wrap: wrap; }
.nt-note { flex: 1 1 200px; min-width: 150px; height: 36px; border: 1px solid rgba(255,106,0,0.2); border-radius: 8px;
  padding: 0 10px; font: inherit; font-size: 13px; color: #0b1b33; background: #fff; }
.nt-note::placeholder { color: #6b7b85; }
.nt-handback { height: 36px; padding: 0 16px; border: 0; border-radius: 8px; background: #ff6a00; color: #fff;
  font: inherit; font-weight: 600; font-size: 13px; cursor: pointer; }
.nt-handback:hover { background: #e65f00; }
.nt-handback:disabled { opacity: 0.6; cursor: default; }
.nt-stop { height: 36px; padding: 0 16px; border: 0; border-radius: 8px; background: #d83b27; color: #fff;
  font: inherit; font-weight: 700; font-size: 13px; cursor: pointer; }
.nt-stop:hover { background: #b92b1a; }
.nt-stop:disabled { opacity: 0.6; cursor: default; }
.nt-warning { font-size: 12px; color: #76502c; margin-top: 7px; }
.nt-status { font-size: 12px; color: #a12a1b; margin-top: 7px; }
.nt-pill-btn { display: inline-flex; align-items: center; gap: 9px; background: #ff6a00; color: #fff; border: 0;
  border-radius: 999px; padding: 7px 13px 7px 7px; box-shadow: 0 8px 22px rgba(2,22,51,0.4); font: inherit;
  font-size: 13px; font-weight: 600; cursor: pointer; float: right; }
.nt-chip { width: 24px; height: 24px; border-radius: 7px; background: #fff; display: flex; align-items: center; justify-content: center; }
.nt-chip img { width: 18px; height: 18px; border-radius: 4px; display: block; }
.nt-dot { width: 7px; height: 7px; border-radius: 50%; background: #f6b53d; box-shadow: 0 0 0 2px rgba(246,181,61,0.3); }
.nt-pill-btn .nt-timer { font-variant-numeric: tabular-nums; opacity: 0.92; }
.nt-chev { opacity: 0.9; }
`

function waitLabel(requestedAt: number, now: number): string {
  const seconds = Math.max(0, Math.floor((now - requestedAt) / 1000))
  const minutes = Math.floor(seconds / 60)
  return `${minutes}:${String(seconds % 60).padStart(2, '0')}`
}

export function createTakeoverBar(
  root: HTMLElement,
  handlers: TakeoverBarHandlers,
): TakeoverBar {
  const doc = root.ownerDocument
  const el = <K extends keyof HTMLElementTagNameMap>(
    tag: K,
    className?: string,
  ): HTMLElementTagNameMap[K] => {
    const node = doc.createElement(tag)
    if (className) node.className = className
    return node
  }

  const wrap = el('div', 'nt-root')
  wrap.hidden = true

  // Expanded bar.
  const bar = el('div', 'nt-bar')
  const head = el('div', 'nt-head')
  const dog = el('span', 'nt-dog')
  if (handlers.iconUrl) {
    const mark = el('img')
    mark.src = handlers.iconUrl
    mark.alt = ''
    dog.append(mark)
  } else {
    dog.textContent = 'Pip'
  }
  const brand = el('span', 'nt-brand')
  brand.textContent = 'Pip Dev'
  const spacer = el('span', 'nt-spacer')
  const pill = el('span', 'nt-pill')
  const pillLabel = el('span')
  pillLabel.textContent = 'AI đang chờ bạn'
  const timer = el('span', 'nt-timer')
  pill.append(pillLabel, ' ', timer)
  const minimize = el('button', 'nt-min')
  minimize.type = 'button'
  minimize.title = 'Thu nhỏ'
  minimize.setAttribute('aria-label', 'Thu nhỏ thanh điều khiển Pip Dev')
  minimize.textContent = '−'
  head.append(dog, brand, spacer, pill, minimize)

  const body = el('div', 'nt-body')
  const title = el('div', 'nt-title')
  title.append('AI đang chờ bạn xử lý: ')
  const agent = el('b', 'nt-agent')
  const site = el('span', 'nt-site')
  title.append(agent, ' ', site)
  const reason = el('div', 'nt-reason')
  const controlNote = el('div', 'nt-control-note')
  const resume = el('div', 'nt-resume')
  const actions = el('div', 'nt-actions')
  const note = el('input', 'nt-note')
  note.type = 'text'
  note.placeholder = 'Ghi chú cho AI (không bắt buộc)'
  note.setAttribute('aria-label', 'Ghi chú cho AI')
  const handback = el('button', 'nt-handback')
  handback.type = 'button'
  handback.textContent = 'Trả quyền cho AI'
  const stop = el('button', 'nt-stop')
  stop.type = 'button'
  stop.textContent = 'Dừng phiên agent'
  stop.hidden = true
  const warning = el('div', 'nt-warning')
  warning.hidden = true
  const status = el('div', 'nt-status')
  status.hidden = true
  actions.append(note, handback, stop)
  body.append(title, reason, resume, controlNote, warning, status, actions)
  bar.append(head, body)

  // Minimized pill.
  const pillBtn = el('button', 'nt-pill-btn')
  pillBtn.type = 'button'
  pillBtn.setAttribute('aria-label', 'Mở điều khiển Pip Dev')
  const chip = el('span', 'nt-chip')
  if (handlers.iconUrl) {
    const mark = el('img')
    mark.src = handlers.iconUrl
    mark.alt = ''
    chip.append(mark)
  } else {
    chip.textContent = 'P'
  }
  const pillAgent = el('span')
  const dot = el('span', 'nt-dot')
  const pillTimer = el('span', 'nt-timer')
  const chev = el('span', 'nt-chev')
  chev.textContent = '›'
  const pillLabelMin = el('span')
  pillLabelMin.textContent = 'AI chờ bạn'
  pillBtn.append(chip, pillAgent, ' ', pillLabelMin, ' ', dot, pillTimer, chev)
  pillBtn.hidden = true

  wrap.append(bar, pillBtn)
  root.append(wrap)

  let current: InPageControlContext | null = null
  let minimized = false
  let ticker: ReturnType<typeof setInterval> | undefined

  const requestKey = (context: InPageControlContext | null): string => {
    if (!context) return ''
    const help = context.mode === 'help' ? context : context.helpRequest
    const control =
      context.mode === 'agent-control' ? context.sessionIds.join(',') : ''
    return `${context.mode}:${control}:${help?.sessionId ?? ''}:${help?.requestedAt ?? ''}`
  }

  const renderTimer = () => {
    if (!current) return
    const help = current.mode === 'help' ? current : current.helpRequest
    if (!help) {
      timer.textContent = ''
      pillTimer.textContent = ''
      return
    }
    const label = waitLabel(help.requestedAt, Date.now())
    timer.textContent = label
    pillTimer.textContent = label
  }

  const syncTicker = () => {
    const help = current?.mode === 'help' ? current : current?.helpRequest
    if (help && ticker === undefined && typeof setInterval === 'function') {
      ticker = setInterval(renderTimer, 1000)
    } else if (!help && ticker !== undefined) {
      clearInterval(ticker)
      ticker = undefined
    }
  }

  const render = () => {
    if (!current) {
      wrap.hidden = true
      return
    }
    wrap.hidden = false
    const control = current.mode === 'agent-control' ? current : null
    const help = current.mode === 'help' ? current : current.helpRequest
    const hasControl = control !== null
    const label = control
      ? control.agentLabels.join(', ')
      : help!.agentLabel
    agent.textContent = label
    title.firstChild!.textContent = help
      ? 'AI đang chờ bạn xử lý: '
      : 'Phiên agent có quyền truy cập tab: '
    site.textContent = help
      ? help.site
        ? `trên ${help.site}`
        : ''
      : control?.sites.length
        ? `· ${control.sites.join(', ')}`
        : ''
    reason.textContent = help
      ? help.reason
      : 'Phiên agent có thể thao tác trong tab này.'
    controlNote.hidden = !hasControl
    controlNote.textContent =
      control?.nativeInputLock === 'locked'
        ? 'Tab này đã khóa chuột và bàn phím thật; agent vẫn thao tác qua CDP.'
        : control
          ? 'Chưa khóa được chuột/bàn phím thật. Hãy dừng agent trước khi thao tác trong tab này.'
          : ''
    resume.hidden = !help?.resumeHint
    resume.textContent =
      help?.resumeHint
        ? `Khi xong, AI sẽ tiếp tục tại: ${help.resumeHint}`
        : ''
    note.hidden = !help
    handback.hidden = !help
    stop.hidden = !hasControl || !handlers.onStop
    stop.textContent =
      control && control.sessionIds.length > 1
        ? `Dừng ${control.sessionIds.length} phiên agent`
        : 'Dừng, trả quyền cho tôi'
    stop.disabled = Boolean(control?.stopping)
    warning.hidden = !hasControl
    warning.textContent = control
      ? `Dừng sẽ hủy ${control.sessionIds.length} phiên agent và các tab thuộc những phiên đó. Tab này chỉ mở khóa sau khi Pip Dev xác nhận đã dừng.`
      : ''
    pillLabel.textContent = help
      ? 'AI đang chờ bạn'
      : 'Phiên agent có quyền'
    pillLabelMin.textContent = help ? 'AI chờ bạn' : 'Phiên agent'
    pillAgent.textContent = label
    renderTimer()
    if (current.connectionLost) {
      status.hidden = false
      status.textContent = 'Mất kết nối với Pip Dev; chưa xác nhận được trạng thái hoặc yêu cầu dừng.'
    } else if (control?.stopFailed) {
      status.hidden = false
      status.textContent = 'Chưa xác nhận được phiên đã dừng. Bạn có thể thử lại.'
      stop.disabled = false
    } else if (control?.stopping) {
      status.hidden = false
      status.textContent = 'Đang chờ Pip Dev xác nhận yêu cầu dừng.'
    } else {
      status.hidden = true
      status.textContent = ''
    }
    bar.hidden = minimized
    pillBtn.hidden = !minimized
  }

  minimize.addEventListener('click', () => {
    minimized = true
    render()
  })
  pillBtn.addEventListener('click', () => {
    minimized = false
    render()
  })
  handback.addEventListener('click', () => {
    handback.disabled = true
    status.hidden = true
    void Promise.resolve(handlers.onHandBack(note.value.trim()))
      .then((resolved) => {
        if (resolved === false) throw new Error('resolve failed')
      })
      .catch(() => {
        status.hidden = false
        status.textContent = 'Không kết nối được với AI. Hãy thử lại.'
        handback.disabled = false
      })
  })
  stop.addEventListener('click', () => {
    if (!handlers.onStop) return
    stop.disabled = true
    status.hidden = true
    status.textContent = ''
    void handlers.onStop().then(({ stopped, total }) => {
      if (stopped > 0) {
        status.hidden = false
        status.textContent =
          stopped === total
            ? 'Đã gửi yêu cầu hủy phiên. Đang chờ xác nhận.'
            : `Đã gửi yêu cầu hủy ${stopped}/${total} phiên agent.`
      } else {
        status.hidden = false
        status.textContent = 'Không dừng được phiên agent. Hãy thử lại.'
        stop.disabled = false
      }
    }).catch(() => {
      status.hidden = false
      status.textContent = 'Không kết nối được với Pip Dev. Hãy thử lại.'
      stop.disabled = false
    })
  })

  return {
    update(context) {
      const sameRequest = requestKey(context) === requestKey(current)
      current = context
      syncTicker()
      // A new request (or reappearance) resets the per-request UI state.
      if (!sameRequest) {
        minimized = false
        note.value = ''
        handback.disabled = false
        stop.disabled = false
        status.hidden = true
        status.textContent = ''
      }
      render()
    },
    destroy() {
      if (ticker !== undefined) clearInterval(ticker)
      ticker = undefined
      wrap.remove()
    },
  }
}
