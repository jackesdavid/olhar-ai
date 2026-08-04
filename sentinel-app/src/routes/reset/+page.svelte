<script lang="ts">
  // Card #166 — the page the emailed reset link opens. Its own route rather than a mode on /login,
  // because it is entered from outside the app carrying a token, and it must render sensibly for
  // someone who is not signed in and may not remember asking.
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
  import { confirmPasswordReset } from "$lib/auth.svelte";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()]);

  let token = $state("");
  let password = $state("");
  let confirm = $state("");
  let busy = $state(false);
  let done = $state(false);

  onMount(() => {
    token = $page.url.searchParams.get("t") ?? "";
  });

  // House style: named helpers, not logic in the markup.
  function passwordTooShort(): boolean {
    return password.length > 0 && password.length < 8;
  }
  function mismatched(): boolean {
    return confirm.length > 0 && password !== confirm;
  }
  function canSubmit(): boolean {
    return (
      token.length > 0 && password.length >= 8 && password === confirm && !busy
    );
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!canSubmit()) return;
    busy = true;
    try {
      await confirmPasswordReset(token, password);
      done = true;
      // Every session was revoked server-side, so there is nothing to return to but a fresh login.
      setTimeout(() => goto("/login"), 2500);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t.reset.genericError);
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex min-h-[100dvh] w-screen items-center justify-center p-6">
  <Card.Root class="w-full max-w-sm">
    <Card.Header class="items-center text-center">
      <SentinelLogo stacked size={52} />
      <Card.Description class="mt-2">{t.reset.description}</Card.Description>
    </Card.Header>

    <Card.Content class="space-y-4">
      {#if done}
        <div class="space-y-3 text-center">
          <p class="text-sm font-medium">{t.reset.doneTitle}</p>
          <p class="text-sm text-muted-foreground">{t.reset.doneBody}</p>
          <Button class="w-full" onclick={() => goto("/login")}>{t.reset.goSignIn}</Button>
        </div>
      {:else if token.length === 0}
        <!-- Opened without a token: a dead end unless we say what to do instead. -->
        <div class="space-y-3 text-center">
          <p class="text-sm font-medium">{t.reset.noTokenTitle}</p>
          <p class="text-sm text-muted-foreground">{t.reset.noTokenBody}</p>
          <Button variant="outline" class="w-full" onclick={() => goto("/login")}>
            {t.reset.goSignIn}
          </Button>
        </div>
      {:else}
        <form class="space-y-4" onsubmit={submit}>
          <div class="space-y-1.5">
            <Label for="password">{t.reset.newPassword}</Label>
            <Input
              id="password"
              type="password"
              bind:value={password}
              placeholder={t.reset.atLeast8}
              autocomplete="new-password"
            />
            {#if passwordTooShort()}
              <p class="text-xs text-destructive">{t.reset.tooShort}</p>
            {/if}
          </div>
          <div class="space-y-1.5">
            <Label for="confirm">{t.reset.confirmPassword}</Label>
            <Input
              id="confirm"
              type="password"
              bind:value={confirm}
              placeholder="••••••••"
              autocomplete="new-password"
            />
            {#if mismatched()}
              <p class="text-xs text-destructive">{t.reset.mismatch}</p>
            {/if}
          </div>
          <Button type="submit" class="w-full" disabled={!canSubmit()}>
            {#if busy}<Busy />{/if}
            {t.reset.submit}
          </Button>
        </form>
      {/if}
    </Card.Content>
  </Card.Root>
</div>
