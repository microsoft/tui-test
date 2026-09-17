<script lang="ts">
  import type { Snippet } from "svelte";
  import type { DetailsTab } from "../types.js";
  import TabBar from "./TabBar.svelte";
  import TabPanel from "./TabPanel.svelte";
  let { active, hasFailure, onselect, resultPanel, cellPanel, callPanel, attachmentsPanel }: {
    active: DetailsTab; hasFailure: boolean; onselect: (tab: DetailsTab) => void;
    resultPanel: Snippet; cellPanel: Snippet; callPanel: Snippet; attachmentsPanel: Snippet;
  } = $props();
  const tabs: readonly { id: DetailsTab; label: string }[] = $derived([
    { id: "errors", label: hasFailure ? "Error" : "Result" }, { id: "cell", label: "Cell" },
    { id: "call", label: "Call" }, { id: "attachments", label: "Attachments" },
  ]);
</script>

<section class="details-panel" aria-label="Action details">
  <TabBar {tabs} {active} {onselect} group="details" label="Action details" />
  <TabPanel id="errors" {active} padding="details">{@render resultPanel()}</TabPanel>
  <TabPanel id="cell" {active} padding="details">{@render cellPanel()}</TabPanel>
  <TabPanel id="call" {active} padding="details">{@render callPanel()}</TabPanel>
  <TabPanel id="attachments" {active} padding="details">{@render attachmentsPanel()}</TabPanel>
</section>

<style>
  .details-panel { grid-column: 1 / -1; min-height: 0; border-top: 1px solid var(--border); display: flex; flex-direction: column; }
</style>
