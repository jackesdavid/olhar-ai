<script lang="ts">
  // The canonical inline "working" indicator: a small Sentinel dot-field mark
  // pinging at a livelier cadence. Use inside buttons/rows/pills in place of a
  // generic spinner while an action is in-flight. Inherits currentColor, so it
  // takes the surrounding text/button ink automatically. Fewer rings than the
  // full mark so it stays legible at button scale.
  import SentinelMark from "./sentinel-mark.svelte";
  let { size = 16, class: klass = "" }: { size?: number; class?: string } = $props();
</script>

<SentinelMark {size} rings={3} speed={1.3} class={klass} />
