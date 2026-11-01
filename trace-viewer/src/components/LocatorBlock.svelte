<script lang="ts">
  let { id, locator }: { id: string; locator?: string } = $props();
</script>

<div id={`${id}-section`} hidden={!locator}>
  <h2>Locator</h2><pre {id} class="locator-expression">{locator ?? ""}</pre>
</div>

<style>
  .locator-expression { padding: 6px 8px; border: 1px solid var(--border); background: var(--panel); color: var(--link); font: 11px/1.4 var(--mono); }
</style>
