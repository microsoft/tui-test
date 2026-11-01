<script lang="ts">
  import type { Properties } from "../types.js";
  let { id, values, colors = {} }: { id: string; values: Properties; colors?: Readonly<Record<string, string>> } = $props();
</script>

<dl {id}>
  {#each Object.entries(values) as [key, value] (key)}
    <dt>{key}</dt>
    <dd>{#if /^#[0-9a-f]{6}$/i.test(colors[key] ?? "")}<span class="swatch" style:background-color={colors[key]}></span>{/if}{value ?? "Not captured"}</dd>
  {/each}
</dl>

<style>
  dl { display: grid; grid-template-columns: minmax(90px, min(30%, 150px)) minmax(0, 1fr); gap: 3px 8px; font-size: 11px; margin: 6px 0; }
  dt { color: var(--muted); }
  dd { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; font-family: var(--mono); }
  .swatch { display: inline-block; width: 9px; height: 9px; border: 1px solid var(--border); margin-right: 4px; vertical-align: middle; }
</style>
