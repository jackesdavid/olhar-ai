<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { toast } from "svelte-sonner";

  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Copy from "@lucide/svelte/icons/copy";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import Check from "@lucide/svelte/icons/check";
  import Terminal from "@lucide/svelte/icons/terminal";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Download from "@lucide/svelte/icons/download";

  import * as Card from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Separator } from "$lib/components/ui/separator";
  import { boxes as boxApi, type Installer, type InstallerOS } from "$lib/api";
  import { API_BASE } from "$lib/config";
  import { pendingInstall } from "$lib/box-install.svelte";
  import { dict, lang } from "$lib/i18n";

  const t = $derived(dict[lang()].boxInstall);

  const install = $derived(pendingInstall.value);

  let code = $state("");
  let expiresAt = $state("");
  let minting = $state(true);
  // Pairing progress (card #85): waiting → accepted (code redeemed, box booting) → online.
  let claimed = $state(false);
  let online = $state(false);
  let stalled = $state(false); // "accepted" has lingered too long — box probably isn't coming up
  let acceptedAt: number | null = null; // when we first saw `claimed` (plain — only the poll reads it)
  let showAdvanced = $state(false);
  let installers = $state<Installer[]>([]);
  let showAllPlatforms = $state(false);
  let timer: ReturnType<typeof setInterval> | null = null;

  // The token only exists in memory from the create step — on a refresh or direct hit
  // there's nothing to show, so send the user back to the list.
  onMount(() => {
    if (!pendingInstall.value) {
      goto("/settings/boxes");
      return;
    }
    mint();
    loadInstallers();
    // Watch the box arrive. The appliance checks in within seconds of being paired, and seeing it
    // happen is the whole confirmation the user needs — better than being told to go look.
    timer = setInterval(checkOnline, 4000);
  });

  // ---- installer downloads (card #84) ----------------------------------------
  // Control names the current per-platform files; we render what it gives us, so a version bump is
  // a control-only change. Only files that exist on the dist server come back, so a button can't 404.
  async function loadInstallers() {
    try {
      installers = (await boxApi.installers()).installers;
    } catch {
      installers = []; // best-effort — the headless command below still gets them there
    }
  }

  function detectOS(): InstallerOS {
    const ua = navigator.userAgent;
    if (/Windows/i.test(ua)) return "windows";
    if (/Mac OS X|Macintosh/i.test(ua)) return "macos";
    return "linux";
  }
  const myOS = detectOS();

  /** The installer for the machine the user is on, if we have one. */
  function primaryInstaller(): Installer | undefined {
    return installers.find((i) => i.os === myOS);
  }
  /** Everything except the primary — the "other platforms" list. */
  function otherInstallers(): Installer[] {
    const primary = primaryInstaller();
    return installers.filter((i) => i !== primary);
  }

  function downloadHref(i: Installer): string {
    return `${API_BASE}${i.url}`;
  }
  function sizeLabel(i: Installer): string {
    if (i.size_bytes == null) return "";
    const mb = i.size_bytes / (1024 * 1024);
    return mb >= 1 ? ` · ${mb.toFixed(0)} MB` : "";
  }
  function kindLabel(i: Installer): string {
    if (i.kind === "sh") return t.oneLineCommand;
    return `.${i.kind}`;
  }

  onDestroy(() => {
    if (timer) clearInterval(timer);
  });

  async function mint() {
    if (!pendingInstall.value) return;
    minting = true;
    try {
      const r = await boxApi.pairingCode(pendingInstall.value.id);
      code = r.code;
      expiresAt = r.expires_at;
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t.codeFailed);
    } finally {
      minting = false;
    }
  }

  async function checkOnline() {
    if (!pendingInstall.value || online) return;
    try {
      const s = await boxApi.pairingStatus(pendingInstall.value.id);
      if (s.claimed && acceptedAt === null) acceptedAt = Date.now();
      claimed = s.claimed;
      // Surface a stall if "accepted" lingers past a minute (box booting but not checking in — the
      // launchd-blocked / SentinelOne case) so we nudge instead of spinning forever. Computed here,
      // in the 4s poll, so it stays reactive without a separate timer.
      stalled = s.claimed && !s.online && acceptedAt !== null && Date.now() - acceptedAt > 60_000;
      if (s.online) {
        online = true;
        if (timer) clearInterval(timer);
      }
    } catch {
      // A failed poll is not worth a toast — the next one is four seconds away.
    }
  }

  // The three states the view renders. `accepted` is the new one — the code was redeemed and the
  // appliance is restarting into a real box, but it hasn't checked in yet.
  type PairPhase = "waiting" | "accepted" | "online";
  function phase(): PairPhase {
    if (online) return "online";
    if (claimed) return "accepted";
    return "waiting";
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(t.copied);
    } catch {
      toast.error(t.copyFailed);
    }
  }

  function done() {
    pendingInstall.value = null;
    goto("/settings/boxes");
  }

  function codeDisplay(): string {
    if (minting) return "·····";
    return code || "—";
  }

  function expiryHint(): string {
    if (!expiresAt) return "";
    const mins = Math.max(0, Math.round((new Date(expiresAt).getTime() - Date.now()) / 60000));
    if (mins <= 0) return t.expired;
    return t.expiresIn(mins);
  }
</script>

{#if install}
  <div class="mx-auto max-w-2xl space-y-6">
    <div class="space-y-3">
      <Button variant="ghost" size="sm" class="-ml-2 text-muted-foreground" onclick={() => goto("/settings/boxes")}>
        <ArrowLeft class="size-4" /> {t.backToBoxes}
      </Button>
      <div class="flex items-center gap-3">
        <HardDrive class="size-6 text-primary" />
        <div>
          <h1 class="text-2xl font-semibold tracking-tight">{t.connectTitle(install.name)}</h1>
          <p class="text-sm text-muted-foreground">{t.connectSubtitle}</p>
        </div>
      </div>
    </div>

    {#if online}
      <Card.Root class="border-primary/30 bg-primary/5">
        <Card.Content class="flex items-center gap-3 py-6">
          <Check class="size-5 shrink-0 text-primary" />
          <div>
            <p class="font-medium">{t.onlineTitle(install.name)}</p>
            <p class="text-sm text-muted-foreground">{t.onlineHint}</p>
          </div>
        </Card.Content>
      </Card.Root>
    {:else if phase() === "accepted"}
      <Card.Root class="border-primary/30 bg-primary/5">
        <Card.Content class="space-y-2 py-6">
          <div class="flex items-center gap-3">
            <RefreshCw class="size-5 shrink-0 animate-spin text-primary" />
            <div>
              <p class="font-medium">{t.acceptedTitle(install.name)}</p>
              <p class="text-sm text-muted-foreground">{t.acceptedHint}</p>
            </div>
          </div>
          {#if stalled}
            <p class="pl-8 text-sm text-amber-700 dark:text-amber-400">{t.stalledHint}</p>
          {/if}
        </Card.Content>
      </Card.Root>
    {:else}
      {#if primaryInstaller()}
        <Card.Root>
          <Card.Header>
            <Card.Title>{t.installStep}</Card.Title>
            <Card.Description>{t.installDescription}</Card.Description>
          </Card.Header>
          <Card.Content class="space-y-3">
            {@const primary = primaryInstaller()}
            {#if primary}
              <Button href={downloadHref(primary)} download class="w-full justify-start gap-3" size="lg">
                <Download class="size-4 shrink-0" />
                <span>{t.downloadFor(primary.label)}<span class="text-xs opacity-70">{" "}({kindLabel(primary)}{sizeLabel(primary)})</span></span>
              </Button>
            {/if}

            {#if otherInstallers().length > 0}
              <Button variant="ghost" size="sm" class="-ml-2 text-muted-foreground" onclick={() => (showAllPlatforms = !showAllPlatforms)}>
                {showAllPlatforms ? t.hide : t.show} {t.otherPlatforms}
              </Button>
              {#if showAllPlatforms}
                <div class="space-y-1.5">
                  {#each otherInstallers() as i (i.os)}
                    <a
                      href={downloadHref(i)}
                      download={i.kind !== "sh"}
                      target={i.kind === "sh" ? "_blank" : undefined}
                      rel="noopener"
                      class="interactive flex items-center gap-2 rounded-md border border-border bg-well px-3 py-2 text-sm"
                    >
                      <Download class="size-3.5 shrink-0 text-muted-foreground" />
                      {i.label}<span class="text-xs text-muted-foreground">{kindLabel(i)}{sizeLabel(i)}</span>
                    </a>
                  {/each}
                </div>
              {/if}
            {/if}
          </Card.Content>
        </Card.Root>
      {/if}

      <Card.Root>
        <Card.Header>
          <Card.Title>{primaryInstaller() ? t.pairingStepPrefix : ""}{t.pairingCode}</Card.Title>
          <Card.Description>{t.pairingDescription}</Card.Description>
        </Card.Header>
        <Card.Content class="space-y-4">
          <button
            type="button"
            class="interactive w-full rounded-lg border border-border bg-well px-4 py-6 text-center font-mono text-3xl font-semibold tracking-[0.2em] tabular-nums"
            title={t.copyCode}
            onclick={() => copy(code)}
            disabled={minting || !code}
          >
            {codeDisplay()}
          </button>
          <div class="flex items-center justify-between text-xs text-muted-foreground">
            <span>{expiryHint()}</span>
            <Button variant="ghost" size="sm" onclick={mint} disabled={minting}>
              <RefreshCw class="size-3.5" /> {t.newCode}
            </Button>
          </div>
          <p class="flex items-center gap-2 text-sm text-muted-foreground">
            <span class="status-live size-2 shrink-0 rounded-full bg-muted-foreground/60"></span>
            {t.waitingFor(install.name)}
          </p>
        </Card.Content>
      </Card.Root>
    {/if}

    <Separator />

    <div class="space-y-3">
      <Button variant="ghost" size="sm" class="-ml-2 text-muted-foreground" onclick={() => (showAdvanced = !showAdvanced)}>
        <Terminal class="size-4" /> {showAdvanced ? t.hide : t.show} {t.headlessToggle}
      </Button>

      {#if showAdvanced}
        <Card.Root>
          <Card.Header>
            <Card.Title class="text-base">{t.headlessTitle}</Card.Title>
            <Card.Description>{t.headlessDescription}</Card.Description>
          </Card.Header>
          <Card.Content class="space-y-4">
            <div class="relative">
              <pre class="overflow-x-auto rounded-md border border-border bg-well p-4 pr-12 font-mono text-xs leading-relaxed">{install.command}</pre>
              <Button variant="ghost" size="icon" class="absolute right-2 top-2" title={t.copyCommand} onclick={() => copy(install.command)}>
                <Copy class="size-4" />
              </Button>
            </div>

            <div class="space-y-1.5">
              <Label>{t.boxToken}</Label>
              <div class="flex items-center gap-2">
                <code class="flex-1 truncate rounded-md border border-border bg-well px-2.5 py-1.5 font-mono text-xs">{install.token}</code>
                <Button variant="outline" size="sm" onclick={() => copy(install.token)}><Copy class="size-3.5" /> {t.copy}</Button>
              </div>
            </div>

            <p class="flex items-center gap-2 text-xs text-primary">
              <Check class="size-3.5 shrink-0" /> {t.nothingElse}
            </p>
          </Card.Content>
        </Card.Root>
      {/if}
    </div>

    <div class="flex justify-end">
      <Button onclick={done}>{online ? t.done : t.later}</Button>
    </div>
  </div>
{/if}
