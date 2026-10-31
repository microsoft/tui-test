<script lang="ts">
  let { id, locator }: { id: string; locator?: string } = $props();
</script>

<div id={`${id}-section`} hidden={!locator}>
  <h2>Locator</h2><pre {id} class="locator-expression">{locator ?? ""}</pre>
</div>

<style>
  .locator-expression { padding: 10px 12px; border: 1px solid var(--border); background: var(--panel); color: var(--link); font: 12px/1.6 var(--mono); }
</style>
