---
name: browser
description: Use Pip Dev's MCP tools for careful, structured browser inspection and interaction.
---

Use Pip Dev MCP when the user asks you to work with a page open in Pip Dev.

- First identify and select the intended tab. Never infer the target from whichever tab happens to be active.
- Prefer DOM snapshots, application-provided state, stable element references, and diffs. Read the current state again after navigation, scrolling, layout changes, or when a reference becomes stale.
- Group independent read-only inspection where the available tools allow it. After actions that change a page, wait for the result and verify the new state before continuing.
- For canvas, image, video, or other pixel-only content, locate the relevant visual target and request only its current crop using its observation identifier. Avoid full-page screenshots. Treat a crop as potentially sensitive and tell the user if it will be sent to an external vision service; do not retain it.
- When Pip Dev reports that an agent controls a tab, keep the task on that tab. Verify each important action, stop on uncertain state or unexpected navigation, and use the provided stop/release action when done or when the user asks to take control.
- Never continue with stale coordinates, repeat an uncertain click blindly, or claim that a canvas was understood when the visual evidence is ambiguous.
