<script lang="ts">
  // Public — reachable without a session (see PUBLIC_PATHS in +layout.svelte). Signup links here
  // before an account exists, so an auth guard on this route would make consent impossible to read.
  import LegalDocument from "$lib/components/legal-document.svelte";
  import { terms } from "$lib/legal/documents";
</script>

<svelte:head><title>Olhar AI — Terms of Service</title></svelte:head>

<LegalDocument doc={terms} />
