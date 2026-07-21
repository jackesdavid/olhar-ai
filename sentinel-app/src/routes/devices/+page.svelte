<script lang="ts">
  // Devices moved under Settings (/settings/devices). Keep this path working for old
  // links/bookmarks by redirecting.
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  onMount(() => goto("/settings/devices", { replaceState: true }));
</script>
