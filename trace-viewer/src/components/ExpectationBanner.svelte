<script lang="ts">
  import type { TraceAction, TraceModel } from "../types.js";
  let { action, isFailure, result }: { action?: TraceAction; isFailure: boolean; result: TraceModel["result"] } = $props();
  const banner = $derived.by(() => {
    const expectation = action?.expectation;
    if (isFailure) return {
      outcome: "failed", summary: expectation ? `Failed: ${expectation}` : `Expected ${result.expected}`, observed: result.actual,
    };
    if (expectation && !action?.unavailableExpectation) return {
      outcome: action?.failed ? "failed" : "passed",
      summary: `${action?.failed ? "Failed" : action?.assertion ? "Passed" : "Completed"}: ${expectation}`,
      observed: action?.failed ? "The selected operation did not complete successfully." : "The selected operation completed successfully.",
    };
    return { outcome: "unknown", summary: expectation || "Expectation not captured for this operation.", observed: "" };
  });
</script>

<div id="expectation-banner" class="expectation-banner" data-outcome={banner.outcome}
  hidden={!action?.expectation && !isFailure && !action?.assertion}>
  <strong id="summary">{banner.summary}</strong><span id="summary-observed">{banner.observed}</span>
</div>

<style>
  .expectation-banner { display: grid; gap: 2px; margin: 6px 8px 0; background: var(--panel); border: 1px solid var(--border); border-left: 2px solid var(--muted); padding: 6px 8px; font-size: 11px; overflow: auto; max-height: 88px; flex-shrink: 0; }
  strong { font-weight: 600; white-space: pre-wrap; overflow-wrap: anywhere; }
  [data-outcome="failed"] { background: var(--error-bg); border-left-color: var(--red); }
  [data-outcome="failed"] strong { color: var(--red); }
  [data-outcome="passed"] { border-left-color: var(--green); }
  [data-outcome="passed"] strong { color: var(--green); }
  @media (max-width: 600px) { .expectation-banner { margin: 4px 6px 0; } }
</style>
