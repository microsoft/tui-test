<script lang="ts">
  import { onMount } from "svelte";
  import type { Snippet } from "svelte";
  import type { DetailsTab } from "../types.js";
  import TabBar from "./TabBar.svelte";
  import TabPanel from "./TabPanel.svelte";
  let { active, hasFailure, onselect, onresize, resultPanel, cellPanel, callPanel, attachmentsPanel }: {
    active: DetailsTab; hasFailure: boolean; onselect: (tab: DetailsTab) => void; onresize: (height: number) => void;
    resultPanel: Snippet; cellPanel: Snippet; callPanel: Snippet; attachmentsPanel: Snippet;
  } = $props();
  const tabs: readonly { id: DetailsTab; label: string }[] = $derived([
    { id: "errors", label: hasFailure ? "Error" : "Result" }, { id: "cell", label: "Cell" },
    { id: "call", label: "Call" }, { id: "attachments", label: "Attachments" },
  ]);
  let panel: HTMLElement | undefined = $state();
  let drag: { id: number; y: number; height: number } | undefined = $state.raw();
  const minimum = 120;
  let height = $state(minimum);
  let maximum = $state(minimum);

  function bounds() {
    const height = panel?.getBoundingClientRect().height ?? minimum;
    const main = panel?.parentElement;
    const narrow = window.matchMedia("(max-width: 600px)").matches;
    const navigation = narrow ? main?.querySelector<HTMLElement>(".sidebar")?.getBoundingClientRect().height ?? 0 : 0;
    return { height, maximum: Math.max(minimum, (main?.clientHeight ?? minimum) - navigation - (narrow ? 260 : 220)) };
  }

  function resize(height: number) {
    onresize(Math.min(bounds().maximum, Math.max(minimum, height)));
  }

  function start(event: PointerEvent & { currentTarget: HTMLDivElement }) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag = { id: event.pointerId, y: event.clientY, height: bounds().height };
  }

  function move(event: PointerEvent) {
    if (drag?.id === event.pointerId) resize(drag.height + drag.y - event.clientY);
  }

  function end(event: PointerEvent & { currentTarget: HTMLDivElement }) {
    if (drag?.id !== event.pointerId) return;
    drag = undefined;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
  }

  function key(event: KeyboardEvent) {
    const current = bounds();
    const values: Record<string, number> = {
      ArrowUp: current.height + 24, ArrowDown: current.height - 24, Home: minimum, End: current.maximum,
    };
    if (values[event.key] === undefined) return;
    event.preventDefault();
    resize(values[event.key]);
  }

  onMount(() => {
    if (!panel?.parentElement) return;
    const observer = new ResizeObserver(() => {
      const current = bounds();
      height = Math.round(current.height);
      maximum = Math.round(current.maximum);
      if (current.height > current.maximum + 1) onresize(current.maximum);
    });
    observer.observe(panel);
    observer.observe(panel.parentElement);
    return () => observer.disconnect();
  });
</script>

<section bind:this={panel} id="action-details" class="details-panel" aria-label="Action details">
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (Focusable separators implement the WAI-ARIA window splitter pattern.) -->
  <div class="resize-handle" class:dragging={!!drag} role="separator" tabindex="0" aria-label="Resize action details"
    aria-orientation="horizontal" aria-controls="action-details" aria-valuemin={minimum} aria-valuemax={maximum}
    aria-valuenow={height} aria-valuetext={`${height} pixels high`}
    onpointerdown={start} onpointermove={move} onpointerup={end} onpointercancel={end}
    onlostpointercapture={() => { drag = undefined; }} onkeydown={key}></div>
  <TabBar {tabs} {active} {onselect} group="details" label="Action details" />
  <TabPanel id="errors" {active} padding="details">{@render resultPanel()}</TabPanel>
  <TabPanel id="cell" {active} padding="details">{@render cellPanel()}</TabPanel>
  <TabPanel id="call" {active} padding="details">{@render callPanel()}</TabPanel>
  <TabPanel id="attachments" {active} padding="details">{@render attachmentsPanel()}</TabPanel>
</section>

<style>
  .details-panel { grid-column: 1 / -1; min-height: 0; border-top: 1px solid var(--border); display: flex; flex-direction: column; }
  .resize-handle { position: relative; height: 7px; flex-shrink: 0; cursor: row-resize; touch-action: none; user-select: none; outline-offset: -2px; }
  .resize-handle::after { content: ""; position: absolute; top: 2px; left: calc(50% - 16px); width: 32px; height: 2px; background: var(--control); }
  .resize-handle:hover, .resize-handle:focus-visible, .resize-handle.dragging { background: var(--selected); }
  .resize-handle:hover::after, .resize-handle:focus-visible::after, .resize-handle.dragging::after { background: var(--accent); }
</style>
