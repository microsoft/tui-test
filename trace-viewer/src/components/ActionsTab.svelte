<script lang="ts">
  import type { TraceAction } from "../types.js";
  import { time } from "../format.js";
  let { actions, selected, filter, assertionsOnly, onselect, onfilter, onassertions }: {
    actions: readonly TraceAction[]; selected?: number; filter: string; assertionsOnly: boolean;
    onselect: (id: number) => void; onfilter: (filter: string) => void; onassertions: (value: boolean) => void;
  } = $props();
  let points: HTMLElement | undefined = $state();
  $effect(() => {
    if (selected !== undefined) points?.querySelector(`[data-operation="${selected}"]`)?.scrollIntoView({ block: "nearest" });
  });
</script>

<div class="action-filter">
  <input id="action-filter" type="search" aria-label="Filter actions" placeholder="Filter actions"
    value={filter} oninput={(event) => onfilter(event.currentTarget.value)} />
  <label><input id="assertions-only" type="checkbox" checked={assertionsOnly}
    onchange={(event) => onassertions(event.currentTarget.checked)} /> Assertions and waits only</label>
</div>
<nav id="points" bind:this={points} aria-label="Recorded actions">
  {#each actions as action (action.id)}
    <button class="point" class:failed={action.failed} data-operation={action.id} aria-current={selected === action.id}
      title={`${action.name} ${action.label} / screen ${action.screen}`} aria-label={`${action.name} ${action.label} ${action.result}`}
      onclick={() => onselect(action.id)}>
      <span class="status"></span>
      <span class="action-label">
        <span class="action-line"><span class="name">{action.name}</span>{#if action.code}<code class="query-code">{action.label}</code>{/if}</span>
        {#if !action.code}<span class="description">{action.label}</span>{/if}
      </span>
      <span class="result">{time(action.duration)}{action.frameIndex === undefined ? "\nnot retained" : ""}</span>
    </button>
  {:else}
    <p class="muted">No matching actions.</p>
  {/each}
</nav>

<style>
  .action-filter { padding: 8px; display: grid; gap: 6px; border-bottom: 1px solid var(--border); }
  .action-filter label { font-size: 11px; color: var(--muted); }
  .point { display: grid; grid-template-columns: 10px minmax(0, 1fr) auto; gap: 7px; width: 100%; border: 0; border-radius: 0; padding: 9px 10px; text-align: left; }
  .status { width: 6px; height: 6px; border-radius: 50%; background: var(--green); margin-top: 6px; }
  .failed .status { background: var(--red); }
  .name, .description { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .action-label { min-width: 0; }
  .action-line { display: flex; gap: 9px; align-items: baseline; min-width: 0; }
  .action-line .name { flex: 0 1 auto; max-width: 48%; }
  .query-code { color: #887120; font: 12px/1.5 Consolas, monospace; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; }
  .description { font-size: 10px; color: var(--muted); margin-top: 2px; }
  .result { font-size: 10px; color: var(--muted); text-align: right; white-space: pre-line; }
  .failed .name, .failed .result { color: var(--red); }
  .point[aria-current="true"] { background: var(--selected); }
  @media (prefers-color-scheme: dark) { .query-code { color: #d5bf75; } }
  @media (max-width: 600px) { .point { padding: 8px 5px; gap: 4px; } }
</style>
