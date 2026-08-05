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

  import { login, signup, loadMe, setToken, googleLoginUrl, loginWithPasskey, passkeysSupported, requestPasswordReset,
    loginWithPasskeyDiscoverable,
  } from "$lib/auth.svelte";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()]);

  let mode = $state<"signin" | "signup" | "forgot">("signin");
  // #167 — must be ticked to create a workspace; the API refuses without it.
  let acceptedTerms = $state(false);
  // The confirmation state after asking for a link. Deliberately says nothing about whether
  // the address exists — the UI must not leak what the endpoint refuses to.
  let resetSent = $state(false);
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
      if (mode === "forgot") {
        await requestPasswordReset(email.trim());
        resetSent = true;
        return;
      }
      if (mode === "signin") {
        await login(email.trim(), password);
      } else {
        await signup(tenantName.trim(), email.trim(), password, acceptedTerms);
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

  function canSubmitFor(m: typeof mode): boolean {
    if (m === "forgot") return email.trim().length > 0;
    if (m === "signin") return email.trim().length > 0 && password.length > 0;
    return (
      tenantName.trim().length > 0 &&
      email.trim().length > 0 &&
      password.length >= 8 &&
      acceptedTerms
    );
  }
  const canSubmit = $derived(canSubmitFor(mode));

  function headerDescription(m: typeof mode): string {
    if (m === "forgot") return t.login.forgotDesc;
    if (m === "signin") return t.login.signinDesc;
    return t.login.signupDesc;
  }
  function submitLabel(m: typeof mode): string {
    if (m === "forgot") return t.login.sendResetLink;
    if (m === "signin") return t.login.signin;
    return t.login.createWorkspace;
  }
  function showPasswordField(m: typeof mode): boolean {
    return m !== "forgot";
  }
  /// Leaving the reset flow must also clear its confirmation, or coming back shows a stale
  /// "check your inbox" over an empty form.
  function switchMode(m: typeof mode) {
    mode = m;
    resetSent = false;
  }
</script>

<div class="flex min-h-[100dvh] w-screen items-center justify-center p-6">
  <Card.Root class="w-full max-w-sm">
    <Card.Header class="items-center text-center">
      <SentinelLogo stacked size={52} />
      <Card.Description class="mt-2">
        {headerDescription(mode)}
      </Card.Description>
    </Card.Header>

    <Card.Content class="space-y-4">
      {#if resetSent}
        <div class="space-y-3 text-center">
          <p class="text-sm font-medium">{t.login.resetSentTitle}</p>
          <p class="text-sm text-muted-foreground">{t.login.resetSentBody}</p>
          <Button variant="outline" class="w-full" onclick={() => switchMode("signin")}>
            {t.login.backToSignin}
          </Button>
        </div>
      {:else}
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
        {#if showPasswordField(mode)}
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
        {/if}
        {#if mode === "signup"}
          <!-- #167. Deliberately unticked by default: a pre-ticked box is not consent, and both
               the GDPR and the LGPD treat it as invalid. The API refuses without the flag too, so
               this is the honest affordance rather than the enforcement. -->
          <label class="flex items-start gap-2.5 text-sm text-muted-foreground">
            <input
              type="checkbox"
              bind:checked={acceptedTerms}
              class="mt-0.5 size-4 shrink-0 rounded border-input accent-foreground"
            />
            <span>
              {t.login.consentPrefix}
              <a href="/terms" target="_blank" rel="noopener" class="underline hover:text-foreground">
                {t.login.termsLink}
              </a>
              {t.login.consentJoin}
              <a href="/privacy" target="_blank" rel="noopener" class="underline hover:text-foreground">
                {t.login.privacyLink}
              </a>.
            </span>
          </label>
        {/if}
        <Button type="submit" class="w-full" disabled={!canSubmit || busy}>
          {#if busy}<Busy />{/if}
          {submitLabel(mode)}
        </Button>
        {#if mode === "signin"}
          <button
            type="button"
            class="w-full text-center text-sm text-muted-foreground hover:text-foreground"
            onclick={() => switchMode("forgot")}
          >
            {t.login.forgotLink}
          </button>
        {/if}
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
      {/if}
    </Card.Content>

    <Card.Footer class="justify-center">
      {#if mode === "signin"}
        <button class="text-sm text-muted-foreground hover:text-foreground" onclick={() => switchMode("signup")}>
          {t.login.noWorkspace} <span class="text-primary">{t.login.createOne}</span>
        </button>
      {:else}
        <button class="text-sm text-muted-foreground hover:text-foreground" onclick={() => switchMode("signin")}>
          {t.login.haveAccount} <span class="text-primary">{t.login.signInLink}</span>
        </button>
      {/if}
    </Card.Footer>
  </Card.Root>
</div>
