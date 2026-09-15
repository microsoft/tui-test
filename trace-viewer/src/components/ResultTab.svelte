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
  .error-heading { display: flex; align-items: baseline; gap: 14px; flex-wrap: wrap; }
  h1 { color: var(--red); }
  h1.passed { color: var(--green); }
  .comparison { display: grid; grid-template-columns: 1fr 1fr; gap: 18px; }
  .comparison > div { border-left: 3px solid var(--border); padding-left: 10px; min-width: 0; }
  .comparison .expected { border-color: var(--green); }
  .comparison .actual { border-color: var(--red); }
  .comparison h2 { color: var(--muted); font-size: 10px; text-transform: uppercase; letter-spacing: .07em; }
  .comparison pre { max-height: 90px; }
  @media (max-width: 600px) { .comparison { grid-template-columns: 1fr; } }
</style>
