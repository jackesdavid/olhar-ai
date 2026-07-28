<script lang="ts">
  import ChevronDown from "@lucide/svelte/icons/chevron-down";

  let {
    value = $bindable(""),
    options,
    disabled = false,
    placeholder,
    onchange,
  }: {
    value?: string;
    options: string[];
    disabled?: boolean;
    placeholder?: string;
    onchange?: (v: string) => void;
  } = $props();

  let open = $state(false);
  let search = $state("");
  let containerEl: HTMLDivElement | undefined = $state();
  let searchEl: HTMLInputElement | undefined = $state();

  const filtered = $derived(
    search.trim()
      ? options.filter((o) => o.toLowerCase().includes(search.toLowerCase()))
      : options,
  );

  function pick(v: string) {
    value = v;
    onchange?.(v);
    open = false;
    search = "";
  }

  function toggle() {
    if (disabled) return;
    open = !open;
    if (open) search = "";
  }

  $effect(() => {
    if (open && searchEl) searchEl.focus();
  });

  $effect(() => {
    if (!open) return;
    function onOutside(e: MouseEvent) {
      if (containerEl && !containerEl.contains(e.target as Node)) {
        open = false;
        search = "";
      }
    }
    document.addEventListener("mousedown", onOutside, true);
    return () => document.removeEventListener("mousedown", onOutside, true);
  });
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].workflowEditor);
</script>

<div bind:this={containerEl} class="relative">
  <button
    type="button"
    {disabled}
    onclick={toggle}
    class="flex h-8 w-full items-center justify-between rounded-md border border-input bg-background px-3 text-sm capitalize shadow-sm ring-offset-background transition-colors hover:bg-accent focus:outline-none focus:ring-1 focus:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
  >
    <span class="truncate">{value || placeholder || t.selectPlaceholder}</span>
    <ChevronDown class="ml-1 size-3.5 shrink-0 opacity-50" />
  </button>

  {#if open}
    <div
      class="absolute left-0 right-0 top-full z-50 mt-1 overflow-hidden rounded-md border border-border bg-popover text-popover-foreground shadow-md"
    >
      <!-- search row -->
      <div class="border-b border-border px-2 py-1.5">
        <input
          bind:this={searchEl}
          bind:value={search}
          type="text"
          placeholder={t.search}
          class="h-6 w-full bg-transparent text-sm outline-none placeholder:text-muted-foreground"
        />
      </div>
      <!-- scrollable list -->
      <div class="max-h-52 overflow-y-auto">
        {#each filtered as c (c)}
          <button
            type="button"
            onclick={() => pick(c)}
            class="flex w-full items-center px-3 py-1.5 text-left text-sm capitalize {c === value
              ? 'bg-accent font-medium text-primary'
              : 'hover:bg-accent/60'}"
          >
            {c}
          </button>
        {:else}
          <p class="px-3 py-2 text-sm text-muted-foreground">No results</p>
        {/each}
      </div>
    </div>
  {/if}
</div>
