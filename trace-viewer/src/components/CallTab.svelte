<script lang="ts">
  import type { TraceAction, TraceFrame } from "../types.js";
  import { json, time } from "../format.js";
  import PropertyList from "./PropertyList.svelte";
  import LocatorBlock from "./LocatorBlock.svelte";
  let { action, frame, missingScreen }: { action?: TraceAction; frame?: TraceFrame; missingScreen?: number } = $props();
  const properties = $derived({
    Action: action?.name ?? "Frame inspection", ...action?.properties,
    ...(action?.expectation ? { Expected: action.expectation } : {}),
    Result: action?.result, Duration: action && time(action.duration), Started: action && time(action.started), Completed: action && time(action.ended),
    Screen: frame?.sequence, "Terminal cursor (column, row)": frame && `${frame.cursor.x}, ${frame.cursor.y}`,
    "Cursor appearance": frame?.cursorAppearance,
  });
  const evidence = $derived(json(frame ? { operation: action?.evidence ?? null, frame: frame.evidence }
    : { operation: action?.evidence, missing_screen_sequence: missingScreen }));
</script>

<LocatorBlock id="call-locator" locator={action?.locator} />
<PropertyList id="call-properties" values={properties} />
<details><summary>Operation and frame metadata</summary><pre id="frame-details">{evidence}</pre></details>
