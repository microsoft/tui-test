<script lang="ts">
  import type { Snippet } from "svelte";
  import type { SidebarTab } from "../types.js";
  import TabBar from "./TabBar.svelte";
  import TabPanel from "./TabPanel.svelte";
  let { active, onselect, actionsPanel, metadataPanel }: {
    active: SidebarTab; onselect: (tab: SidebarTab) => void; actionsPanel: Snippet; metadataPanel: Snippet;
  } = $props();
  const tabs: readonly { id: SidebarTab; label: string }[] = [{ id: "actions", label: "Actions" }, { id: "metadata", label: "Metadata" }];
</script>

<aside class="sidebar">
  <TabBar {tabs} {active} {onselect} group="sidebar" label="Trace navigation" />
  <TabPanel id="actions" {active}>{@render actionsPanel()}</TabPanel>
  <TabPanel id="metadata" {active} padding="metadata">{@render metadataPanel()}</TabPanel>
</aside>

<style>
  .sidebar { min-height: 0; min-width: 0; border-right: 1px solid var(--border); display: flex; flex-direction: column; }
</style>
