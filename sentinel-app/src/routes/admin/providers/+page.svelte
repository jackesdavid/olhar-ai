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
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].admin);

  let loading = $state(true);
  let provider = $state<TelegramProviderState | null>(null);
  let botToken = $state("");
  let saving = $state(false);

  onMount(async () => {
    try {
      provider = await admin.getTelegramProvider();
    } catch (err) {
      console.error("Failed to load Telegram provider", err);
      toast.error(t.providerLoadFailed, { description: String(err) });
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
    return t.tenantBots(n);
  }

  async function save() {
    if (!botToken.trim()) {
      toast.error(t.enterBotToken);
      return;
    }
    saving = true;
    try {
      // The endpoint validates the token with Telegram and returns the resolved state, so we show
      // the real @username immediately — no second fetch, and a rejected token surfaces as an error.
      provider = await admin.setTelegramProvider(botToken);
      botToken = "";
      toast.success(provider.bot_username ? t.tokenVerified(provider.bot_username) : t.globalBotCleared);
    } catch (err) {
      console.error("Failed to save Telegram provider", err);
      toast.error(err instanceof Error ? err.message : t.tokenSaveFailed);
    } finally {
      saving = false;
    }
  }
</script>

<div class="space-y-6">
  <PageHeader
    title={t.providersTitle}
    description={t.providersDescription}
    icon={Bell}
  />

  <Card.Root>
    <Card.Header>
      <Card.Title class="flex items-center gap-2">
        <Bot class="size-5" />
        {t.telegramBot}
        {#if loading}
          <Busy class="text-muted-foreground" />
        {:else if providerState() === "global"}
          <Badge variant="outline" class="border-border bg-muted text-foreground">
            {t.globalBotActive}
            {#if provider?.bot_username}
              · @{provider.bot_username}
            {/if}
          </Badge>
        {:else if providerState() === "per-tenant"}
          <Badge variant="outline" class="border-border bg-muted text-muted-foreground">
            {t.noGlobalBot} · {tenantBotNote()}
          </Badge>
        {:else}
          <Badge variant="outline" class="border-amber-500/30 bg-amber-500/15 text-amber-400">
            {t.notConfigured}
          </Badge>
        {/if}
      </Card.Title>
      <Card.Description>
        {t.providersHelp}
      </Card.Description>
    </Card.Header>
    <Card.Content>
      {#if loading}
        <Loader label={t.loading} />
      {:else}
        <div class="space-y-4">
          <div class="space-y-2">
            <Label for="bot-token">{t.botToken}</Label>
            <Input
              id="bot-token"
              type="password"
              bind:value={botToken}
              placeholder={provider?.configured ? t.botTokenSet : t.botTokenPlaceholder}
              autocomplete="off"
              class="max-w-sm"
            />
            <p class="text-xs text-muted-foreground">{t.tokenWriteOnly}</p>
          </div>

          <details class="rounded-lg border border-border bg-well/50 px-3 py-2 text-sm">
            <summary class="cursor-pointer text-muted-foreground">{t.howToToken}</summary>
            <ol class="mt-2 list-decimal space-y-1 pl-5 text-muted-foreground">
              <li>{t.tokenStep1Before}<a href="https://t.me/BotFather" target="_blank" rel="noopener" class="underline">@BotFather</a>{t.tokenStep1After}</li>
              <li>{t.tokenStep2Before}<code>/newbot</code>{t.tokenStep2After}<code>bot</code>{t.tokenStep2End}</li>
              <li>{t.tokenStep3Before}<code>123456:ABC-DEF…</code>{t.tokenStep3After}</li>
            </ol>
          </details>
          <div class="flex justify-end">
            <Button onclick={save} disabled={saving}>
              {#if saving}
                <Busy />
                {t.saving}
              {:else}
                <Save class="size-4" />
                {t.saveToken}
              {/if}
            </Button>
          </div>
        </div>
      {/if}
    </Card.Content>
  </Card.Root>
</div>
