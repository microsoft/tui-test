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

<div class="actions-panel">
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
        <span class="status" aria-hidden="true"></span>
        <span class="action-label">
          <span class="action-line"><span class="name">{action.name}</span>{#if action.code}<code class="query-code">{action.label}</code>{:else}<span class="description">{action.label}</span>{/if}</span>
        </span>
        <span class="result">{time(action.duration)}{action.frameIndex === undefined ? " / not retained" : ""}</span>
      </button>
    {:else}
      <p class="muted">No matching actions.</p>
    {/each}
  </nav>
</div>

<style>
  .actions-panel { height: 100%; min-height: 0; display: flex; flex-direction: column; }
  .action-filter { padding: 6px 8px; display: grid; gap: 4px; border-bottom: 1px solid var(--border); flex-shrink: 0; }
  .action-filter label { display: flex; align-items: center; gap: 4px; font-size: 10px; color: var(--muted); }
  .action-filter input[type="checkbox"] { margin: 0; }
  #points { flex: 1; min-height: 0; overflow: auto; }
  .point { display: grid; grid-template-columns: 4px minmax(0, 1fr) auto; align-items: center; gap: 6px; width: 100%; border: 0; border-bottom: 1px solid var(--border); border-radius: 0; padding: 5px 8px; text-align: left; }
  .status { width: 4px; height: 4px; background: var(--green); }
  .failed .status { background: var(--red); }
  .name, .description { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .action-label { min-width: 0; }
  .action-line { display: flex; align-items: baseline; gap: 6px; min-width: 0; }
  .name { flex: 0 1 auto; max-width: 45%; font: 11px/1.4 var(--sans); }
  .query-code { flex: 1; color: var(--link); font: 10px/1.4 var(--mono); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; }
  .description { flex: 1; min-width: 0; font-size: 10px; color: var(--muted); }
  .result { font: 9px/1.4 var(--mono); color: var(--muted); text-align: right; white-space: nowrap; }
  .failed .name, .failed .result { color: var(--red); }
  .point[aria-current="true"] { background: var(--selected); box-shadow: inset 2px 0 0 var(--link); }
</style>
