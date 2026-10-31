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
  <div class="timeline-heading"><span>Session timeline</span><span>{frames.length} retained frames / {actions.length} actions</span></div>
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
  .timeline { height: 138px; padding: 8px 16px; flex-shrink: 0; border: 1px solid var(--border); background: var(--bg); overflow: hidden; }
  .timeline-heading { display: flex; justify-content: space-between; gap: 12px; color: var(--muted); font: 10px/1.6 var(--mono); }
  .timeline-heading span:first-child { color: var(--text); letter-spacing: .07em; text-transform: uppercase; }
  .time-scale { height: 22px; display: flex; justify-content: space-between; color: var(--muted); font: 10px/22px var(--mono); }
  .action-timeline { height: 23px; position: relative; border-top: 1px solid var(--border); background: repeating-linear-gradient(to right, transparent 0, transparent calc(20% - 1px), var(--border) 20%); }
  .action-bar { position: absolute; top: 6px; height: 8px; min-width: 4px; border: 0; padding: 0; border-radius: 0; background: var(--link); }
  .action-bar.failed { background: var(--red); }
  .action-bar:hover:not(:disabled) { background: var(--accent); }
  .action-bar.failed:hover:not(:disabled) { background: var(--red); }
  .action-bar[aria-current="true"] { outline: 1px solid var(--accent); outline-offset: 2px; }
  .filmstrip { display: flex; align-items: stretch; gap: 8px; height: 60px; overflow-x: auto; padding: 3px; margin: 0 -3px; }
  .thumbnail { padding: 3px; min-width: 104px; max-width: 140px; flex-shrink: 0; text-align: left; border: 1px solid var(--border); background: var(--panel); }
  .thumbnail img { width: 100%; height: 30px; object-fit: contain; background: #111; display: block; }
  .thumbnail span { display: block; padding: 3px 4px; font: 10px/1.6 var(--mono); color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .thumbnail[aria-current="true"] { border-color: var(--link); background: var(--selected); }
  .thumbnail[aria-current="true"] span { color: var(--link); }
  @media (max-width: 600px) { .timeline { height: 142px; padding: 8px 10px; } .timeline-heading { font-size: 9px; } .filmstrip { height: 66px; } .thumbnail img { height: 38px; } }
</style>
