<script lang="ts">
  // The page an invite link opens (#181). Its own route rather than a mode on /login, for the same
  // reason /reset is: it is entered from OUTSIDE the app carrying a token, by someone who has no
  // account yet and may never have seen this product.
  //
  // It asks which organisation before it asks for anything else. "Join Acme?" is the one piece of
  // context that makes a stranger's link safe to act on, and it is the only thing the public
  // preview endpoint returns.
  import { goto } from "$app/navigation";
  import { page } from "$app/stores";
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";

  import { Button } from "$lib/components/ui/button";
  import * as Card from "$lib/components/ui/card";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import Busy from "$lib/components/busy.svelte";
  import SentinelLogo from "$lib/components/sentinel-logo.svelte";
  import { acceptInvite, previewInvite } from "$lib/auth.svelte";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].join);

  const token = $derived($page.params.token ?? "");
  let tenantName = $state<string | null>(null);
  let invitedEmail = $state<string | null>(null);
  let email = $state("");
  let password = $state("");
  let confirm = $state("");
  let loading = $state(true);
  let dead = $state(false);
  let busy = $state(false);

  onMount(async () => {
    try {
      const p = await previewInvite(token);
      tenantName = p.tenant_name;
      invitedEmail = p.email;
      if (p.email) email = p.email;
    } catch {
      // Missing, expired and already-used are one answer on purpose (the endpoint is
      // enumerable), so the page says the one true thing it can: ask for another.
      dead = true;
    } finally {
      loading = false;
    }
  });

  // An invite with no address was handed over in person, so the person supplies their own.
  function ready(): boolean {
    return (
      password.length >= 8 && password === confirm && (!!invitedEmail || email.includes("@"))
    );
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (busy || !ready()) return;
    busy = true;
    try {
      await acceptInvite(token, password, invitedEmail ? undefined : email.trim());
      toast.success(t.welcome(tenantName ?? ""));
      await goto("/");
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t.failed);
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex min-h-screen items-center justify-center p-4">
  <Card.Root class="w-full max-w-md">
    <Card.Header class="items-center space-y-3 text-center">
      <SentinelLogo />
      {#if loading}
        <Card.Title>{t.checking}</Card.Title>
      {:else if dead}
        <Card.Title>{t.deadTitle}</Card.Title>
        <Card.Description>{t.deadBody}</Card.Description>
      {:else}
        <Card.Title>{t.joinTitle(tenantName ?? "")}</Card.Title>
        <Card.Description>{t.joinBody}</Card.Description>
      {/if}
    </Card.Header>

    {#if !loading && !dead}
      <Card.Content>
        <form class="space-y-4" onsubmit={submit}>
          {#if invitedEmail}
            <div class="space-y-1.5">
              <Label>{t.email}</Label>
              <Input value={invitedEmail} readonly />
            </div>
          {:else}
            <div class="space-y-1.5">
              <Label for="join-email">{t.email}</Label>
              <Input id="join-email" type="email" bind:value={email} placeholder={t.emailPlaceholder} />
            </div>
          {/if}
          <div class="space-y-1.5">
            <Label for="join-pw">{t.password}</Label>
            <Input id="join-pw" type="password" bind:value={password} autocomplete="new-password" placeholder={t.passwordPlaceholder} />
          </div>
          <div class="space-y-1.5">
            <Label for="join-pw2">{t.confirm}</Label>
            <Input id="join-pw2" type="password" bind:value={confirm} autocomplete="new-password" />
          </div>
          <Button type="submit" class="w-full" disabled={!ready() || busy}>
            {#if busy}<Busy />{/if}
            {t.join}
          </Button>
        </form>
      </Card.Content>
    {:else if dead}
      <Card.Content>
        <Button variant="outline" class="w-full" onclick={() => goto("/login")}>{t.toLogin}</Button>
      </Card.Content>
    {/if}
  </Card.Root>
</div>
