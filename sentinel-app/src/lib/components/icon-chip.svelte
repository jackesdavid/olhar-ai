<script lang="ts">
  // The canonical icon "chip": a tinted rounded square with an inset ring holding a
  // single icon. One treatment for every leading icon (page headers, stat tiles,
  // list rows) so they all read the same. Caller sizes the icon inside (e.g.
  // `<Icon class="size-5" />`); the chip sizes its box.
  import type { Snippet } from "svelte";
  let {
    size = "md",
    class: klass = "",
    children,
  }: { size?: "sm" | "md" | "lg"; class?: string; children: Snippet } = $props();

  const box: Record<string, string> = {
    sm: "size-8 rounded-md",
    md: "size-10 rounded-lg",
    lg: "size-12 rounded-xl",
  };
</script>

<div
  class="flex shrink-0 items-center justify-center bg-primary/10 text-primary ring-1 ring-inset ring-primary/20 {box[
    size
  ]} {klass}"
>
  {@render children()}
</div>
