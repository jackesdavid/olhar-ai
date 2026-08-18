<!--
  The Telegram PROVIDER panel — the pairing code, the enrollment link and its QR.

  It lives on Integrations, not on Recipients, because of a split that had blurred: Integrations is
  where a provider is configured (credentials, the code that authorises pairing); Recipients is who
  receives. Configuring the bot on the Recipients screen made "is Telegram connected?" answerable
  only by someone who already knew which screen it hid on (#191).

  Self-contained on purpose: it loads and saves its own config, so both screens do not have to keep
  a copy of the same state in sync.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";
  import { data } from "$lib/data";
  import QRCode from "qrcode";

  import Save from "@lucide/svelte/icons/save";
  import Copy from "@lucide/svelte/icons/copy";
  import Check from "@lucide/svelte/icons/check";
  import Link2 from "@lucide/svelte/icons/link-2";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Share2 from "@lucide/svelte/icons/share-2";
  import ExternalLink from "@lucide/svelte/icons/external-link";

  import * as Card from "$lib/components/ui/card";
  import { Button, buttonVariants } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Badge } from "$lib/components/ui/badge";
  import Loader from "$lib/components/loader.svelte";
  import Busy from "$lib/components/busy.svelte";

  import { dict, lang } from "$lib/i18n";
  import type { TelegramConfigState } from "$lib/types";

  const t = $derived(dict[lang()].beacons);
  const c = $derived(dict[lang()].common);

  let config = $state<TelegramConfigState | null>(null);
  let loading = $state(true);
  let pairingCode = $state("");
  let savingCode = $state(false);
  let copied = $state(false);
  let copiedCmd = $state(false);
  let qrSvg = $state("");

  $effect(() => {
    const link = currentLink();
    if (!link) {
      qrSvg = "";
      return;
    }
    QRCode.toString(link, { type: "svg", margin: 1, width: 160 })
      .then((svg) => (qrSvg = svg))
      .catch(() => (qrSvg = ""));
  });

  function enrollmentLink(username: string, code: string) {
    return `https://t.me/${username}?start=${code}`;
  }
  function currentLink(): string {
    if (!config?.bot_username || !config.pairing_code) return "";
    return enrollmentLink(config.bot_username, config.pairing_code);
  }

  async function copyEnrollmentLink() {
    const link = currentLink();
    if (!link) return;
    await navigator.clipboard.writeText(link);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }

  /** The exact message a recipient pastes when the deep-link Start button won't resend the
   *  payload — i.e. they've opened this bot before, so Telegram reopens the chat and drops the
   *  `start=` code. Pasting `/start <code>` always works. */
  function startCommand(): string {
    return config?.pairing_code ? `/start ${config.pairing_code}` : "";
  }
  async function copyStartCommand() {
    const cmd = startCommand();
    if (!cmd) return;
    await navigator.clipboard.writeText(cmd);
    copiedCmd = true;
    setTimeout(() => (copiedCmd = false), 2000);
  }

  function canShare(): boolean {
    return typeof navigator !== "undefined" && typeof navigator.share === "function";
  }
  async function shareEnrollmentLink() {
    const link = currentLink();
    if (!link) return;
    try {
      await navigator.share({ title: t.shareTitle, text: t.shareText, url: link });
    } catch {
      // Share sheet dismissed — not an error worth a toast.
    }
  }

  /** Don't make anyone invent a secret. A fresh strong code, editable, rotatable. */
  function generateCode(): string {
    const alphabet = "abcdefghjkmnpqrstuvwxyz23456789"; // no lookalikes
    const bytes = crypto.getRandomValues(new Uint8Array(10));
    return Array.from(bytes, (b) => alphabet[b % alphabet.length]).join("");
  }
  async function rotateCode() {
    pairingCode = generateCode();
    await saveCode();
  }

  async function saveCode() {
    savingCode = true;
    try {
      await data.setPairingCode(pairingCode.trim());
      config = await data.getTelegramConfig();
      toast.success(t.codeSaved);
    } catch (err) {
      console.error("Failed to save pairing code", err);
      toast.error(t.saveFailed, { description: String(err) });
    } finally {
      savingCode = false;
    }
  }

  onMount(async () => {
    try {
      config = await data.getTelegramConfig();
      pairingCode = config?.pairing_code ?? "";
      // Bot ready but no code yet → offer a strong one pre-filled. Not auto-saved: the operator
      // stays in control of when pairing becomes possible.
      if (config?.provider_configured && !config.pairing_code && !pairingCode) {
        pairingCode = generateCode();
      }
    } catch (err) {
      console.error("Failed to load Telegram config", err);
      toast.error(t.loadFailed, { description: String(err) });
    } finally {
      loading = false;
    }
  });
</script>

{#if loading}
  <Loader />
{:else}
<!-- Telegram enrollment -->
<Card.Root>
  <Card.Header>
    <Card.Title class="flex items-center gap-2">
      {t.telegram}
      {#if config?.provider_configured}
        <Badge variant="outline" class="border-primary/30 bg-primary/10 text-primary">
          {t.providerActive}
        </Badge>
      {:else}
        <Badge variant="outline" class="border-amber-500/30 bg-amber-500/15 text-amber-400">
          {t.providerNotConfigured}
        </Badge>
      {/if}
    </Card.Title>
    <Card.Description>
      {config?.provider_configured ? t.configuredHint : t.notConfiguredHint}
    </Card.Description>
  </Card.Header>
  {#if config?.provider_configured}
    <Card.Content class="space-y-4">
      <div class="space-y-2">
        <Label for="pairing-code">{t.pairingCode}</Label>
        <div class="flex flex-wrap gap-2">
          <Input
            id="pairing-code"
            bind:value={pairingCode}
            placeholder={t.autoGenerated}
            autocomplete="off"
            class="max-w-xs font-mono"
          />
          <Button onclick={saveCode} disabled={savingCode} size="sm">
            {#if savingCode}
              <Busy />
              {t.saving}
            {:else}
              <Save class="size-4" />
              {c.save}
            {/if}
          </Button>
          <Button variant="outline" size="sm" onclick={rotateCode} disabled={savingCode} title={t.newCodeTitle}>
            <RefreshCw class="size-3.5" />
            {t.newCode}
          </Button>
        </div>
        <p class="text-xs text-muted-foreground">{t.codeHint}</p>
      </div>

      {#if config.bot_username && config.pairing_code}
        <div class="rounded-md border border-border bg-muted/40 p-4 text-sm">
          <div class="flex flex-col gap-4 sm:flex-row sm:items-center">
            {#if qrSvg}
              <div class="shrink-0 self-center rounded-md bg-white p-2 [&>svg]:block [&>svg]:size-32">
                <!-- eslint-disable-next-line svelte/no-at-html-tags -->
                {@html qrSvg}
              </div>
            {/if}
            <div class="min-w-0 flex-1 space-y-3">
              <div>
                <p class="font-medium text-foreground">{t.threeSteps}</p>
                <ol class="mt-1 list-decimal space-y-0.5 pl-5 text-xs text-muted-foreground">
                  <li>{t.step1}</li>
                  <li>{t.step2Before}<strong>{t.step2Bold}</strong>{t.step2After}</li>
                  <li>{t.step3}</li>
                </ol>
              </div>
              <div class="flex items-center gap-2">
                <Link2 class="size-4 shrink-0 text-primary" />
                <span class="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground">
                  {enrollmentLink(config.bot_username, config.pairing_code)}
                </span>
              </div>
              <div class="flex flex-wrap items-center gap-1">
                <Button variant="outline" size="sm" class="h-7 gap-1 text-xs" onclick={copyEnrollmentLink}>
                  {#if copied}
                    <Check class="size-3 text-primary" />
                    {t.copied}
                  {:else}
                    <Copy class="size-3" />
                    {t.copyLink}
                  {/if}
                </Button>
                {#if canShare()}
                  <Button variant="outline" size="sm" class="h-7 gap-1 text-xs" onclick={shareEnrollmentLink}>
                    <Share2 class="size-3" />
                    {t.share}
                  </Button>
                {/if}
                <a
                  href={enrollmentLink(config.bot_username, config.pairing_code)}
                  target="_blank"
                  rel="noopener noreferrer"
                  class={buttonVariants({ variant: "outline", size: "sm" }) + " h-7 gap-1 text-xs"}
                >
                  <ExternalLink class="size-3" />
                  {t.open}
                </a>
              </div>
              <div class="rounded-md border border-dashed border-border/70 bg-background/40 p-2.5">
                <p class="text-xs text-muted-foreground">{t.noResponseHint}</p>
                <div class="mt-1.5 flex items-center gap-2">
                  <code class="min-w-0 flex-1 truncate rounded bg-muted px-2 py-1 font-mono text-xs text-foreground">
                    {startCommand()}
                  </code>
                  <Button variant="outline" size="sm" class="h-7 gap-1 text-xs" onclick={copyStartCommand}>
                    {#if copiedCmd}
                      <Check class="size-3 text-primary" />
                      {t.copied}
                    {:else}
                      <Copy class="size-3" />
                      {t.copy}
                    {/if}
                  </Button>
                </div>
              </div>
            </div>
          </div>
        </div>
      {/if}
    </Card.Content>
  {/if}
</Card.Root>
{/if}
