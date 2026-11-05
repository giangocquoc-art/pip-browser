use crate::framework::{ToolCtx, ToolExecResult, ToolResult, parse_args, pending_dialog_result};
use browseros_core::{
    PageId,
    screenshot::{ScreenshotCaptureOptions, ScreenshotFormat as CoreScreenshotFormat, Viewport},
};
use futures_util::future::BoxFuture;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

const DEFAULT_SCREENSHOT_QUALITY: i64 = 80;
const MAX_REGION_EDGE: i64 = 2048;
const MAX_REGION_PIXELS: i64 = 1_048_576;
const DESCRIPTION: &str = concat!(
    "Capture a screenshot of the page or a viewport region, returned inline. ",
    "Defaults to JPEG quality 80 at the current CSS viewport, with native device pixel density; ",
    "prefer snapshot for structure/actions. Use region for canvas, games, charts, or image-only content. ",
    "region uses CSS pixels relative to the visible viewport and returns only that crop. ",
    "Pass the observationId from visual_targets; the crop must fit inside one returned visual target, and stale tokens are rejected. ",
    "Crop limit is 1 megapixel. Privacy: image bytes are returned to the MCP caller, which may forward them to an external model; this tool does not save the image.",
);

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum ScreenshotFormat {
    #[default]
    Jpeg,
    Png,
    Webp,
}

/// Optional CSS viewport bounds; only supplied axes constrain the capture scale.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ScreenshotSize {
    #[schemars(range(min = 1, max = 4096))]
    width: Option<i64>,
    #[schemars(range(min = 1, max = 4096))]
    height: Option<i64>,
}

/// Visible viewport rectangle in CSS pixels. Crops remain on-screen and bounded.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct ScreenshotRegion {
    #[schemars(range(min = 0, max = 16384))]
    x: i64,
    #[schemars(range(min = 0, max = 16384))]
    y: i64,
    #[schemars(range(min = 1, max = 2048))]
    width: i64,
    #[schemars(range(min = 1, max = 2048))]
    height: i64,
    /// Token returned by visual_targets; prevents using stale bounds after layout changes.
    observation_id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ScreenshotArgs {
    page: u32,
    #[serde(default)]
    format: ScreenshotFormat,
    #[schemars(range(min = 0, max = 100))]
    quality: Option<i64>,
    /// Max viewport size in CSS pixels, preserving aspect ratio without upscaling.
    /// No cap by default; omitted width or height is unconstrained. Ignored for fullPage.
    size: Option<ScreenshotSize>,
    /// Capture beyond the viewport.
    #[serde(rename = "fullPage")]
    full_page: Option<bool>,
    /// Crop a visible viewport region in CSS pixels. Requires a fresh visual_targets observationId.
    /// Cannot be combined with fullPage or size.
    region: Option<ScreenshotRegion>,
    /// Overlay numbered refs from a fresh snapshot. Defaults false.
    annotate: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayoutMetrics {
    css_layout_viewport: Option<LayoutViewport>,
    layout_viewport: LayoutViewport,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayoutViewport {
    page_x: f64,
    page_y: f64,
    client_width: f64,
    client_height: f64,
}

pub fn definition() -> crate::framework::ToolDef {
    super::def::<ScreenshotArgs>(
        "screenshot",
        DESCRIPTION,
        Some(super::read_only_annotations()),
        handler,
    )
}

fn handler<'a>(
    raw: Value,
    ctx: &'a ToolCtx,
    _response: &'a mut crate::response::ToolResponse,
) -> BoxFuture<'a, ToolExecResult<Option<ToolResult>>> {
    Box::pin(async move {
        let args: ScreenshotArgs = parse_args(raw)?;
        validate_args(&args)?;
        if args.annotate.unwrap_or(false)
            && let Some(result) = pending_dialog_result(ctx, PageId(args.page))
        {
            return Ok(Some(result));
        }
        let full_page = args.full_page.unwrap_or(false);
        let mut options = ScreenshotCaptureOptions {
            format: Some(core_format(&args.format)),
            quality: screenshot_quality(&args.format, args.quality),
            full_page: Some(full_page),
            annotate: Some(args.annotate.unwrap_or(false)),
            clip: None,
        };
        if !full_page {
            // A bound view's page session brackets Page.getLayoutMetrics with scope and
            // target-incarnation checks; the raw registry session would bypass the profile.
            let page = ctx.browser.page_session(PageId(args.page)).await?;
            if let Some(region) = args.region.as_ref() {
                let current = super::visual_targets::current_observation(
                    PageId(args.page),
                    &page,
                    &region.observation_id,
                )
                .await?;
                if current.observation_id != region.observation_id {
                    return Err(crate::framework::ToolError::InvalidArguments(vec![
                        crate::framework::ArgIssue {
                            path: "region.observationId".to_string(),
                            message: "page layout changed; call visual_targets again and use its fresh observationId".to_string(),
                        },
                    ]));
                }
                if !crop_is_contained_by_target(region, &current.regions) {
                    return Err(crate::framework::ToolError::InvalidArguments(vec![
                        crate::framework::ArgIssue {
                            path: "region".to_string(),
                            message: "crop must be fully contained within one of the visual targets returned by visual_targets".to_string(),
                        },
                    ]));
                }
            }
            options.clip = Some(if let Some(region) = args.region.as_ref() {
                build_region_clip(&page.session, region).await?
            } else {
                build_screenshot_clip(&page.session, args.size.as_ref()).await?
            });
        }
        let result = ctx.browser.screenshot(PageId(args.page), options).await?;
        if let Some(region) = args.region.as_ref() {
            let page = ctx.browser.page_session(PageId(args.page)).await?;
            let current = super::visual_targets::current_observation(
                PageId(args.page),
                &page,
                &region.observation_id,
            )
            .await?;
            if current.observation_id != region.observation_id {
                return Ok(Some(crate::framework::error_result(
                    "page layout changed during crop; discard it and call visual_targets again"
                        .to_string(),
                )));
            }
        }
        let bytes = estimated_base64_decoded_len(&result.data);
        let mut structured = json!({
            "page": args.page,
            "format": format_name(&args.format),
            "bytes": bytes,
            "imageDelivery": "inline to MCP caller; caller may forward externally; not stored by this tool"
        });
        if let Some(region) = &args.region
            && let Value::Object(object) = &mut structured
        {
            object.insert(
                "region".to_string(),
                json!({
                    "x": region.x,
                    "y": region.y,
                    "width": region.width,
                    "height": region.height,
                    "observationId": region.observation_id,
                    "coordinateSpace": "visible-viewport-css-pixels"
                }),
            );
        }
        if !result.annotations.is_empty()
            && let Value::Object(object) = &mut structured
        {
            object.insert(
                "annotations".to_string(),
                Value::Array(
                    result
                        .annotations
                        .iter()
                        .map(|annotation| {
                            let mut value = json!({
                                "ref": annotation.ref_id,
                                "number": annotation.number,
                                "role": annotation.role,
                                "box": {
                                    "x": annotation.box_.x,
                                    "y": annotation.box_.y,
                                    "width": annotation.box_.width,
                                    "height": annotation.box_.height
                                }
                            });
                            if let (Value::Object(object), Some(name)) =
                                (&mut value, &annotation.name)
                            {
                                object.insert("name".to_string(), json!(name));
                            }
                            value
                        })
                        .collect(),
                ),
            );
        }
        Ok(Some(ToolResult::image(
            result.data,
            result.mime_type,
            structured,
        )))
    })
}

async fn build_screenshot_clip(
    session: &browseros_core::ProtocolSession,
    target: Option<&ScreenshotSize>,
) -> ToolExecResult<Viewport> {
    let metrics: LayoutMetrics = session.send("Page.getLayoutMetrics", json!({})).await?;
    let viewport = metrics
        .css_layout_viewport
        .unwrap_or(metrics.layout_viewport);
    // The clip covers the user's CSS viewport without resizing the page. There
    // is no default cap: scale 1 preserves native device density (2x on Retina).
    let mut scale = 1.0_f64;
    if let Some(target) = target {
        if let Some(width) = target.width
            && viewport.client_width > 0.0
        {
            scale = scale.min(width as f64 / viewport.client_width);
        }
        if let Some(height) = target.height
            && viewport.client_height > 0.0
        {
            scale = scale.min(height as f64 / viewport.client_height);
        }
    }
    Ok(Viewport {
        x: viewport.page_x,
        y: viewport.page_y,
        width: viewport.client_width,
        height: viewport.client_height,
        scale,
    })
}

async fn build_region_clip(
    session: &browseros_core::ProtocolSession,
    region: &ScreenshotRegion,
) -> ToolExecResult<Viewport> {
    let metrics: LayoutMetrics = session.send("Page.getLayoutMetrics", json!({})).await?;
    let viewport = metrics
        .css_layout_viewport
        .unwrap_or(metrics.layout_viewport);
    let x = region.x as f64;
    let y = region.y as f64;
    let width = region.width as f64;
    let height = region.height as f64;
    if x + width > viewport.client_width || y + height > viewport.client_height {
        return Err(crate::framework::ToolError::InvalidArguments(vec![
            crate::framework::ArgIssue {
                path: "region".to_string(),
                message: "region must fit inside the current visible viewport".to_string(),
            },
        ]));
    }
    Ok(Viewport {
        x: viewport.page_x + x,
        y: viewport.page_y + y,
        width,
        height,
        scale: 1.0,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CssRect {
    x: i64,
    y: i64,
    width: i64,
    height: i64,
}

impl CssRect {
    fn contains(self, other: Self) -> bool {
        if self.x < 0
            || self.y < 0
            || self.width <= 0
            || self.height <= 0
            || other.x < 0
            || other.y < 0
            || other.width <= 0
            || other.height <= 0
        {
            return false;
        }
        let (Some(self_right), Some(self_bottom), Some(other_right), Some(other_bottom)) = (
            self.x.checked_add(self.width),
            self.y.checked_add(self.height),
            other.x.checked_add(other.width),
            other.y.checked_add(other.height),
        ) else {
            return false;
        };
        other.x >= self.x
            && other.y >= self.y
            && other_right <= self_right
            && other_bottom <= self_bottom
    }
}

fn crop_is_contained_by_target(region: &ScreenshotRegion, targets: &Value) -> bool {
    let crop = CssRect {
        x: region.x,
        y: region.y,
        width: region.width,
        height: region.height,
    };
    targets.as_array().is_some_and(|targets| {
        targets.iter().any(|target| {
            let Some(rect) = target.as_object() else {
                return false;
            };
            let target = CssRect {
                x: rect.get("x").and_then(Value::as_i64).unwrap_or(-1),
                y: rect.get("y").and_then(Value::as_i64).unwrap_or(-1),
                width: rect.get("width").and_then(Value::as_i64).unwrap_or(0),
                height: rect.get("height").and_then(Value::as_i64).unwrap_or(0),
            };
            target.contains(crop)
        })
    })
}

fn validate_args(args: &ScreenshotArgs) -> ToolExecResult<()> {
    if args.region.is_some() && args.size.is_some() {
        return Err(crate::framework::ToolError::InvalidArguments(vec![
            crate::framework::ArgIssue {
                path: "region".to_string(),
                message: "region cannot be combined with size".to_string(),
            },
        ]));
    }
    if args.region.is_some() && args.full_page.unwrap_or(false) {
        return Err(crate::framework::ToolError::InvalidArguments(vec![
            crate::framework::ArgIssue {
                path: "region".to_string(),
                message: "region cannot be combined with fullPage".to_string(),
            },
        ]));
    }
    if let Some(region) = &args.region
        && (region.x < 0
            || region.y < 0
            || region.width <= 0
            || region.height <= 0
            || region.width > MAX_REGION_EDGE
            || region.height > MAX_REGION_EDGE
            || region.width.saturating_mul(region.height) > MAX_REGION_PIXELS)
    {
        return Err(crate::framework::ToolError::InvalidArguments(vec![
            crate::framework::ArgIssue {
                path: "region".to_string(),
                message: "x/y must be non-negative; crop edges must be between 1 and 2048 CSS pixels and total crop area cannot exceed 1 megapixel".to_string(),
            },
        ]));
    }
    if let Some(quality) = args.quality
        && !(0..=100).contains(&quality)
    {
        return Err(crate::framework::ToolError::InvalidArguments(vec![
            crate::framework::ArgIssue {
                path: "quality".to_string(),
                message: "Number must be greater than or equal to 0 and less than or equal to 100"
                    .to_string(),
            },
        ]));
    }
    if let Some(size) = &args.size
        && [size.width, size.height]
            .into_iter()
            .flatten()
            .any(|dimension| !(1..=4096).contains(&dimension))
    {
        return Err(crate::framework::ToolError::InvalidArguments(vec![
            crate::framework::ArgIssue {
                path: "size".to_string(),
                message: "supplied width and height must be between 1 and 4096".to_string(),
            },
        ]));
    }
    Ok(())
}

fn estimated_base64_decoded_len(encoded: &str) -> usize {
    let padding = encoded
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count()
        .min(2);
    encoded
        .len()
        .saturating_mul(3)
        .saturating_div(4)
        .saturating_sub(padding)
}

fn screenshot_quality(format: &ScreenshotFormat, quality: Option<i64>) -> Option<i64> {
    if matches!(format, ScreenshotFormat::Jpeg) {
        Some(quality.unwrap_or(DEFAULT_SCREENSHOT_QUALITY))
    } else {
        None
    }
}

fn core_format(format: &ScreenshotFormat) -> CoreScreenshotFormat {
    match format {
        ScreenshotFormat::Jpeg => CoreScreenshotFormat::Jpeg,
        ScreenshotFormat::Png => CoreScreenshotFormat::Png,
        ScreenshotFormat::Webp => CoreScreenshotFormat::Webp,
    }
}

fn format_name(format: &ScreenshotFormat) -> &'static str {
    match format {
        ScreenshotFormat::Jpeg => "jpeg",
        ScreenshotFormat::Png => "png",
        ScreenshotFormat::Webp => "webp",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(region: Value, extra: Value) -> ScreenshotArgs {
        let mut value = json!({ "page": 1, "region": region });
        if let (Value::Object(base), Value::Object(extra)) = (&mut value, extra) {
            base.extend(extra);
        }
        serde_json::from_value(value).expect("valid screenshot arguments")
    }

    #[test]
    fn region_crop_requires_positive_bounded_dimensions() {
        let valid = args(
            json!({
                "x": 0,
                "y": 0,
                "width": 640,
                "height": 480,
                "observationId": "0123abcd"
            }),
            json!({}),
        );
        assert!(validate_args(&valid).is_ok());

        let invalid = args(
            json!({
                "x": -1,
                "y": 0,
                "width": 640,
                "height": 480,
                "observationId": "0123abcd"
            }),
            json!({}),
        );
        assert!(validate_args(&invalid).is_err());

        let too_large = args(
            json!({
                "x": 0,
                "y": 0,
                "width": 2048,
                "height": 1024,
                "observationId": "0123abcd"
            }),
            json!({}),
        );
        assert!(validate_args(&too_large).is_err());
    }

    #[test]
    fn region_crop_must_fit_inside_a_returned_visual_target() {
        let region = |x, y, width, height| ScreenshotRegion {
            x,
            y,
            width,
            height,
            observation_id: "v1:12:0123abcd".to_string(),
        };
        let targets = json!([
            { "x": 40, "y": 30, "width": 200, "height": 120 },
            { "x": 400, "y": 100, "width": 80, "height": 80 }
        ]);

        assert!(crop_is_contained_by_target(
            &region(40, 30, 200, 120),
            &targets
        ));
        assert!(crop_is_contained_by_target(
            &region(80, 50, 40, 40),
            &targets
        ));
        assert!(crop_is_contained_by_target(
            &region(420, 120, 20, 20),
            &targets
        ));
        assert!(!crop_is_contained_by_target(
            &region(39, 30, 20, 20),
            &targets
        ));
        assert!(!crop_is_contained_by_target(
            &region(220, 30, 21, 20),
            &targets
        ));
        assert!(!crop_is_contained_by_target(
            &region(250, 60, 20, 20),
            &targets
        ));
        assert!(!crop_is_contained_by_target(
            &region(40, 30, 0, 20),
            &targets
        ));
    }

    #[test]
    fn region_crop_cannot_be_combined_with_full_page_or_resize() {
        let region = json!({
            "x": 10,
            "y": 10,
            "width": 100,
            "height": 100,
            "observationId": "0123abcd"
        });
        let full_page = args(region.clone(), json!({ "fullPage": true }));
        assert!(validate_args(&full_page).is_err());

        let resized = args(region, json!({ "size": { "width": 100 } }));
        assert!(validate_args(&resized).is_err());
    }

    #[test]
    fn region_requires_a_layout_observation_id() {
        let error = serde_json::from_value::<ScreenshotArgs>(json!({
            "page": 1,
            "region": { "x": 0, "y": 0, "width": 100, "height": 100 }
        }));
        assert!(error.is_err());
    }

    #[test]
    fn encoded_image_size_is_counted_without_decoding_a_second_buffer() {
        assert_eq!(estimated_base64_decoded_len(""), 0);
        assert_eq!(estimated_base64_decoded_len("Zg=="), 1);
        assert_eq!(estimated_base64_decoded_len("Zm8="), 2);
        assert_eq!(estimated_base64_decoded_len("Zm9v"), 3);
    }
}
