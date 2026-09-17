<script lang="ts">
  import type { TraceAction, TraceFrame, ImageResult } from "../types.js";
  import { time } from "../format.js";
  let { duration, actions, frames, images, selectedAction, selectedFrame, failedImages, onaction, onframe, onimageerror }: {
    duration: number; actions: readonly TraceAction[]; frames: readonly TraceFrame[]; images: readonly ImageResult[];
    selectedAction?: number; selectedFrame?: number; failedImages: readonly number[];
    onaction: (id: number) => void; onframe: (index: number) => void; onimageerror: (index: number) => void;
  } = $props();
</script>

<section class="timeline" aria-label="Trace timeline">
  <div id="time-scale" class="time-scale" aria-hidden="true">
    {#each [0, 1, 2, 3, 4, 5] as tick}<span>{time(Math.round(duration * tick / 5))}</span>{/each}
  </div>
  <div id="action-timeline" class="action-timeline" aria-label="Operation durations">
    {#each actions as action (action.id)}
      {@const title = `${action.name} ${action.label}: ${time(action.duration)} (${action.result})`}
      <button class="action-bar" class:failed={action.failed} data-operation={action.id} {title}
        aria-label={title} aria-current={selectedAction === action.id} onclick={() => onaction(action.id)}
        style:left={`${Math.min(99.5, action.started / Math.max(1, duration) * 100)}%`}
        style:width={`${Math.max(.4, action.duration / Math.max(1, duration) * 100)}%`}></button>
    {/each}
  </div>
  <nav id="filmstrip" class="filmstrip" aria-label="Retained terminal frames">
    {#each frames as frame (frame.index)}
      <button class="thumbnail" data-frame={frame.index} aria-current={selectedFrame === frame.index}
        aria-label={`Screen ${frame.sequence} at ${time(frame.started)}`} onclick={() => onframe(frame.index)}>
        {#if images[frame.index]?.url && !failedImages.includes(frame.index)}
          <img src={images[frame.index].url} alt="" onerror={() => onimageerror(frame.index)} />
        {/if}
        <span>{frame.label}</span>
      </button>
    {/each}
  </nav>
</section>

<style>
  .timeline { height: 132px; padding: 0 12px 6px; flex-shrink: 0; border-bottom: 1px solid var(--border); background: var(--panel); overflow: hidden; }
  .time-scale { height: 21px; display: flex; justify-content: space-between; color: var(--muted); font: 10px/21px Consolas, monospace; }
  .action-timeline { height: 23px; position: relative; border-top: 1px solid var(--border); background: repeating-linear-gradient(to right, transparent 0, transparent calc(20% - 1px), var(--border) 20%); }
  .action-bar { position: absolute; top: 6px; height: 10px; min-width: 4px; border: 0; padding: 0; border-radius: 2px; background: #7fb0eb; }
  .action-bar.failed { background: #d95c65; }
  .action-bar[aria-current="true"] { outline: 2px solid var(--blue); }
  .filmstrip { display: flex; align-items: stretch; gap: 7px; height: 78px; overflow-x: auto; }
  .thumbnail { padding: 2px; min-width: 92px; max-width: 126px; flex-shrink: 0; text-align: left; border: 1px solid var(--border); background: var(--bg); }
  .thumbnail img { width: 100%; height: 46px; object-fit: contain; background: #111; display: block; }
  .thumbnail span { display: block; padding: 1px 4px; font-size: 10px; color: var(--muted); }
  .thumbnail[aria-current="true"] { border-color: var(--blue); box-shadow: inset 0 0 0 1px var(--blue); }
  @media (max-width: 600px) { .timeline { height: 112px; } .filmstrip { height: 58px; } .thumbnail img { height: 30px; } }
</style>
