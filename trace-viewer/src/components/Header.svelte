<script lang="ts">
  import type { Properties } from "../types.js";
  let { summary, hasFailure, canJump, onjump }: {
    summary: Properties; hasFailure: boolean; canJump: boolean; onjump: () => void;
  } = $props();
</script>

<header class="topbar">
  <div class="brand">
    <svg class="brand-mark" viewBox="0 0 32 32" aria-hidden="true">
      <rect x="3" y="4" width="26" height="20" rx="3" />
      <path d="m8 10 4 4-4 4m9 0h6M12 24l-2 5m10-5 2 5M8 29h16" />
    </svg>
    <div><strong>tui-test</strong><span>Trace viewer</span></div>
  </div>
  <div id="session-summary" class="session-summary">
    {#each Object.entries(summary) as [label, value] (label)}
      <span class="metadata-item" title={`${label}: ${value}`}>
        <span class="metadata-label">{label}:</span><span class="metadata-value">{value}</span>
      </span>
    {/each}
  </div>
  <button id="failure" class="failure-button" class:failed={hasFailure} disabled={!canJump} onclick={onjump}>
    {hasFailure ? "Jump to failure" : "Jump to end"}
  </button>
</header>

<style>
  .topbar { min-height: 68px; display: flex; gap: 32px; align-items: center; padding: 0 0 16px; margin-bottom: 16px; border-bottom: 1px solid var(--border); flex-shrink: 0; }
  .brand { display: inline-flex; align-items: center; gap: 12px; white-space: nowrap; font-family: var(--mono); }
  .brand-mark { width: 34px; height: 34px; flex-shrink: 0; fill: none; stroke: var(--accent); stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
  .brand strong { font-size: 22px; font-weight: 600; letter-spacing: -.04em; line-height: 1.3; }
  .brand span { display: block; color: var(--muted); font-size: 10px; letter-spacing: .07em; text-transform: uppercase; }
  .session-summary { display: flex; flex: 1; flex-wrap: wrap; gap: 8px 24px; min-width: 0; font: 11px/1.6 var(--mono); }
  .metadata-item { display: inline-flex; flex-direction: column; align-items: flex-start; min-width: 0; max-width: 100%; }
  .metadata-label { color: var(--muted); font-size: 9px; letter-spacing: .07em; text-transform: uppercase; }
  .metadata-value { max-width: 100%; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .failure-button { color: var(--link); border-color: var(--link); white-space: nowrap; }
  .failure-button.failed { color: var(--red); border-color: var(--red); }
  @media (max-width: 850px) { .session-summary, .topbar { gap: 12px; } }
  @media (max-width: 600px) { .topbar { flex-wrap: wrap; padding: 8px 4px 16px; } .session-summary { order: 1; flex-basis: 100%; } .failure-button { margin-left: auto; } }
</style>
