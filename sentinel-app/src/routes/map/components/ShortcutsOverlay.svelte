<script lang="ts">
  // Keyboard-shortcut cheat sheet for the Map editor. Toggled by the "?" key or the
  // help button; a floating card over the canvas.
  import X from "@lucide/svelte/icons/x";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].map);

  let { open = false, onClose }: { open?: boolean; onClose: () => void } = $props();

  const GROUPS: { title: string; rows: [string, string][] }[] = $derived([
    {
      title: t.groupTools,
      rows: [
        ["V", t.scSelect],
        ["W", t.scWall],
        ["M", t.scMarker],
        ["F", t.scFit],
        ["`", t.scPanel],
      ],
    },
    {
      title: t.groupEdit,
      rows: [
        ["Arrows", t.scNudge],
        ["Shift + Arrows", t.scGrow],
        ["Shift + Alt + Arrows", t.scShrink],
        ["[  /  ]", t.scRotate],
        ["C  /  ⇧C", t.scCycle],
        ["Delete", t.scDelete],
        ["Esc", t.scDeselect],
      ],
    },
    {
      title: t.groupHistory,
      rows: [
        ["⌘Z / Ctrl+Z", t.scUndo],
        ["⇧⌘Z", t.scRedo],
        ["?", t.scToggleHelp],
      ],
    },
  ]);
</script>

{#if open}
  <div class="absolute inset-0 z-20 flex items-center justify-center bg-overlay p-4 backdrop-blur-sm">
    <div class="w-full max-w-md rounded-xl border border-border bg-card p-4 shadow-2xl">
      <div class="mb-3 flex items-center justify-between">
        <h2 class="text-sm font-semibold">{t.shortcutsTitle}</h2>
        <button class="interactive rounded-md p-1 text-muted-foreground hover:bg-hover hover:text-foreground" title={t.close} onclick={onClose}>
          <X class="size-4" />
        </button>
      </div>
      <div class="space-y-4">
        {#each GROUPS as g (g.title)}
          <div>
            <p class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{g.title}</p>
            <dl class="space-y-1">
              {#each g.rows as [keys, desc] (keys)}
                <div class="flex items-center justify-between gap-3 text-sm">
                  <dt class="text-muted-foreground">{desc}</dt>
                  <dd><kbd class="rounded border border-border bg-muted px-1.5 py-0.5 text-[11px] font-medium tabular-nums">{keys}</kbd></dd>
                </div>
              {/each}
            </dl>
          </div>
        {/each}
      </div>
    </div>
  </div>
{/if}
