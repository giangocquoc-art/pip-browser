import type { ScreenshotCaptureOptions } from '@browseros/browser-core/core/screenshot'
import type { Viewport } from '@browseros/cdp-protocol/domains/page'
import type { ProtocolApi } from '@browseros/cdp-protocol/protocol-api'
import { z } from 'zod/v4'
import { defineTool } from './framework'

const DEFAULT_SCREENSHOT_FORMAT = 'jpeg'
const DEFAULT_SCREENSHOT_QUALITY = 80
const screenshotFormat = z.enum(['jpeg', 'png', 'webp'])
const screenshotSize = z
  .object({
    width: z.number().int().positive().max(4096).optional(),
    height: z.number().int().positive().max(4096).optional(),
  })
  .strict()
const viewportSchema = z
  .object({
    x: z.number().finite(),
    y: z.number().finite(),
    width: z.number().positive().finite(),
    height: z.number().positive().finite(),
  })
  .strict()
const screenshotRegion = z
  .object({
    x: z.number().finite().nonnegative(),
    y: z.number().finite().nonnegative(),
    width: z.number().positive().finite().max(16384),
    height: z.number().positive().finite().max(16384),
  })
  .strict()

type ScreenshotFormat = z.infer<typeof screenshotFormat>
type ScreenshotSize = z.infer<typeof screenshotSize>
type ScreenshotRegion = z.infer<typeof screenshotRegion>
type ScreenshotViewport = z.infer<typeof viewportSchema>

function screenshotQuality(format: ScreenshotFormat, quality?: number) {
  if (format !== 'jpeg') return undefined
  return quality ?? DEFAULT_SCREENSHOT_QUALITY
}

/**
 * Builds a viewport clip and rejects crops based on stale geometry. Crop x/y
 * are CSS pixels relative to the visible viewport; observedViewport binds them
 * to the scroll position and size returned by a preceding screenshot call.
 */
export async function buildScreenshotClip(
  session: ProtocolApi,
  target?: ScreenshotSize,
  region?: ScreenshotRegion,
  observedViewport?: ScreenshotViewport,
): Promise<{ clip: Viewport; viewport: ScreenshotViewport }> {
  const metrics = await session.Page.getLayoutMetrics()
  const viewport = metrics.cssLayoutViewport ?? metrics.layoutViewport
  const currentViewport = {
    x: viewport.pageX,
    y: viewport.pageY,
    width: viewport.clientWidth,
    height: viewport.clientHeight,
  }
  if (currentViewport.width <= 0 || currentViewport.height <= 0) {
    throw new Error('The visible page viewport has no usable size; retry after it loads.')
  }

  let x = currentViewport.x
  let y = currentViewport.y
  let width = currentViewport.width
  let height = currentViewport.height
  if (region) {
    if (!observedViewport) {
      throw new Error(
        'A crop requires observedViewport from a recent screenshot. Capture a fresh screenshot and retry with its viewport metadata.',
      )
    }
    if (!sameViewport(currentViewport, observedViewport)) {
      throw new Error(
        'The page viewport or scroll position changed since it was observed. Capture a fresh screenshot and recalculate the crop.',
      )
    }
    if (
      region.x + region.width > currentViewport.width ||
      region.y + region.height > currentViewport.height
    ) {
      throw new Error(
        `Crop is outside the visible viewport (${currentViewport.width}x${currentViewport.height} CSS px). Capture a fresh screenshot and recalculate the crop.`,
      )
    }
    x += region.x
    y += region.y
    width = region.width
    height = region.height
  }

  // Capture CSS pixels without resizing the page. No default cap means scale
  // 1 preserves native device density; a target only scales down.
  const scale = Math.min(
    1,
    target?.width !== undefined && width > 0 ? target.width / width : 1,
    target?.height !== undefined && height > 0 ? target.height / height : 1,
  )

  return {
    clip: { x, y, width, height, scale },
    viewport: currentViewport,
  }
}

function sameViewport(a: ScreenshotViewport, b: ScreenshotViewport): boolean {
  return (
    a.x === b.x &&
    a.y === b.y &&
    a.width === b.width &&
    a.height === b.height
  )
}

export const screenshot = defineTool({
  name: 'screenshot',
  description:
    'Capture a screenshot inline; prefer snapshot for structure/actions. For canvas or image work, first capture a viewport screenshot, then request only the needed `region` with its returned `viewport` as `observedViewport`. Crops are rejected after scrolling/resizing. Full-page capture is only for cases that need it.',
  input: z
    .object({
      page: z.number().int(),
      format: screenshotFormat.default(DEFAULT_SCREENSHOT_FORMAT),
      quality: z.number().int().min(0).max(100).optional(),
      size: screenshotSize
        .optional()
        .describe(
          'Max output size in CSS pixels, preserving aspect ratio without upscaling. No cap by default; omitted width or height is unconstrained. Ignored for fullPage.',
        ),
      fullPage: z.boolean().optional().describe('Capture beyond the viewport.'),
      region: screenshotRegion
        .optional()
        .describe(
          'Crop in CSS pixels relative to the visible viewport from a recent screenshot. Requires observedViewport and cannot be combined with fullPage or annotate.',
        ),
      observedViewport: viewportSchema
        .optional()
        .describe(
          'The viewport object returned by the screenshot used to choose region. The crop is refused if scroll position or viewport size has changed.',
        ),
      annotate: z
        .boolean()
        .optional()
        .describe(
          'Overlay numbered refs from a fresh snapshot. Defaults false; not supported with region crops.',
        ),
    })
    .strict(),
  annotations: { title: 'Take screenshot', readOnlyHint: true },
  handler: async (args, ctx) => {
    const fullPage = args.fullPage ?? false
    if (fullPage && (args.region || args.observedViewport)) {
      throw new Error('fullPage cannot be combined with region or observedViewport.')
    }
    if (args.region && !args.observedViewport) {
      throw new Error('region requires observedViewport from a recent screenshot.')
    }
    if (!args.region && args.observedViewport) {
      throw new Error('observedViewport is only valid when region is provided.')
    }
    if (args.region && args.annotate) {
      throw new Error('annotate is not supported with region crops.')
    }

    const captureOptions: ScreenshotCaptureOptions = {
      format: args.format,
      fullPage,
      annotate: args.annotate ?? false,
    }
    const quality = screenshotQuality(args.format, args.quality)
    if (quality !== undefined) captureOptions.quality = quality
    let viewport: ScreenshotViewport | undefined
    if (!fullPage) {
      const { session } = await ctx.session.pages.getSession(args.page)
      const result = await buildScreenshotClip(
        session,
        args.size,
        args.region,
        args.observedViewport,
      )
      captureOptions.clip = result.clip
      viewport = result.viewport
    }

    const result = await ctx.session.screenshot(args.page, captureOptions)
    return {
      content: [
        { type: 'image', data: result.data, mimeType: result.mimeType },
      ],
      structuredContent: {
        page: args.page,
        format: args.format,
        bytes: Buffer.from(result.data, 'base64').length,
        ...(viewport && { viewport }),
        ...(args.region && { region: args.region }),
        ...(result.annotations.length > 0 && {
          annotations: result.annotations,
        }),
      },
    }
  },
})
