<script lang="ts">
  import "../app.css";
  import "@fontsource/sora/600.css"; // brand wordmark (Sora 600) — logo lockup only
  import { onMount } from "svelte";
  import { toast } from "svelte-sonner";
  import { page } from "$app/stores";
  import { goto, afterNavigate } from "$app/navigation";
  import { data, onEvent } from "$lib/data";
  import { COMMIT } from "$lib/config";
  import { dict, lang, initLang } from "$lib/i18n";
  import type { Alert } from "$lib/types";
  import { navItems } from "$lib/nav";
  import Menu from "@lucide/svelte/icons/menu";
  import { Toaster } from "$lib/components/ui/sonner";
  import SentinelLogo from "$lib/components/sentinel-logo.svelte";
  import { theme, loadTheme, toggleTheme } from "$lib/theme.svelte";
  import Sun from "@lucide/svelte/icons/sun";
  import Moon from "@lucide/svelte/icons/moon";
  // Animated dot-field backdrop — disabled for now; kept for a future feature.
  // import SensorField from "$lib/components/sensor-field.svelte";
  import Splash from "$lib/components/splash.svelte";
  import PanelLeftOpen from "@lucide/svelte/icons/panel-left-open";
  import PanelLeftClose from "@lucide/svelte/icons/panel-left-close";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import UsersRound from "@lucide/svelte/icons/users-round";
  import Building2 from "@lucide/svelte/icons/building-2";
  import Bell from "@lucide/svelte/icons/bell";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import Activity from "@lucide/svelte/icons/activity";
  import Wallet from "@lucide/svelte/icons/wallet";
  import Cpu from "@lucide/svelte/icons/cpu";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import Plug from "@lucide/svelte/icons/plug";
  import { Button } from "$lib/components/ui/button";
  import { billing, billingPortal } from "$lib/api";
  import {
    billingLock,
    syncFromStatus,
    actionFor,
    type BillingAction,
  } from "$lib/billing.svelte";
  import LogOut from "@lucide/svelte/icons/log-out";
  import Undo2 from "@lucide/svelte/icons/undo-2";

  import {
    auth,
    loadToken,
    loadMe,
    logout,
    hasPerm,
    isAssuming,
    exitAssume,
  } from "$lib/auth.svelte";

  let { children } = $props();

  // Resolve the language before first paint (initLang no-ops on the server). In onMount it would
  // land a frame late and the splash — the very first thing anyone sees — would flash English.
  initLang();

  // Reading `lang()` here re-renders the whole shell on a language switch.
  const t = $derived(dict[lang()]);

  // Boot gate: hold the splash until the JWT session check completes.
  let ready = $state(false); // app content mounted
  let showSplash = $state(true); // splash present
  let leaving = $state(false); // splash fading out
  let bootState = $state<"connecting" | "error">("connecting");

  // Minimum time the splash stays up so the brand mark reads as deliberate, not a flicker.
  const MIN_SPLASH_MS = 3000;
  const SPLASH_FADE_MS = 500; // matches the splash's transition-opacity duration

  // Routes reachable WITHOUT a session. `/reset` is the load-bearing one: a locked-out user has no
  // token by definition, so bouncing them to /login makes the password-reset link they were just
  // emailed impossible to open — the boot guard silently defeating the whole feature (#166). The
  // reset page already expects to be entered from outside the app carrying a token; this is the
  // other half of that contract.
  // `/join` carries an invite token from outside the app, for someone who has NO account yet —
  // bouncing them to /login would make the link they were sent do nothing (#181).
  const PUBLIC_PATHS = ["/login", "/reset", "/terms", "/privacy", "/join"];
  const isPublicPath = (p: string) =>
    PUBLIC_PATHS.some((q) => p === q || p.startsWith(q + "/"));

  // ---- boot: require a valid JWT session ----------------------------------
  async function cloudBoot() {
    const bootStart = Date.now();
    loadToken();
    const path = $page.url.pathname;
    // Mount the app behind the splash immediately, but hold the splash for at least
    // MIN_SPLASH_MS, then fade it out (so a fast boot doesn't flash the mark).
    const finish = () => {
      ready = true;
      const wait = Math.max(0, MIN_SPLASH_MS - (Date.now() - bootStart));
      setTimeout(() => {
        leaving = true;
        setTimeout(() => (showSplash = false), SPLASH_FADE_MS);
      }, wait);
    };
    if (!auth.token) {
      auth.ready = true;
      finish();
      if (!isPublicPath(path)) await goto("/login");
      return;
    }
    const ok = await loadMe();
    auth.ready = true;
    if (!ok) {
      logout();
      finish();
      if (!isPublicPath(path)) await goto("/login");
      return;
    }
    finish();
    // A master's own token has no tenant context — tenant-scoped pages would 400.
    // Send them to the console to pick a tenant to operate inside (Navigate as tenant).
    if (
      auth.me?.is_master &&
      !auth.me.tenant_id &&
      !path.startsWith("/admin")
    ) {
      await goto("/admin/tenants");
    }
  }

  let navCollapsed = $state(false);

  // The sidebar is TWO different things. On a desktop it is a permanent column that can be
  // narrowed to icons. On a phone there is no room for a column at all — 240px of a 390px screen
  // is the app — so it becomes an off-canvas drawer over the content. Same markup, so the nav is
  // never written twice; only the frame changes.
  let navOpen = $state(false);

  const asideClass = $derived(
    [
      // phone: off-canvas drawer
      "fixed inset-y-0 left-0 z-50 flex w-60 shrink-0 flex-col",
      "border-r border-sidebar-border bg-sidebar text-sidebar-foreground",
      "transition-transform duration-200",
      navOpen ? "translate-x-0" : "-translate-x-full",
      // desktop: back to a static column that animates its width
      "md:static md:z-auto md:translate-x-0 md:transition-[width]",
      navCollapsed ? "md:w-16" : "md:w-60",
    ].join(" "),
  );

  // A route change means the drawer did its job. Leaving it open would cover the page it just
  // navigated to.
  afterNavigate(() => {
    navOpen = false;
  });
  const themeLabel = $derived(
    theme.current === "dark" ? t.shell.switchToLight : t.shell.switchToDark,
  );
  function toggleNav() {
    navCollapsed = !navCollapsed;
    try {
      localStorage.setItem("sentinel.navCollapsed", String(navCollapsed));
    } catch {
      /* ignore */
    }
  }
  // App-wide alert toasts: the relay forwards `alerts-changed` (tenant-scoped) whenever an
  // alert is raised/delivered; surface anything newer than the baseline as a toast, anywhere.
  function toastAlert(a: Alert) {
    const title = dict[lang()].shell.newAlert(a.level);
    const opts = {
      description: a.room_name ? `${a.message} — ${a.room_name}` : a.message,
    };
    if (a.level === "error") toast.error(title, opts);
    else if (a.level === "warn") toast.warning(title, opts);
    else toast.info(title, opts);
  }

  onMount(() => {
    loadTheme();
    if (localStorage.getItem("sentinel.navCollapsed") === "true")
      navCollapsed = true;
    cloudBoot();

    // No browser pinch-zoom of the whole page on tablet/iPad — Safari ignores `user-scalable=no`,
    // so block its `gesture*` events too. These are Safari's page-zoom, SEPARATE from the pointer
    // events the workflow/map/faces canvases use for their own zoom, so this doesn't touch those.
    const preventGesture = (e: Event) => e.preventDefault();
    document.addEventListener("gesturestart", preventGesture, { passive: false });
    document.addEventListener("gesturechange", preventGesture, { passive: false });
    document.addEventListener("gestureend", preventGesture, { passive: false });

    if (COMMIT === "dev") return;
    let updateToasted = false;
    const checkVersion = async () => {
      if (updateToasted) return;
      try {
        const res = await fetch("/version.json", { cache: "no-store" });
        if (!res.ok) return;
        const { commit } = (await res.json()) as { commit?: string };
        if (commit && commit !== "dev" && commit.slice(0, 7) !== COMMIT) {
          updateToasted = true;
          const s = dict[lang()].shell;
          toast.info(s.updateAvailable, {
            duration: Infinity,
            action: { label: s.reload, onClick: () => location.reload() },
          });
        }
      } catch { /* silent */ }
    };
    const iv = setInterval(checkVersion, 5 * 60 * 1000);
    void checkVersion();
    return () => clearInterval(iv);
  });

  // App-wide alert toasts — only once authenticated. Subscribing earlier would fire
  // unauthenticated `/alerts` calls that 401; the api layer's 401 handler then clears the
  // session and bounces to /login, which on an OAuth return clobbers the token that the
  // login page just set (it races the same mount). Gate on `auth.me` so nothing
  // authenticated runs until we have a session.
  $effect(() => {
    if (!auth.me) return;
    let lastAlertId = 0;
    // Baseline now so we only toast alerts that arrive after this point.
    data
      .listAlerts()
      .then((a) => {
        lastAlertId = a.reduce((m, x) => Math.max(m, x.id), 0);
      })
      .catch(() => {});
    let unsub: (() => void) | null = null;
    onEvent("alerts-changed", async () => {
      try {
        const alerts = await data.listAlerts();
        const newest = alerts.reduce<Alert | null>(
          (m, x) => (!m || x.id > m.id ? x : m),
          null,
        );
        if (newest && newest.id > lastAlertId) {
          lastAlertId = newest.id;
          toastAlert(newest);
        }
      } catch {
        /* ignore */
      }
    }).then((u) => {
      unsub = u;
    });
    return () => unsub?.();
  });

  function isActive(href: string): boolean {
    const path = $page.url.pathname;
    if (href === "/") return path === "/";
    return path === href || path.startsWith(href + "/");
  }

  // Routes that manage their own full-width/height canvas (no centered max-width
  // container, no page padding) — e.g. the workflow editor.
  const FULL_BLEED = ["/workflows", "/map"];
  const fullBleed = $derived(
    FULL_BLEED.some(
      (p) => $page.url.pathname === p || $page.url.pathname.startsWith(p + "/"),
    ),
  );

  // The app shell (sidebar + chrome) shows only once authenticated. The bare-render
  // path (no shell) carries /login.
  const showShell = $derived(auth.me !== null);

  // Per-page read permission (cloud mode filters the nav by these). `null` = always shown.
  const NAV_PERM: Record<string, string | null> = {
    "/": null,
    "/devices": "devices.read",
    "/live": "live.view",
    "/events": "alerts.read",
    "/beacons": "beacons.read",
    "/people": "persons.read",
    "/map": "map.read",
    "/chat": null,
    "/workflows": "workflows.read",
    "/recordings": "recordings.read",
    "/playback": "recordings.read",
    "/logs": "devices.read",
    "/settings": "settings.read",
  };
  // Features without a cloud backend yet — kept as routes but hidden from nav.
  const CLOUD_HIDE = new Set<string>([]);
  // A tenant context exists for normal users always, and for the master only while
  // "acting as" a tenant (assume sets tenant_id). Tenant-scoped pages require it.
  const inTenant = $derived(auth.me?.tenant_id != null);

  // Show the read-only banner before the tenant discovers it by clicking something.
  $effect(() => {
    if (!inTenant || !hasPerm("billing.read")) return;
    billing
      .subscription()
      .then((s) => syncFromStatus(s?.status))
      .catch(() => {});
  });

  // ---- billing banner (kept out of the markup, house style) ------------------
  let portalBusy = $state(false);

  function billingAction(): BillingAction {
    return actionFor(billingLock.status);
  }
  function billingTitle(): string {
    return billingAction() === "portal" ? t.shell.pastDueTitle : t.shell.lapsedTitle;
  }
  /** The 402 backstop has a server message and no status; prefer it when that is all we have. */
  function billingBody(): string {
    if (billingLock.status == null && billingLock.message) return billingLock.message;
    return billingAction() === "portal" ? t.shell.pastDueBody : t.shell.lapsedBody;
  }
  /** Stripe hosts the card form — we never see a card number, so there is nothing to build here. */
  async function openBillingPortal() {
    if (portalBusy) return;
    portalBusy = true;
    try {
      const { url } = await billingPortal.open();
      window.location.href = url;
    } catch (err) {
      // Falling back to the plan grid would be the duplicate-subscription trap again, so this
      // says what failed and leaves the tenant where they are.
      toast.error(t.shell.portalFailed, { description: String(err) });
      portalBusy = false;
    }
  }
  const mainNav = $derived(
    !inTenant
      ? []
      : navItems
          .filter(
            (i) =>
              !CLOUD_HIDE.has(i.href) &&
              (NAV_PERM[i.href] == null || hasPerm(NAV_PERM[i.href]!)),
          )
          .map((i) => ({ ...i, label: t.nav[i.key] })),
  );

  // Settings group — tenant configuration screens (Devices moved here off the main menu),
  // shown by permission (cloud mode only — `me` is null on desktop).
  const settingsItems = $derived(
    !inTenant
      ? []
      : [
          { label: t.nav.general, href: "/settings", icon: SlidersHorizontal },
          ...(hasPerm("devices.read") ? [{ label: t.nav.devices, href: "/settings/devices", icon: Cpu }] : []),
          ...(hasPerm("devices.write") ? [{ label: t.nav.boxes, href: "/settings/boxes", icon: HardDrive }] : []),
          ...(hasPerm("roles.manage") ? [{ label: t.nav.roles, href: "/settings/roles", icon: ShieldCheck }] : []),
          ...(hasPerm("users.manage") ? [{ label: t.nav.users, href: "/settings/users", icon: UsersRound }] : []),
          ...(hasPerm("tuya.read") ? [{ label: t.nav.integrations, href: "/settings/integrations", icon: Plug }] : []),
          ...(hasPerm("billing.read") ? [{ label: t.nav.billing, href: "/billing", icon: Wallet }] : []),
        ],
  );

  // Master-only platform screens.
  const adminItems = $derived([
    ...(auth.me?.is_master
      ? [{ label: t.nav.tenants, href: "/admin/tenants", icon: Building2 }]
      : []),
    ...(auth.me?.is_master
      ? [{ label: t.nav.billing, href: "/admin/billing", icon: Wallet }]
      : []),
    ...(auth.me?.is_master
      ? [{ label: t.nav.storage, href: "/admin/storage", icon: HardDrive }]
      : []),
    ...(auth.me?.is_master
      ? [{ label: t.nav.systemLoad, href: "/admin/jobs", icon: Activity }]
      : []),
    ...(auth.me?.is_master
      ? [{ label: t.nav.providers, href: "/admin/providers", icon: Bell }]
      : []),
  ]);

  async function signOut() {
    logout();
    await goto("/login");
  }
  async function leaveTenant() {
    await exitAssume();
    await goto("/admin/tenants");
  }
</script>

<!-- <SensorField /> — animated backdrop disabled for now; re-enable for a future feature -->

{#if ready}
  {#if showShell}
    <div class="flex h-[100dvh] w-screen overflow-hidden text-foreground">
      <!-- Scrim: tapping outside the drawer closes it. Phone only — on a desktop the sidebar is
           part of the layout and there is nothing to dismiss. -->
      {#if navOpen}
        <button
          type="button"
          class="fixed inset-0 z-40 bg-black/50 md:hidden"
          aria-label={t.shell.closeMenu}
          onclick={() => (navOpen = false)}
        ></button>
      {/if}

      <aside class={asideClass}>
        <div
          class="flex h-[calc(3.5rem+env(safe-area-inset-top))] items-center border-b border-sidebar-border pt-[env(safe-area-inset-top)] md:h-14 md:pt-0 {navCollapsed
            ? 'justify-center px-0'
            : 'gap-2.5 px-4'}"
        >
          {#if navCollapsed}
            <button
              class="interactive rounded-md p-1.5 text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground"
              title={t.shell.expandSidebar}
              aria-label={t.shell.expandSidebar}
              onclick={toggleNav}
            >
              <PanelLeftOpen class="size-5" />
            </button>
          {:else}
            <SentinelLogo size={24} short />
            <button
              class="interactive ml-auto hidden rounded-md p-1 text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground md:block"
              title={t.shell.collapseSidebar}
              aria-label={t.shell.collapseSidebar}
              onclick={toggleNav}
            >
              <PanelLeftClose class="size-4" />
            </button>
          {/if}
        </div>

        <nav class="flex-1 space-y-0.5 overflow-y-auto p-2">
          {#each mainNav as item (item.href)}
            {@const Icon = item.icon}
            {@const active = isActive(item.href)}
            <a
              href={item.href}
              data-active={active}
              title={navCollapsed ? item.label : undefined}
              class="group relative flex items-center gap-3 rounded-md py-2 text-sm font-medium text-muted-foreground transition-colors duration-200 hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground data-[active=true]:bg-sidebar-accent data-[active=true]:text-foreground {navCollapsed
                ? 'justify-center px-0'
                : 'px-3'}"
            >
              <span
                class="absolute left-0 top-1/2 h-5 -translate-y-1/2 rounded-r-full bg-primary transition-all duration-200 {active
                  ? 'w-1 opacity-100'
                  : 'w-0 opacity-0'}"
              ></span>
              <Icon
                class="size-4 shrink-0 transition-colors group-data-[active=true]:text-primary"
              />
              {#if !navCollapsed}<span>{item.label}</span>{/if}
            </a>
          {/each}

          {#if settingsItems.length > 0}
            <div class="px-3 pb-1 pt-3 {navCollapsed ? 'hidden' : ''}">
              <span
                class="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground"
                >{t.nav.settingsGroup}</span
              >
            </div>
            {#each settingsItems as item (item.href)}
              {@const Icon = item.icon}
              {@const active = item.href === "/settings" ? $page.url.pathname === "/settings" : isActive(item.href)}
              <a
                href={item.href}
                data-active={active}
                title={navCollapsed ? item.label : undefined}
                class="group relative flex items-center gap-3 rounded-md py-2 text-sm font-medium text-muted-foreground transition-colors duration-200 hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground data-[active=true]:bg-sidebar-accent data-[active=true]:text-foreground {navCollapsed
                  ? 'justify-center px-0'
                  : 'px-3'}"
              >
                <span
                  class="absolute left-0 top-1/2 h-5 -translate-y-1/2 rounded-r-full bg-primary transition-all duration-200 {active
                    ? 'w-1 opacity-100'
                    : 'w-0 opacity-0'}"
                ></span>
                <Icon class="size-4 shrink-0 transition-colors group-data-[active=true]:text-primary" />
                {#if !navCollapsed}<span>{item.label}</span>{/if}
              </a>
            {/each}
          {/if}

          {#if adminItems.length > 0}
            <div class="px-3 pb-1 pt-3 {navCollapsed ? 'hidden' : ''}">
              <span
                class="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground"
                >{t.nav.adminGroup}</span
              >
            </div>
            {#each adminItems as item (item.href)}
              {@const Icon = item.icon}
              {@const active = isActive(item.href)}
              <a
                href={item.href}
                data-active={active}
                title={navCollapsed ? item.label : undefined}
                class="group relative flex items-center gap-3 rounded-md py-2 text-sm font-medium text-muted-foreground transition-colors duration-200 hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground data-[active=true]:bg-sidebar-accent data-[active=true]:text-foreground {navCollapsed
                  ? 'justify-center px-0'
                  : 'px-3'}"
              >
                <span
                  class="absolute left-0 top-1/2 h-5 -translate-y-1/2 rounded-r-full bg-primary transition-all duration-200 {active
                    ? 'w-1 opacity-100'
                    : 'w-0 opacity-0'}"
                ></span>
                <Icon
                  class="size-4 shrink-0 transition-colors group-data-[active=true]:text-primary"
                />
                {#if !navCollapsed}<span>{item.label}</span>{/if}
              </a>
            {/each}
          {/if}
        </nav>

        {#if auth.me}
          <div class="border-t border-sidebar-border p-2">
            {#if !navCollapsed}
              <div
                class="truncate px-2 py-1 text-[11px] text-muted-foreground"
                title={auth.me.email ?? auth.me.user_id}
              >
                {auth.me.email ?? t.shell.signedIn}
              </div>
            {/if}
            <button
              class="interactive flex w-full items-center gap-3 rounded-md py-2 text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground {navCollapsed
                ? 'justify-center px-0'
                : 'px-3'}"
              title={t.shell.signOut}
              onclick={signOut}
            >
              <LogOut class="size-4 shrink-0" />
              {#if !navCollapsed}<span>{t.shell.signOut}</span>{/if}
            </button>
          </div>
        {/if}

        <div
          class="border-t border-sidebar-border p-3 pb-[max(0.75rem,env(safe-area-inset-bottom))] text-[11px] text-muted-foreground {navCollapsed
            ? 'flex flex-col items-center gap-1'
            : 'flex items-center gap-1.5'}"
        >
          {#if !navCollapsed}
            <span class="size-1.5 rounded-full bg-primary status-live"></span>
            <span class="font-mono">v0.5.0</span>
            <span class="font-mono" title={t.shell.deployedCommit}>· {COMMIT}</span>
            <button
              class="interactive ml-auto rounded-md p-1 hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground"
              title={themeLabel}
              aria-label={themeLabel}
              onclick={toggleTheme}
            >
              {#if theme.current === "dark"}
                <Sun class="size-4" />
              {:else}
                <Moon class="size-4" />
              {/if}
            </button>
          {:else}
            <button
              class="interactive rounded-md p-1 hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground"
              title={themeLabel}
              aria-label={themeLabel}
              onclick={toggleTheme}
            >
              {#if theme.current === "dark"}
                <Sun class="size-4" />
              {:else}
                <Moon class="size-4" />
              {/if}
            </button>
          {/if}
        </div>
      </aside>

      <div class="flex min-w-0 flex-1 flex-col">
        <!-- Phone-only chrome. The desktop has no top bar and does not need one; the phone has no
             visible sidebar, so this is the only way in. `pt-[env(safe-area-inset-top)]` keeps it
             clear of the notch — the viewport is already `viewport-fit=cover`, which puts content
             under the status bar unless it is padded. -->
        <div
          class="flex h-[calc(3.5rem+env(safe-area-inset-top))] shrink-0 items-center gap-3 border-b border-sidebar-border bg-sidebar px-4 pt-[env(safe-area-inset-top)] md:hidden"
        >
          <button
            type="button"
            class="interactive rounded-md p-1.5 text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground"
            aria-label={t.shell.openMenu}
            onclick={() => (navOpen = true)}
          >
            <Menu class="size-5" />
          </button>
          <SentinelLogo size={22} short />
        </div>
        {#if isAssuming()}
          <div
            class="flex items-center gap-2 border-b border-amber-500/30 bg-amber-500/10 px-4 py-1.5 text-xs text-amber-600 dark:text-amber-400"
          >
            <span>{t.shell.assumingBanner}</span>
            <button
              class="interactive ml-auto inline-flex items-center gap-1 rounded-md px-2 py-0.5 font-medium hover:bg-amber-500/15"
              onclick={leaveTenant}
            >
              <Undo2 class="size-3.5" /> {t.shell.exit}
            </button>
          </div>
        {/if}
        <!-- Billing banner: shown on every page of a locked workspace, and NOT dismissible —
             footage is quietly failing to reach the cloud while it is up, so a banner someone can
             hide is a banner that costs them recordings. -->
        {#if billingLock.locked}
          <div
            class="flex items-center justify-between gap-4 border-b border-destructive/30 bg-destructive/10 px-6 py-2.5 text-sm"
          >
            <span class="min-w-0">
              <span class="font-medium">{billingTitle()}</span>
              <span class="text-muted-foreground"> {billingBody()}</span>
            </span>
            {#if billingAction() === "portal"}
              <Button size="sm" disabled={portalBusy} onclick={openBillingPortal}>
                {t.shell.updatePayment}
              </Button>
            {:else}
              <Button size="sm" onclick={() => goto("/billing")}>{t.shell.choosePlan}</Button>
            {/if}
          </div>
        {/if}
        <main class="flex-1 overflow-y-auto">
          <!-- Key on the route id, not the full path, so param-only changes
               (e.g. /workflows ↔ /workflows/123) don't remount the page. -->
          {#key $page.route?.id}
            {#if fullBleed}
              <div
                class="h-full overflow-hidden duration-200 animate-in fade-in"
              >
                {@render children()}
              </div>
            {:else}
              <div
                class="w-full p-6 duration-300 animate-in fade-in slide-in-from-bottom-2"
              >
                {@render children()}
              </div>
            {/if}
          {/key}
        </main>
      </div>
    </div>
  {:else}
    {@render children()}
  {/if}
{/if}

{#if showSplash}
  <Splash state={bootState} {leaving} />
{/if}

<Toaster />
