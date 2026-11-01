<script lang="ts">
  import type { Properties } from "../types.js";
  let { summary, hasFailure, canJump, onjump }: {
    summary: Properties; hasFailure: boolean; canJump: boolean; onjump: () => void;
  } = $props();
</script>

<header class="topbar">
  <div class="brand">
    <svg class="brand-mark" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="none" aria-hidden="true">
      <rect x="32" y="36" width="192" height="144" rx="28" fill="none" stroke="#33FF33" stroke-width="20" />
      <path d="M 104,180 L 92,220 M 152,180 L 164,220 M 72,220 L 184,220" fill="none" stroke="#33FF33" stroke-width="18" stroke-linecap="round" stroke-linejoin="round" />
      <polyline points="72,86 100,106 72,126" fill="none" stroke="#7CD67C" stroke-width="16" stroke-linecap="round" stroke-linejoin="round" />
      <polyline points="140,108 156,124 188,86" fill="none" stroke="#7CD67C" stroke-width="16" stroke-linecap="round" stroke-linejoin="round" />
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
  .topbar { min-height: 36px; display: flex; gap: 20px; align-items: center; padding-bottom: 6px; margin-bottom: 6px; border-bottom: 1px solid var(--border); flex-shrink: 0; }
  .brand { display: inline-flex; align-items: center; gap: 6px; white-space: nowrap; font-family: var(--mono); }
  .brand-mark { width: 28px; height: 28px; flex-shrink: 0; }
  .brand > div { display: flex; align-items: baseline; gap: 8px; }
  .brand strong { font-size: 18px; font-weight: 600; letter-spacing: -.04em; line-height: 1.3; }
  .brand span { color: var(--muted); font-size: 9px; letter-spacing: .07em; text-transform: uppercase; }
  .session-summary { display: flex; flex: 1; flex-wrap: wrap; gap: 4px 16px; min-width: 0; font: 10px/1.4 var(--mono); }
  .metadata-item { display: inline-flex; align-items: baseline; gap: 4px; min-width: 0; max-width: 100%; }
  .metadata-label { flex-shrink: 0; color: var(--muted); font-size: 9px; letter-spacing: .04em; text-transform: uppercase; }
  .metadata-value { max-width: 100%; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .failure-button { color: var(--link); border-color: var(--link); white-space: nowrap; }
  .failure-button.failed { color: var(--red); border-color: var(--red); }
  @media (max-width: 850px) { .brand span { display: none; } .session-summary, .topbar { gap: 8px; } }
  @media (max-width: 600px) { .topbar { flex-wrap: wrap; padding: 2px 2px 6px; } .session-summary { order: 1; flex-basis: 100%; } .failure-button { margin-left: auto; } }
</style>
