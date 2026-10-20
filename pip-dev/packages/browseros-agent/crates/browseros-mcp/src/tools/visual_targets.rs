use crate::framework::{ToolCtx, ToolExecResult, ToolResult, parse_args, text_result};
use browseros_core::PageId;
use futures_util::future::BoxFuture;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_TARGETS: usize = 12;
const DESCRIPTION: &str = "Find visible canvas, image, video, SVG and role=img regions in a page without capturing them. Returns viewport-relative CSS-pixel bounds and an observationId; pass the token with a selected region to screenshot.region for a transient crop. The crop is rejected if the URL, viewport, scroll position, or visual target bounds changed.";

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct VisualTargetsArgs {
    /// Page id from `tabs` or `navigate`.
    page: u32,
    /// Maximum number of visible regions to return (1-12).
    max_targets: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct EvaluateResult {
    result: RemoteObject,
    exception_details: Option<ExceptionDetails>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VisualTargetSnapshot {
    observation_id: String,
    regions: Value,
}

#[derive(Debug, Deserialize)]
struct RemoteObject {
    value: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ExceptionDetails {
    text: String,
}

pub fn definition() -> crate::framework::ToolDef {
    super::def::<VisualTargetsArgs>(
        "visual_targets",
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
        let args: VisualTargetsArgs = parse_args(raw)?;
        let max_targets = args
            .max_targets
            .unwrap_or(MAX_TARGETS as u32)
            .clamp(1, MAX_TARGETS as u32);
        let page = ctx.browser.page_session(PageId(args.page)).await?;
        let expression = format!(
            r#"(() => {{
              const maxTargets = {max_targets};
              const selectors = 'canvas,img,video,svg,[role="img"]';
              const viewportWidth = window.innerWidth || 0;
              const viewportHeight = window.innerHeight || 0;
              const regions = Array.from(document.querySelectorAll(selectors)).flatMap((element, index) => {{
                const rect = element.getBoundingClientRect();
                const style = getComputedStyle(element);
                const left = Math.max(0, rect.left);
                const top = Math.max(0, rect.top);
                const right = Math.min(viewportWidth, rect.right);
                const bottom = Math.min(viewportHeight, rect.bottom);
                if (style.display === 'none' || style.visibility === 'hidden' || Number(style.opacity) === 0 || right - left < 12 || bottom - top < 12) return [];
                const tag = element.tagName.toLowerCase();
                const naturalWidth = element.naturalWidth || element.videoWidth || element.width || 0;
                const naturalHeight = element.naturalHeight || element.videoHeight || element.height || 0;
                return [{{
                  index,
                  type: tag,
                  label: element.getAttribute('aria-label') || element.getAttribute('alt') || element.getAttribute('title') || '',
                  x: Math.round(left),
                  y: Math.round(top),
                  width: Math.round(right - left),
                  height: Math.round(bottom - top),
                  contentWidth: naturalWidth,
                  contentHeight: naturalHeight
                }}];
              }}).slice(0, 64);
              const state = JSON.stringify({{
                url: location.href,
                viewport: [viewportWidth, viewportHeight, window.scrollX || 0, window.scrollY || 0, window.devicePixelRatio || 1],
                regions
              }});
              let hash = 2166136261;
              for (let index = 0; index < state.length; index++) {{
                hash ^= state.charCodeAt(index);
                hash = Math.imul(hash, 16777619);
              }}
              return {{ observationId: (hash >>> 0).toString(16).padStart(8, '0'), regions: regions.slice(0, maxTargets) }};
            }})()"#
        );
        let mut result: EvaluateResult = page
            .session
            .send(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": false
                }),
            )
            .await?;
        if let Some(exception) = result.exception_details.take() {
            return Ok(Some(crate::framework::error_result(format!(
                "visual_targets: {}",
                exception.text
            ))));
        }
        let snapshot: VisualTargetSnapshot =
            serde_json::from_value(result.result.value.unwrap_or_else(|| json!({})))
                .map_err(|error| crate::framework::ToolError::message(error.to_string()))?;
        let regions = snapshot.regions;
        let count = regions.as_array().map_or(0, Vec::len);
        let text = serde_json::to_string_pretty(&regions).unwrap_or_else(|_| "[]".to_string());
        let mut structured = json!({
            "page": args.page,
            "coordinateSpace": "visible-viewport-css-pixels",
            "observationId": snapshot.observation_id,
            "regions": regions
        });
        if let Value::Object(object) = &mut structured {
            object.insert("count".to_string(), json!(count));
        }
        Ok(Some(text_result(text, Some(structured))))
    })
}

pub async fn current_observation_id(
    session: &browseros_core::ProtocolSession,
) -> ToolExecResult<String> {
    let expression = r#"(() => {
      const width = window.innerWidth || 0;
      const height = window.innerHeight || 0;
      const regions = Array.from(document.querySelectorAll('canvas,img,video,svg,[role="img"]')).flatMap((element, index) => {
        const rect = element.getBoundingClientRect();
        const style = getComputedStyle(element);
        const left = Math.max(0, rect.left);
        const top = Math.max(0, rect.top);
        const right = Math.min(width, rect.right);
        const bottom = Math.min(height, rect.bottom);
        if (style.display === 'none' || style.visibility === 'hidden' || Number(style.opacity) === 0 || right - left < 12 || bottom - top < 12) return [];
        const naturalWidth = element.naturalWidth || element.videoWidth || element.width || 0;
        const naturalHeight = element.naturalHeight || element.videoHeight || element.height || 0;
        return [{ index, type: element.tagName.toLowerCase(), label: element.getAttribute('aria-label') || element.getAttribute('alt') || element.getAttribute('title') || '', x: Math.round(left), y: Math.round(top), width: Math.round(right - left), height: Math.round(bottom - top), contentWidth: naturalWidth, contentHeight: naturalHeight }];
      }).slice(0, 64);
      const state = JSON.stringify({ url: location.href, viewport: [width, height, window.scrollX || 0, window.scrollY || 0, window.devicePixelRatio || 1], regions });
      let hash = 2166136261;
      for (let index = 0; index < state.length; index++) { hash ^= state.charCodeAt(index); hash = Math.imul(hash, 16777619); }
      return (hash >>> 0).toString(16).padStart(8, '0');
    })()"#;
    let result: EvaluateResult = session
        .send(
            "Runtime.evaluate",
            json!({ "expression": expression, "returnByValue": true, "awaitPromise": false }),
        )
        .await?;
    if let Some(exception) = result.exception_details {
        return Err(crate::framework::ToolError::message(exception.text));
    }
    result
        .result
        .value
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| {
            crate::framework::ToolError::message(
                "could not read page observation token".to_string(),
            )
        })
}
