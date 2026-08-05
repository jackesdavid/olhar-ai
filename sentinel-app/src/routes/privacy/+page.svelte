<script lang="ts">
  // Public — see the note in /terms.
  import LegalDocument from "$lib/components/legal-document.svelte";
  import { privacy } from "$lib/legal/documents";
</script>

<svelte:head><title>Olhar AI — Privacy Policy</title></svelte:head>

<LegalDocument doc={privacy} />
