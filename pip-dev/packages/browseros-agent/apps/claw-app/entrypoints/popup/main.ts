import './style.css'
import type { TakeoverActionResponse } from '@/modules/takeover/takeover-action'

const targetLabel = document.querySelector<HTMLElement>('#target')
const statusLabel = document.querySelector<HTMLElement>('#status')
const stopButton = document.querySelector<HTMLButtonElement>('#stop')

if (targetLabel && statusLabel && stopButton) {
  let target: { tabId: number; windowId: number } | null = null

  const message = (request: Record<string, unknown>) =>
    chrome.runtime.sendMessage(request) as Promise<TakeoverActionResponse>

  const showUnavailable = (detail: string) => {
    statusLabel.textContent = detail
    stopButton.disabled = false
  }

  const checkTarget = async () => {
    stopButton.disabled = true
    try {
      const [tab] = await chrome.tabs.query({
        active: true,
        lastFocusedWindow: true,
      })
      if (typeof tab?.id !== 'number' || typeof tab.windowId !== 'number') {
        targetLabel.textContent = 'Không tìm thấy tab đang mở.'
        statusLabel.textContent = 'Hãy mở popup trên tab cần dừng agent.'
        return
      }
      target = { tabId: tab.id, windowId: tab.windowId }
      targetLabel.textContent = `Tab: ${tab.title || tab.url || `#${tab.id}`}`

      const result = await message({
        type: 'takeover-action-poll',
        ...target,
      })
      if (!result.ok) {
        statusLabel.textContent =
          result.reason === 'inactive-tab'
            ? 'Tab đã đổi. Đóng popup rồi mở lại trên tab cần dừng.'
            : 'Không thể kiểm tra quyền điều khiển tab.'
        return
      }

      const poll = result.response
      if (poll.active && poll.context.mode === 'agent-control') {
        const names = poll.context.agentLabels.filter(Boolean).join(', ')
        statusLabel.textContent = names
          ? `Agent đang điều khiển tab này: ${names}`
          : 'Agent đang điều khiển tab này.'
        stopButton.disabled = false
        return
      }
      if (!poll.active && 'unavailable' in poll && poll.unavailable) {
        showUnavailable('Không thể kết nối để xác nhận phiên. Bạn vẫn có thể gửi lệnh dừng.')
        return
      }
      statusLabel.textContent = poll.active
        ? 'Tab này đang chờ trợ giúp, không có agent cần dừng.'
        : 'Không có agent đang điều khiển tab này.'
    } catch {
      showUnavailable('Không kiểm tra được phiên. Bạn vẫn có thể gửi lệnh dừng.')
    }
  }

  stopButton.addEventListener('click', async () => {
    if (!target) return
    stopButton.disabled = true
    statusLabel.textContent = 'Đang gửi lệnh dừng…'
    try {
      const result = await message({
        type: 'takeover-action-stop',
        ...target,
      })
      if (result.ok && result.action === 'stop') {
        statusLabel.textContent = result.total === 0
          ? 'Không còn phiên agent trên tab này; đã trả quyền thao tác.'
          : `Đã dừng ${result.stopped}/${result.total} phiên agent trên tab này.`
      } else if (result.reason === 'partial-stop') {
        statusLabel.textContent =
          `Đã dừng ${result.stopped ?? 0}/${result.total ?? 0} phiên. Tab vẫn khóa khi còn agent.`
        stopButton.disabled = false
      } else if (result.reason === 'inactive-tab') {
        statusLabel.textContent = 'Tab đã đổi. Mở lại popup trên tab cần dừng.'
      } else {
        statusLabel.textContent = 'Chưa xác nhận được lệnh dừng. Kiểm tra kết nối rồi thử lại.'
        stopButton.disabled = false
      }
    } catch {
      statusLabel.textContent = 'Không gửi được lệnh dừng. Hãy kiểm tra kết nối rồi thử lại.'
      stopButton.disabled = false
    }
  })

  void checkTarget()
}
