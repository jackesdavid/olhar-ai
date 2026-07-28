<script lang="ts">
  // Click-to-draw a reusable line/zone (card: edge tracking) on a live camera snapshot, for the
  // "Region" node's `points` config. 2 points = a crossing line, 3+ = a polygon zone — which it
  // means is decided by whichever `On event` reads it (see docs/ref/workflows.md's "Gated
  // events"), not by this editor. Points are stored normalized 0..1 (origin top-left) so the
  // same config works whatever resolution the camera actually produces.
  import { Button } from "$lib/components/ui/button";
  import { data } from "$lib/data";

  let {
    cameraIp,
    points,
    disabled = false,
    onchange,
  }: {
    /** The resolved device IP (already looked up by the caller from config.camera) — empty
     * string when no camera is picked yet. */
    cameraIp: string;
    points: [number, number][];
    disabled?: boolean;
    onchange: (points: [number, number][]) => void;
  } = $props();

  let imgEl: HTMLImageElement | undefined = $state();
  let imgSrc = $state<string | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let objectUrl: string | null = null;

  async function loadSnapshot() {
    if (!cameraIp) {
      imgSrc = null;
      return;
    }
    loading = true;
    error = null;
    try {
      const blob = await data.snapshotBlob(cameraIp);
      if (objectUrl) URL.revokeObjectURL(objectUrl);
      objectUrl = URL.createObjectURL(blob);
      imgSrc = objectUrl;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void cameraIp; // re-fetch whenever the resolved camera changes
    loadSnapshot();
    return () => {
      if (objectUrl) {
        URL.revokeObjectURL(objectUrl);
        objectUrl = null;
      }
    };
  });

  function addPoint(e: MouseEvent) {
    if (disabled || !imgEl) return;
    const r = imgEl.getBoundingClientRect();
    const x = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    const y = Math.min(1, Math.max(0, (e.clientY - r.top) / r.height));
    onchange([...points, [Number(x.toFixed(4)), Number(y.toFixed(4))]]);
  }
  function undoLast() {
    onchange(points.slice(0, -1));
  }
  function clearAll() {
    onchange([]);
  }

  const shapeLabel = $derived(describeShape(points.length));

  /** House style: no nested ternaries — name the shape the current points make. */
  function describeShape(n: number): string {
    if (n === 0) return t.shapeNone;
    if (n === 1) return t.shapeOnePoint;
    if (n === 2) return t.shapeLine;
    return t.shapeZone(n);
  }
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].workflowEditor);
</script>

<div class="space-y-2">
  {#if !cameraIp}
    <p class="text-xs text-muted-foreground">{t.pickCameraFirst}</p>
  {:else if loading}
    <p class="text-xs text-muted-foreground">{t.loadingSnapshot}</p>
  {:else if error}
    <p class="text-xs text-destructive">{error}</p>
    <Button variant="outline" size="sm" onclick={loadSnapshot}>{t.retry}</Button>
  {:else if imgSrc}
    <div class="relative overflow-hidden rounded-md border border-border bg-video-well" style="aspect-ratio: 16/9">
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
      <img
        bind:this={imgEl}
        src={imgSrc}
        alt={t.snapshotAlt}
        class="h-full w-full object-contain select-none {disabled ? '' : 'cursor-crosshair'}"
        draggable="false"
        onclick={addPoint}
      />
      <svg class="pointer-events-none absolute inset-0 h-full w-full" viewBox="0 0 100 100" preserveAspectRatio="none">
        {#if points.length >= 3}
          <polygon
            points={points.map(([x, y]) => `${x * 100},${y * 100}`).join(" ")}
            fill="rgba(192,132,252,0.25)"
            stroke="#c084fc"
            stroke-width="0.5"
            vector-effect="non-scaling-stroke"
          />
        {:else if points.length === 2}
          <line
            x1={points[0][0] * 100}
            y1={points[0][1] * 100}
            x2={points[1][0] * 100}
            y2={points[1][1] * 100}
            stroke="#c084fc"
            stroke-width="0.5"
            vector-effect="non-scaling-stroke"
          />
        {/if}
        {#each points as [x, y], i (i)}
          <circle cx={x * 100} cy={y * 100} r="1" fill="#c084fc" stroke="white" stroke-width="0.3" vector-effect="non-scaling-stroke" />
        {/each}
      </svg>
    </div>
    <div class="flex items-center justify-between gap-2">
      <p class="text-[10px] text-muted-foreground">{t.clickToAdd(shapeLabel)}</p>
      <div class="flex gap-1">
        <Button variant="outline" size="sm" disabled={disabled || points.length === 0} onclick={undoLast}>{t.undo}</Button>
        <Button variant="outline" size="sm" disabled={disabled || points.length === 0} onclick={clearAll}>{t.clear}</Button>
        <Button variant="outline" size="sm" onclick={loadSnapshot}>{t.refresh}</Button>
      </div>
    </div>
  {/if}
</div>
