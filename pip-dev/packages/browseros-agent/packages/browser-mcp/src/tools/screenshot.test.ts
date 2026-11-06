import { describe, expect, it } from 'bun:test'
import type { ProtocolApi } from '@browseros/cdp-protocol/protocol-api'
import { buildScreenshotClip } from './screenshot'

function createSession(viewport = { x: 25, y: 180, width: 1280, height: 720 }) {
  const session = {
    Page: {
      getLayoutMetrics: async () => ({
        cssLayoutViewport: {
          pageX: viewport.x,
          pageY: viewport.y,
          clientWidth: viewport.width,
          clientHeight: viewport.height,
        },
      }),
    },
  }
  return session as unknown as ProtocolApi
}

describe('screenshot crop geometry', () => {
  it('maps a fresh viewport-relative crop to page coordinates and scales down', async () => {
    const viewport = { x: 25, y: 180, width: 1280, height: 720 }
    const result = await buildScreenshotClip(
      createSession(viewport),
      { width: 300 },
      { x: 100, y: 40, width: 600, height: 300 },
      viewport,
    )

    expect(result).toEqual({
      clip: { x: 125, y: 220, width: 600, height: 300, scale: 0.5 },
      viewport,
    })
  })

  it('refuses coordinates after scrolling or resizing', async () => {
    const observed = { x: 25, y: 180, width: 1280, height: 720 }
    const current = { ...observed, y: 300 }

    await expect(
      buildScreenshotClip(
        createSession(current),
        undefined,
        { x: 5, y: 5, width: 30, height: 30 },
        observed,
      ),
    ).rejects.toThrow(/viewport or scroll position changed/i)
  })

  it('refuses a crop outside the currently observed viewport', async () => {
    const viewport = { x: 25, y: 180, width: 1280, height: 720 }

    await expect(
      buildScreenshotClip(
        createSession(viewport),
        undefined,
        { x: 1200, y: 5, width: 100, height: 30 },
        viewport,
      ),
    ).rejects.toThrow(/outside the visible viewport/i)
  })
})
