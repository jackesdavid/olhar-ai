<script lang="ts">
  import PageHeader from "$lib/components/page-header.svelte";
  import Bell from "@lucide/svelte/icons/bell";
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";

  import Save from "@lucide/svelte/icons/save";
  import Busy from "$lib/components/busy.svelte";
  import Bot from "@lucide/svelte/icons/bot";

  import * as Card from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";
  import Loader from "$lib/components/loader.svelte";

  import { admin } from "$lib/api";
  import type { TelegramProviderState } from "$lib/types";

  let loading = $state(true);
  let provider = $state<TelegramProviderState | null>(null);
  let botToken = $state("");
  let saving = $state(false);

  onMount(async () => {
    try {
      provider = await admin.getTelegramProvider();
    } catch (err) {
      console.error("Failed to load Telegram provider", err);
      toast.error("Failed to load provider config", { description: String(err) });
    } finally {
      loading = false;
    }
  });

  // The badge must tell the truth (card #73): a master with no GLOBAL bot but tenants running their
  // own bot was being told "Not configured" while delivery worked. Three honest states, not two.
  type ProviderState = "global" | "per-tenant" | "none";
  function providerState(): ProviderState {
    if (provider?.configured) return "global";
    if ((provider?.tenant_bot_count ?? 0) > 0) return "per-tenant";
    return "none";
  }
  function tenantBotNote(): string {
    const n = provider?.tenant_bot_count ?? 0;
    return `${n} tenant${n === 1 ? "" : "s"} use their own bot`;
  }

  async function save() {
    if (!botToken.trim()) {
      toast.error("Enter a bot token.");
      return;
    }
    saving = true;
    try {
      // The endpoint validates the token with Telegram and returns the resolved state, so we show
      // the real @username immediately — no second fetch, and a rejected token surfaces as an error.
      provider = await admin.setTelegramProvider(botToken);
      botToken = "";
      toast.success(
        provider.bot_username ? `Saved — verified as @${provider.bot_username}.` : "Global bot cleared.",
      );
    } catch (err) {
      console.error("Failed to save Telegram provider", err);
      toast.error(err instanceof Error ? err.message : "Failed to save the token.");
    } finally {
      saving = false;
    }
  }
</script>

<div class="space-y-6">
  <PageHeader
    title="Notification Providers"
    description="Global notification integrations shared across all tenants."
    icon={Bell}
  />

  <Card.Root>
    <Card.Header>
      <Card.Title class="flex items-center gap-2">
        <Bot class="size-5" />
        Telegram Bot
        {#if loading}
          <Busy class="text-muted-foreground" />
        {:else if providerState() === "global"}
          <Badge variant="outline" class="border-border bg-muted text-foreground">
            Global bot active
            {#if provider?.bot_username}
              · @{provider.bot_username}
            {/if}
          </Badge>
        {:else if providerState() === "per-tenant"}
          <Badge variant="outline" class="border-border bg-muted text-muted-foreground">
            No global bot · {tenantBotNote()}
          </Badge>
        {:else}
          <Badge variant="outline" class="border-amber-500/30 bg-amber-500/15 text-amber-400">
            Not configured
          </Badge>
        {/if}
      </Card.Title>
      <Card.Description>
        Create a bot with @BotFather and paste its token here. A global bot is shared by every
        tenant as a fallback; a tenant can also set their own bot in Recipients, which takes
        precedence. Either way, each tenant sets their own pairing code in Recipients.
      </Card.Description>
    </Card.Header>
    <Card.Content>
      {#if loading}
        <Loader label="Loading…" />
      {:else}
        <div class="space-y-4">
          <div class="space-y-2">
            <Label for="bot-token">Bot token</Label>
            <Input
              id="bot-token"
              type="password"
              bind:value={botToken}
              placeholder={provider?.configured
                ? "•••••••• (set — paste new token to replace)"
                : "123456:ABC-DEF…"}
              autocomplete="off"
              class="max-w-sm"
            />
            <p class="text-xs text-muted-foreground">
              The token is write-only — it is never returned to the UI. It's checked with Telegram on
              save, so an invalid token is rejected instead of silently stored.
            </p>
          </div>

          <details class="rounded-lg border border-border bg-well/50 px-3 py-2 text-sm">
            <summary class="cursor-pointer text-muted-foreground">How do I get a bot token?</summary>
            <ol class="mt-2 list-decimal space-y-1 pl-5 text-muted-foreground">
              <li>In Telegram, open <a href="https://t.me/BotFather" target="_blank" rel="noopener" class="underline">@BotFather</a>.</li>
              <li>Send <code>/newbot</code> and follow the prompts (name + a username ending in <code>bot</code>).</li>
              <li>BotFather replies with a token like <code>123456:ABC-DEF…</code> — paste it above.</li>
            </ol>
          </details>
          <div class="flex justify-end">
            <Button onclick={save} disabled={saving}>
              {#if saving}
                <Busy />
                Saving…
              {:else}
                <Save class="size-4" />
                Save token
              {/if}
            </Button>
          </div>
        </div>
      {/if}
    </Card.Content>
  </Card.Root>
</div>
