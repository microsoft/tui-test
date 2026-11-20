<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import type { TraceModel, Resources, ViewerEvent, ViewerState } from "./types.js";
  import { initialState, update, PLAYBACK_INTERVAL } from "./state.js";
  import { selection, visibleActions, inspectCell } from "./model.js";
  import Header from "./components/Header.svelte";
  import Timeline from "./components/Timeline.svelte";
  import SideNav from "./components/SideNav.svelte";
  import BottomNav from "./components/BottomNav.svelte";
  import TerminalViewer from "./components/TerminalViewer.svelte";
  import ActionsTab from "./components/ActionsTab.svelte";
  import MetadataTab from "./components/MetadataTab.svelte";
  import ResultTab from "./components/ResultTab.svelte";
  import CallTab from "./components/CallTab.svelte";
  import CellTab from "./components/CellTab.svelte";
  import AttachmentsTab from "./components/AttachmentsTab.svelte";

  let { model, resources, hash = "" }: { model: TraceModel; resources: Resources; hash?: string } = $props();
  let view = $state.raw<ViewerState>(untrack(() => initialState(model, hash)));
  let detailsHeight: number | undefined = $state();
  const selected = $derived(selection(model, view));
  const actions = $derived(visibleActions(model, view));
  const inspection = $derived(inspectCell(model, view));
  const playback = $derived(view.playback);

  function send(event: ViewerEvent) {
    view = update(model, view, event);
  }

  $effect(() => {
    if (!playback) return;
    const timer = window.setInterval(() => send({ type: "tick" }), PLAYBACK_INTERVAL);
    return () => window.clearInterval(timer);
  });

  function pagehide(event: PageTransitionEvent) {
    send({ type: "pause" });
    if (!event.persisted) resources.dispose();
  }

  onDestroy(() => resources.dispose());
</script>

<svelte:head><title>{model.title}</title></svelte:head>
<svelte:window onpagehide={pagehide} />
<svelte:document onvisibilitychange={() => { if (document.hidden) send({ type: "pause" }); }} />

<Header summary={model.summary} hasFailure={model.hasFailure} canJump={model.failureFrameIndex !== undefined}
  onjump={() => send({ type: "jump-to-result" })} />

<Timeline duration={model.duration} actions={model.actions} frames={model.frames} images={resources.images}
  selectedAction={view.actionId} selectedFrame={view.frameIndex} failedImages={view.failedImages}
  onaction={(id) => send({ type: "select-action", id })}
  onframe={(index) => send({ type: "select-frame", index })}
  onimageerror={(index) => send({ type: "image-failed", index })} />

<main style:--details-height={detailsHeight === undefined ? undefined : `${detailsHeight}px`}>
  <SideNav active={view.sidebarTab} onselect={(tab) => send({ type: "select-tab", tab })}>
    {#snippet actionsPanel()}
      <ActionsTab {actions} selected={view.actionId} filter={view.filter} assertionsOnly={view.assertionsOnly}
        onselect={(id) => send({ type: "select-action", id })}
        onfilter={(value) => send({ type: "filter", value })}
        onassertions={(value) => send({ type: "assertions-only", value })} />
    {/snippet}
    {#snippet metadataPanel()}
      <MetadataTab metadata={model.metadata} />
    {/snippet}
  </SideNav>

  <TerminalViewer frame={selected.frame} action={selected.action} missingScreen={view.missingScreen}
    sharesFailure={selected.sharesFailure} isFailure={selected.isFailure} result={model.result}
    {actions} frameCount={model.frames.length} zoom={view.zoom} playing={!!playback}
    image={selected.frame && resources.images[selected.frame.index]}
    imageFailed={!!selected.frame && view.failedImages.includes(selected.frame.index)}
    geometry={model.geometry} mismatches={selected.mismatches} cell={view.cell}
    onaction={(delta) => send({ type: "navigate-action", delta })}
    onframe={(index) => send({ type: "select-frame", index })}
    onplay={() => send({ type: "toggle-playback" })}
    onzoom={(value) => send({ type: "zoom", value })}
    oninspect={(x, y) => send({ type: "inspect", x, y, activate: true })}
    onmove={(dx, dy) => send({ type: "move-cell", dx, dy })}
    onimageerror={(index) => send({ type: "image-failed", index })} />

  <BottomNav active={view.detailsTab} hasFailure={model.hasFailure} onselect={(tab) => send({ type: "select-tab", tab })}
    onresize={(height) => { detailsHeight = height; }}>
    {#snippet resultPanel()}
      <ResultTab result={model.result} hasFailure={model.hasFailure} />
    {/snippet}
    {#snippet cellPanel()}
      <CellTab {inspection} frame={selected.frame} column={view.column} row={view.row}
        oncoordinate={(axis, value) => send({ type: "coordinate", axis, value })}
        oninspect={() => send({ type: "inspect", x: view.column === "" ? NaN : Number(view.column),
          y: view.row === "" ? NaN : Number(view.row), activate: false })} />
    {/snippet}
    {#snippet callPanel()}
      <CallTab action={selected.action} frame={selected.frame} missingScreen={view.missingScreen} />
    {/snippet}
    {#snippet attachmentsPanel()}
      <AttachmentsTab files={model.attachments} urls={resources.attachments} unavailableFiles={model.unavailableFiles}
        recordingNote={model.recordingNote} selected={view.attachmentId} failedImages={view.failedAttachments}
        onselect={(id) => send({ type: "preview-attachment", id })}
        onimageerror={(id) => send({ type: "attachment-image-failed", id })} />
    {/snippet}
  </BottomNav>
</main>

<style>
  main { flex: 1; min-height: 0; display: grid; grid-template-columns: clamp(260px, 27vw, 420px) minmax(0, 1fr); grid-template-rows: minmax(220px, 1fr) var(--details-height, clamp(200px, 28vh, 300px)); border: 1px solid var(--border); border-top: 0; background: var(--bg); }
  @media (max-width: 850px) { main { grid-template-columns: 180px minmax(0, 1fr); } }
  @media (max-height: 650px) and (min-width: 601px) { main { flex: none; height: 600px; grid-template-rows: minmax(220px, 1fr) var(--details-height, 240px); } }
  @media (max-width: 600px) { main { flex: none; height: 850px; grid-template-columns: minmax(0, 1fr); grid-template-rows: 160px minmax(260px, 1fr) var(--details-height, 260px); } }
</style>
