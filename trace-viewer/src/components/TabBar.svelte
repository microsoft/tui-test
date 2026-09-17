<script lang="ts" generics="T extends string">
  let { tabs, active, label, group, onselect }: {
    tabs: readonly { id: T; label: string }[]; active: T; label: string; group: string; onselect: (tab: T) => void;
  } = $props();
  let buttons: HTMLButtonElement[] = [];

  function navigate(event: KeyboardEvent, index: number) {
    const offsets: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1, Home: -index, End: tabs.length - 1 - index };
    if (offsets[event.key] === undefined) return;
    event.preventDefault();
    const next = (index + offsets[event.key] + tabs.length) % tabs.length;
    onselect(tabs[next].id);
    buttons[next]?.focus();
  }
</script>

<div class="tabs" role="tablist" aria-label={label}>
  {#each tabs as tab, index (tab.id)}
    <button bind:this={buttons[index]} role="tab" id={`${tab.id}-tab`} data-group={group}
      aria-controls={`${tab.id}-panel`} aria-selected={active === tab.id} tabindex={active === tab.id ? 0 : -1}
      onclick={() => onselect(tab.id)} onkeydown={(event) => navigate(event, index)}>
      {tab.label}
    </button>
  {/each}
</div>

<style>
  .tabs { display: flex; border-bottom: 1px solid var(--border); background: var(--panel); flex-shrink: 0; }
  button { border: 0; border-radius: 0; padding: 7px 12px; background: transparent; color: var(--muted); border-bottom: 2px solid transparent; white-space: nowrap; }
  button[aria-selected="true"] { color: var(--text); border-bottom-color: var(--blue); }
</style>
