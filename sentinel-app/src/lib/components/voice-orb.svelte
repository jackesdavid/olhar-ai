<script lang="ts">
  // The voice-first centerpiece: the Sentinel hex dot-field mark, audio-reactive.
  // It swells with live audio amplitude (`level` 0..1) and tints by who's talking —
  // primary ink when Sentinel speaks, sky-blue when the operator speaks, dim when idle.
  import SentinelMark from "$lib/components/sentinel-mark.svelte";

  let {
    level = 0,
    state = "idle",
    size = 240,
  }: { level?: number; state?: "idle" | "listening" | "speaking"; size?: number } = $props();

  // Sentinel = primary ink; operator = sky (a distinct speaker signal); idle = dim grey.
  function toneClass(s: string): string {
    if (s === "speaking") return "text-primary";
    if (s === "listening") return "text-sky-400";
    return "text-muted-foreground/40";
  }

  const amp = $derived(Math.min(1, Math.max(0, level)));
  const scale = $derived(1 + amp * 0.32); // swell with amplitude
  const glow = $derived(state === "idle" ? 0.12 : 0.2 + amp * 0.6);
  const ringScale = $derived(1.15 + amp * 0.5);
</script>

<div class="relative grid place-items-center {toneClass(state)}" style="width:{size}px;height:{size}px">
  <!-- soft amplitude halo -->
  <div
    class="pointer-events-none absolute rounded-full blur-2xl"
    style="width:{size}px;height:{size}px;background:radial-gradient(closest-side,currentColor,transparent);opacity:{glow};transform:scale({ringScale})"
  ></div>
  <!-- expanding ring on loud peaks -->
  <div
    class="pointer-events-none absolute rounded-full border"
    style="width:{size * 0.7}px;height:{size * 0.7}px;border-color:currentColor;opacity:{amp * 0.4};transform:scale({ringScale})"
  ></div>
  <!-- the mark itself, scaled by amplitude -->
  <div class="relative" style="transform:scale({scale});will-change:transform">
    <SentinelMark {size} rings={6} animated={state === "idle"} />
  </div>
</div>
