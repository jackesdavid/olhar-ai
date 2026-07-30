<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";

  import Busy from "$lib/components/busy.svelte";

  import * as Card from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Separator } from "$lib/components/ui/separator";
  import SentinelLogo from "$lib/components/sentinel-logo.svelte";

  import { login, signup, loadMe, setToken, googleLoginUrl, loginWithPasskey, passkeysSupported,
    loginWithPasskeyDiscoverable,
  } from "$lib/auth.svelte";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()]);

  let mode = $state<"signin" | "signup">("signin");
  let email = $state("");
  let password = $state("");
  let tenantName = $state("");
  let busy = $state(false);

  // OAuth return: the control plane bounces back with `#token=…` in the URL fragment.
  onMount(async () => {
    const hash = window.location.hash.startsWith("#") ? window.location.hash.slice(1) : "";
    const token = new URLSearchParams(hash).get("token");
    if (token) {
      setToken(token);
      history.replaceState(null, "", window.location.pathname);
      if (await loadMe()) {
        await goto("/");
      } else {
        toast.error(t.login.oauthFailed);
        setToken(null);
      }
    }
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    busy = true;
    try {
      if (mode === "signin") {
        await login(email.trim(), password);
      } else {
        await signup(tenantName.trim(), email.trim(), password);
      }
      await goto("/");
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t.login.genericError);
    } finally {
      busy = false;
    }
  }

  async function passkey() {
    if (busy) return;
    busy = true;
    try {
      // No address typed → let the browser offer whatever passkeys it holds for this site. With an
      // address, keep the targeted flow: it still works for a credential the authenticator chose
      // not to store as discoverable, which would never appear in the picker.
      if (email.trim().length === 0) {
        await loginWithPasskeyDiscoverable();
        await goto("/");
        return;
      }
      const ok = await loginWithPasskey(email.trim());
      if (!ok) {
        toast.error(t.webauthn.noPasskey);
        return;
      }
      await goto("/");
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t.webauthn.failed);
    } finally {
      busy = false;
    }
  }

  const canSubmit = $derived(
    mode === "signin"
      ? email.trim().length > 0 && password.length > 0
      : tenantName.trim().length > 0 && email.trim().length > 0 && password.length >= 8,
  );
</script>

<div class="flex min-h-[100dvh] w-screen items-center justify-center p-6">
  <Card.Root class="w-full max-w-sm">
    <Card.Header class="items-center text-center">
      <SentinelLogo stacked size={52} />
      <Card.Description class="mt-2">
        {mode === "signin" ? t.login.signinDesc : t.login.signupDesc}
      </Card.Description>
    </Card.Header>

    <Card.Content class="space-y-4">
      <form class="space-y-4" onsubmit={submit}>
        {#if mode === "signup"}
          <div class="space-y-1.5">
            <Label for="tenant">{t.login.workspaceName}</Label>
            <Input id="tenant" bind:value={tenantName} placeholder={t.login.workspacePlaceholder} autocomplete="organization" />
          </div>
        {/if}
        <div class="space-y-1.5">
          <Label for="email">{t.login.email}</Label>
          <Input id="email" type="email" bind:value={email} placeholder="you@example.com" autocomplete="email" />
        </div>
        <div class="space-y-1.5">
          <Label for="password">{t.login.password}</Label>
          <Input
            id="password"
            type="password"
            bind:value={password}
            placeholder={mode === "signup" ? t.login.passwordSignupPlaceholder : "••••••••"}
            autocomplete={mode === "signin" ? "current-password" : "new-password"}
          />
        </div>
        <Button type="submit" class="w-full" disabled={!canSubmit || busy}>
          {#if busy}<Busy />{/if}
          {mode === "signin" ? t.login.signin : t.login.createWorkspace}
        </Button>
      </form>

      <div class="flex items-center gap-3">
        <Separator class="flex-1" />
        <span class="text-xs text-muted-foreground">{t.login.or}</span>
        <Separator class="flex-1" />
      </div>

      <div class="space-y-2">
        {#if mode === "signin" && passkeysSupported()}
          <Button variant="outline" class="w-full" onclick={passkey} disabled={busy}>
            {t.webauthn.passkeyButton}
          </Button>
        {/if}
        <Button variant="outline" class="w-full" onclick={() => (window.location.href = googleLoginUrl())}>
          {t.login.continueGoogle}
        </Button>
        <Button variant="outline" class="w-full" disabled title={t.login.comingSoon}>
          {t.login.continueApple}
        </Button>
      </div>
    </Card.Content>

    <Card.Footer class="justify-center">
      {#if mode === "signin"}
        <button class="text-sm text-muted-foreground hover:text-foreground" onclick={() => (mode = "signup")}>
          {t.login.noWorkspace} <span class="text-primary">{t.login.createOne}</span>
        </button>
      {:else}
        <button class="text-sm text-muted-foreground hover:text-foreground" onclick={() => (mode = "signin")}>
          {t.login.haveAccount} <span class="text-primary">{t.login.signInLink}</span>
        </button>
      {/if}
    </Card.Footer>
  </Card.Root>
</div>
