<script lang="ts">
  import type { TraceAttachment } from "../types.js";
  let { file, url, failed, onimageerror }: { file?: TraceAttachment; url?: string; failed: boolean; onimageerror: (id: number) => void } = $props();
</script>

<div id="attachment-preview">
  {#if !file}
    <p class="muted">Select an attachment to preview it.</p>
  {:else}
    <h2>{file.name}</h2>
    {#if failed}
      <p class="notice error">Captured SVG could not be displayed. Download retains the original attachment bytes.</p>
    {:else if file.mime === "image/svg+xml"}
      <img src={url} alt={file.name} onerror={() => onimageerror(file.id)} />
    {:else}
      <pre>{file.preview}</pre>
      {#if file.truncated}<p class="muted">Preview limited to 512 KiB. Download contains the complete file.</p>{/if}
    {/if}
  {/if}
</div>

<style>
  #attachment-preview { min-width: 0; }
  img { max-width: 100%; max-height: 300px; }
  pre { padding: 6px 8px; border: 1px solid var(--border); background: var(--panel); }
</style>
