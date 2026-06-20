<script lang="ts">
  import { T } from "@threlte/core";
  import { Text } from "@threlte/extras";
  import type { MapStore } from "$lib/map/store.svelte";
  import { ACCENT_BRIGHT } from "$lib/map/palette";

  let { store }: { store: MapStore } = $props();
</script>

{#each store.texts as t (t.id)}
  {@const selected = t.id === store.selectedTextId}
  {@const col = selected ? ACCENT_BRIGHT : (t.color ?? "#ffffff")}
  {@const rotRad = ((t.rot ?? 0) * Math.PI) / 180}
  <T.Group position={[t.x, 0.06, t.y]} rotation.x={-Math.PI / 2} rotation.z={rotRad}>
    <Text
      text={t.text}
      fontSize={t.size ?? 0.5}
      color={col}
      anchorX="center"
      anchorY="middle"
      depthOffset={-1}
    />
  </T.Group>
{/each}
