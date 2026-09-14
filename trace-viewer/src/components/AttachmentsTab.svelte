<script lang="ts">
  import type { TraceAttachment } from "../types.js";
  import AttachmentPreview from "./AttachmentPreview.svelte";
  let { files, urls, unavailableFiles, recordingNote, selected, failedImages, onselect, onimageerror }: {
    files: readonly TraceAttachment[]; urls: readonly string[]; unavailableFiles: readonly string[]; recordingNote: string;
    selected?: number; failedImages: readonly number[]; onselect: (id: number) => void; onimageerror: (id: number) => void;
  } = $props();
  const file = $derived(selected === undefined ? undefined : files[selected]);
</script>

<p class="muted">Every available file below is embedded in this HTML. Preview or download it without the original directory. Captured output is untrusted test data; review it before sharing. The embedded JSON describes the evidence at report generation and excludes the HTML's own hash. A separately exported manifest also includes the final HTML hash.</p>
<div class="attachment-layout">
  <div id="evidence-links">
    {#each files as file (file.id)}
      <div class="attachment">
        <button data-attachment={file.name} onclick={() => onselect(file.id)}>{file.name} ({file.byteLength.toLocaleString("en-US")} B)</button>
        <a href={urls[file.id]} download={file.name} data-download={file.name}>Download</a>
      </div>
    {/each}
    {#each unavailableFiles as message}<p class="muted">{message}</p>{/each}
  </div>
  <AttachmentPreview {file} url={file && urls[file.id]} failed={!!file && failedImages.includes(file.id)} {onimageerror} />
</div>
<p id="recording-note" class="muted">{recordingNote}</p>

<style>
  .attachment-layout { display: grid; grid-template-columns: 300px minmax(0, 1fr); gap: 20px; }
  .attachment { display: flex; align-items: center; justify-content: space-between; gap: 10px; border-bottom: 1px solid var(--border); padding: 6px 0; }
  .attachment button { border: 0; padding-left: 0; color: var(--blue); }
  .attachment a { color: var(--blue); font-size: 11px; }
  @media (max-width: 600px) { .attachment-layout { grid-template-columns: 1fr; } }
</style>
