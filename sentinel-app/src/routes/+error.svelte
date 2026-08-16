<script lang="ts">
  // There was no error page anywhere, so a bad URL and a failed load rendered the SAME blank
  // screen — and with adapter-static's index.html fallback, so did every route that does not
  // exist. A blank page teaches nothing and looks like the app died.
  //
  // The 404/5xx split is the point. Telling someone "page not found" when the server actually
  // failed sends them hunting for a mistake they did not make.
  import { page } from "$app/stores";
  import { Button } from "$lib/components/ui/button";
  import SentinelLogo from "$lib/components/sentinel-logo.svelte";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].errorPage);
  const status = $derived($page.status);
  const isMissing = $derived(status === 404);
</script>

<svelte:head><title>{isMissing ? t.notFoundTitle : t.faultTitle}</title></svelte:head>

<div class="flex min-h-screen items-center justify-center p-6">
  <div class="w-full max-w-md space-y-4 text-center">
    <div class="flex justify-center"><SentinelLogo /></div>
    <h1 class="text-xl font-semibold">{isMissing ? t.notFoundTitle : t.faultTitle}</h1>
    <p class="text-sm text-muted-foreground">{isMissing ? t.notFoundBody : t.faultBody}</p>
    <!-- The raw message only when there is one and it is ours to show; a 404 has nothing useful. -->
    {#if !isMissing && $page.error?.message}
      <p class="text-xs text-muted-foreground">{$page.error.message}</p>
    {/if}
    <div class="flex justify-center gap-2">
      <Button href="/">{t.goHome}</Button>
      {#if !isMissing}
        <Button variant="outline" onclick={() => location.reload()}>{t.retry}</Button>
      {/if}
    </div>
  </div>
</div>
