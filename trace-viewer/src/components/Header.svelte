<script lang="ts">
  import type { Properties } from "../types.js";
  let { summary, hasFailure, canJump, onjump }: {
    summary: Properties; hasFailure: boolean; canJump: boolean; onjump: () => void;
  } = $props();
</script>

<header class="topbar">
  <strong class="brand">tui-test <span>Trace viewer</span></strong>
  <div id="session-summary" class="session-summary">
    {#each Object.entries(summary) as [label, value] (label)}
      <span class="metadata-item" title={`${label}: ${value}`}>
        <span class="metadata-label">{label}:</span><span class="metadata-value">{value}</span>
      </span>
    {/each}
  </div>
  <button id="failure" class="failure-button" disabled={!canJump} onclick={onjump}>
    {hasFailure ? "Jump to failure" : "Jump to end"}
  </button>
</header>

<style>
  .topbar { min-height: 52px; display: flex; gap: 24px; align-items: center; padding: 8px 16px; border-bottom: 1px solid var(--border); flex-shrink: 0; line-height: 20px; }
  .brand { display: inline-flex; align-items: center; gap: 10px; white-space: nowrap; font-size: 15px; color: var(--green); }
  .brand span { color: var(--muted); font-size: 12px; font-weight: 400; }
  .session-summary { display: flex; flex: 1; flex-wrap: wrap; gap: 8px 24px; min-width: 0; font-size: 12px; }
  .metadata-item { display: inline-flex; align-items: center; gap: 6px; min-width: 0; max-width: 100%; }
  .metadata-label { flex-shrink: 0; color: var(--muted); }
  .metadata-value { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .failure-button { color: var(--red); border-color: var(--red); white-space: nowrap; }
  @media (max-width: 850px) { .brand span { display: none; } .session-summary, .topbar { gap: 10px; } }
  @media (max-width: 600px) { .session-summary { gap: 0 8px; } .failure-button { font-size: 11px; } }
</style>
