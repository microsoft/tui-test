<script lang="ts">
  import type { TraceModel } from "../types.js";
  import LocatorBlock from "./LocatorBlock.svelte";
  let { result, hasFailure }: { result: TraceModel["result"]; hasFailure: boolean } = $props();
</script>

<div class="error-heading">
  <h1 id="operation" class:passed={!hasFailure}>{result.title}</h1><span id="failure-timing" class="muted">{result.timing}</span>
</div>
<LocatorBlock id="error-locator" locator={result.locator} />
<div id="comparison" class="comparison" hidden={!hasFailure}>
  <div class="expected"><h2>Expected</h2><pre id="expected">{result.expected}</pre></div>
  <div class="actual"><h2>Observed</h2><pre id="actual">{result.actual}</pre></div>
</div>
<p id="explanation-note" class="muted">{result.note}</p>
<ul id="hints">{#each result.hints as hint}<li>{hint}</li>{/each}</ul>
<details>
  <summary id="raw-details-heading">{hasFailure ? "Original error and locator evidence" : "Raw trace evidence"}</summary>
  <pre id="raw-error">{result.error}</pre><pre id="diagnosis">{result.evidence}</pre>
</details>

<style>
  .error-heading { display: flex; align-items: baseline; gap: 8px; flex-wrap: wrap; }
  h1 { color: var(--red); }
  h1.passed { color: var(--green); }
  .comparison { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin-top: 8px; }
  .comparison > div { border: 1px solid var(--border); border-left-width: 2px; padding: 6px 8px; min-width: 0; background: var(--panel); }
  .comparison .expected { border-left-color: var(--green); }
  .comparison .actual { border-left-color: var(--red); }
  .comparison h2 { margin: 0 0 4px; }
  @media (max-width: 600px) { .comparison { grid-template-columns: 1fr; } }
</style>
