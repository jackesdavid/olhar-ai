<script lang="ts">
  // The canonical inline "working" indicator: the OlharAI eye, pulsing at a working tempo. Use
  // inside buttons/rows/pills in place of a generic spinner while an action is in-flight.
  // Inherits currentColor, so it takes the surrounding text/button ink automatically.
  //
  // Two deliberate departures from the mark's resting state, both because 16px in a button is not
  // a logo lockup:
  //   • `minimal` (automatic under 24px) drops the orbit and moon — at this size their stroke is
  //     sub-pixel and would smear rather than shrink — and crops to the eye, which is the part
  //     that carries the identity.
  //   • `pace` runs the breathe at ~0.85s. BRAND.md's 3.4s is a calm logo; a button spinner has to
  //     look like it's working. The blink is off — at this tempo it reads as a twitch.
  import OlharMark from "./olhar-mark.svelte";
  let { size = 16, class: klass = "" }: { size?: number; class?: string } = $props();
</script>

<OlharMark {size} pace={4} blink={false} class={klass} label="" />
