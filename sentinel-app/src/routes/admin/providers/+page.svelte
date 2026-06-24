<script lang="ts">
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";

  import Save from "@lucide/svelte/icons/save";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
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

  async function save() {
    if (!botToken.trim()) {
      toast.error("Enter a bot token.");
      return;
    }
    saving = true;
    try {
      await admin.setTelegramProvider(botToken);
      botToken = "";
      provider = await admin.getTelegramProvider();
      toast.success("Telegram provider saved.");
    } catch (err) {
      console.error("Failed to save Telegram provider", err);
      toast.error("Failed to save", { description: String(err) });
    } finally {
      saving = false;
    }
  }
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-2xl font-semibold tracking-tight">Notification Providers</h1>
    <p class="text-sm text-muted-foreground">
      Global notification integrations shared across all tenants.
    </p>
  </div>

  <Card.Root>
    <Card.Header>
      <Card.Title class="flex items-center gap-2">
        <Bot class="size-5" />
        Telegram Bot
        {#if loading}
          <LoaderCircle class="size-4 animate-spin text-muted-foreground" />
        {:else if provider?.configured}
          <Badge variant="outline" class="border-border bg-muted text-foreground">
            Configured
            {#if provider.bot_username}
              · @{provider.bot_username}
            {/if}
          </Badge>
        {:else}
          <Badge variant="outline" class="border-amber-500/30 bg-amber-500/15 text-amber-400">
            Not configured
          </Badge>
        {/if}
      </Card.Title>
      <Card.Description>
        Create a bot with @BotFather and paste its token here. This bot is shared by all tenants —
        each tenant sets their own pairing code in Recipients.
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
              The token is write-only — it is never returned to the UI.
            </p>
          </div>
          <div class="flex justify-end">
            <Button onclick={save} disabled={saving}>
              {#if saving}
                <LoaderCircle class="size-4 animate-spin" />
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
