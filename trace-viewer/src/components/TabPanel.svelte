<script lang="ts">
  import type { Snippet } from "svelte";
  let { id, active, padding = "none", children }: {
    id: string; active: string; padding?: "none" | "metadata" | "details"; children: Snippet;
  } = $props();
</script>

<div class="tab-panel" id={`${id}-panel`} role="tabpanel" tabindex="0" aria-labelledby={`${id}-tab`} hidden={active !== id}
  class:metadata={padding === "metadata"} class:details={padding === "details"}>
  {@render children()}
</div>

<style>
  .tab-panel { flex: 1; overflow: auto; min-height: 0; }
  .metadata { padding: 0 8px 8px; }
  .details { padding: 8px 10px; }
  @media (max-width: 600px) { .details { padding: 6px; } }
</style>
