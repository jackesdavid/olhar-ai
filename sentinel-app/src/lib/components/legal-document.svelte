<script lang="ts">
  // Renders a published legal document (#167). Shared by /terms and /privacy so the two pages
  // cannot drift in layout or in how they present the version — the version line is the thing a
  // user matches against the acceptance recorded on their account.
  import { lang } from "$lib/i18n";
  import SentinelLogo from "$lib/components/sentinel-logo.svelte";
  import type { Document } from "$lib/legal/documents";

  let { doc }: { doc: { en: Document; pt: Document } } = $props();

  const d = $derived(doc[lang()]);
  const updatedLabel = $derived(
    lang() === "pt" ? "Última atualização" : "Last updated",
  );
</script>

<div class="mx-auto w-full max-w-3xl px-6 py-12">
  <a href="/" class="mb-10 inline-flex" aria-label="Olhar AI">
    <SentinelLogo />
  </a>

  <h1 class="text-3xl font-semibold tracking-tight">{d.title}</h1>
  <p class="mt-2 text-sm text-muted-foreground">{updatedLabel}: {d.updated}</p>
  <p class="mt-6 leading-relaxed text-muted-foreground">{d.intro}</p>

  <div class="mt-10 space-y-10">
    {#each d.sections as section (section.heading)}
      <section>
        <h2 class="text-lg font-medium tracking-tight">{section.heading}</h2>
        <div class="mt-3 space-y-3">
          {#each section.body as paragraph}
            <p class="leading-relaxed text-muted-foreground">{paragraph}</p>
          {/each}
        </div>
      </section>
    {/each}
  </div>
</div>
