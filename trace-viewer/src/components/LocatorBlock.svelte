<script lang="ts">
  let { id, locator }: { id: string; locator?: string } = $props();
</script>

<div id={`${id}-section`} hidden={!locator}>
  <h2>Locator</h2><pre {id} class="locator-expression">{locator ?? ""}</pre>
</div>

<style>
  .locator-expression { padding: 8px 10px; border: 1px solid var(--border); background: var(--panel); color: #887120; font: 12px/1.5 Consolas, monospace; }
  @media (prefers-color-scheme: dark) { .locator-expression { color: #d5bf75; } }
</style>
