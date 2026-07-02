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
  import SentinelMark from "$lib/components/sentinel-mark.svelte";

  import { login, signup, loadMe, setToken, googleLoginUrl } from "$lib/auth.svelte";

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
        toast.error("That sign-in did not complete. Please try again.");
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
      toast.error(err instanceof Error ? err.message : "Something went wrong.");
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

<div class="flex min-h-screen w-screen items-center justify-center p-6">
  <Card.Root class="w-full max-w-sm">
    <Card.Header class="items-center text-center">
      <SentinelMark
        size={40}
        class="text-primary"
        style="filter: drop-shadow(var(--brand-glow))"
      />
      <Card.Title class="mt-2 text-xl">Sentinel</Card.Title>
      <Card.Description>
        {mode === "signin" ? "Sign in to your workspace" : "Create a new workspace"}
      </Card.Description>
    </Card.Header>

    <Card.Content class="space-y-4">
      <form class="space-y-4" onsubmit={submit}>
        {#if mode === "signup"}
          <div class="space-y-1.5">
            <Label for="tenant">Workspace name</Label>
            <Input id="tenant" bind:value={tenantName} placeholder="Acme Security" autocomplete="organization" />
          </div>
        {/if}
        <div class="space-y-1.5">
          <Label for="email">Email</Label>
          <Input id="email" type="email" bind:value={email} placeholder="you@example.com" autocomplete="email" />
        </div>
        <div class="space-y-1.5">
          <Label for="password">Password</Label>
          <Input
            id="password"
            type="password"
            bind:value={password}
            placeholder={mode === "signup" ? "At least 8 characters" : "••••••••"}
            autocomplete={mode === "signin" ? "current-password" : "new-password"}
          />
        </div>
        <Button type="submit" class="w-full" disabled={!canSubmit || busy}>
          {#if busy}<Busy />{/if}
          {mode === "signin" ? "Sign in" : "Create workspace"}
        </Button>
      </form>

      <div class="flex items-center gap-3">
        <Separator class="flex-1" />
        <span class="text-xs text-muted-foreground">or</span>
        <Separator class="flex-1" />
      </div>

      <div class="space-y-2">
        <Button variant="outline" class="w-full" onclick={() => (window.location.href = googleLoginUrl())}>
          Continue with Google
        </Button>
        <Button variant="outline" class="w-full" disabled title="Coming soon">
          Continue with Apple
        </Button>
      </div>
    </Card.Content>

    <Card.Footer class="justify-center">
      {#if mode === "signin"}
        <button class="text-sm text-muted-foreground hover:text-foreground" onclick={() => (mode = "signup")}>
          Don't have a workspace? <span class="text-primary">Create one</span>
        </button>
      {:else}
        <button class="text-sm text-muted-foreground hover:text-foreground" onclick={() => (mode = "signin")}>
          Already have an account? <span class="text-primary">Sign in</span>
        </button>
      {/if}
    </Card.Footer>
  </Card.Root>
</div>
