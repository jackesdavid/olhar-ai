<script lang="ts">
  import { onMount } from "svelte";
  import ScanFace from "@lucide/svelte/icons/scan-face";
  import Video from "@lucide/svelte/icons/video";
  import BellRing from "@lucide/svelte/icons/bell-ring";
  import Workflow from "@lucide/svelte/icons/workflow";
  import MapPinned from "@lucide/svelte/icons/map-pinned";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Check from "@lucide/svelte/icons/check";
  import Lock from "@lucide/svelte/icons/lock";
  import Eye from "@lucide/svelte/icons/eye";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import KeyRound from "@lucide/svelte/icons/key-round";

  import OlharMark from "$lib/components/olhar-mark.svelte";
  import SensorField from "$lib/components/sensor-field.svelte";
  import HeroNet from "$lib/components/hero-net.svelte";
  import { lang, setLang, initLang, dict, type Lang } from "$lib/i18n.svelte";

  const APP = "https://app.olharai.com";

  // All copy, in the current language — re-derives (and re-renders the page) on toggle.
  const t = $derived(dict[lang()]);

  // Icons / numbers / asset paths are language-agnostic; the parallel text arrays live in the
  // dictionary and are zipped by index in each section below.
  const featureIcons = [Eye, ScanFace, Workflow, Video, BellRing, MapPinned];
  const useCaseIcons = [BellRing, Video, MessageSquare, Eye, ScanFace, Workflow];
  const privacyIcons = [Lock, ShieldCheck, Eye, KeyRound];
  const stepNums = ["01", "02", "03"];
  const shotFiles = [
    "/shots/workflow-editor.png",
    "/shots/live-grid.png",
    "/shots/map-presence.png",
    "/shots/tracking-overlay.png",
  ];
  const langs: Lang[] = ["en", "pt"];

  // Sticky nav: warm the border once the page scrolls a touch.
  let hdr = $state<HTMLElement>();
  onMount(() => {
    initLang();
    const onScroll = () => hdr?.setAttribute("data-s", String(scrollY > 8));
    onScroll();
    addEventListener("scroll", onScroll, { passive: true });
    return () => removeEventListener("scroll", onScroll);
  });

  // Scroll-reveal: fade/slide a node in when it enters the viewport (once).
  function reveal(node: HTMLElement, delay = 0) {
    node.style.transitionDelay = `${delay}ms`;
    node.classList.add("rv");
    const io = new IntersectionObserver(
      (es) => es.forEach((e) => { if (e.isIntersecting) { node.classList.add("rv-in"); io.disconnect(); } }),
      { threshold: 0.15 },
    );
    io.observe(node);
    return { destroy: () => io.disconnect() };
  }
</script>

<svelte:head>
  <title>{t.meta.title}</title>
  <meta name="description" content={t.meta.description} />
  <meta property="og:title" content={t.meta.title} />
  <meta property="og:description" content={t.meta.description} />
  <meta property="og:type" content="website" />
  <meta property="og:url" content="https://olharai.com" />
</svelte:head>

<SensorField />

<!-- ===== nav ===== -->
<header class="sticky top-0 z-50 border-b border-transparent backdrop-blur-xl transition-colors duration-300 data-[s=true]:border-border data-[s=true]:bg-background/70"
  data-s="false"
  bind:this={hdr}>
  <div class="mx-auto flex h-16 w-full max-w-6xl items-center gap-7 px-6">
    <a href="/" class="flex items-center gap-2.5 font-semibold tracking-tight">
      <OlharMark label="" size={26} class="text-primary drop-shadow-[0_0_10px_oklch(1_0_0/0.35)]" />
      <span>Olhar AI</span>
    </a>
    <nav class="ml-3 hidden gap-7 text-sm font-medium text-muted-foreground md:flex">
      <a class="transition-colors hover:text-foreground" href="#features">{t.nav.features}</a>
      <a class="transition-colors hover:text-foreground" href="#use-cases">{t.nav.useCases}</a>
      <a class="transition-colors hover:text-foreground" href="#how">{t.nav.how}</a>
      <a class="transition-colors hover:text-foreground" href="#privacy">{t.nav.privacy}</a>
    </nav>
    <div class="ml-auto flex items-center gap-3">
      <!-- language toggle -->
      <div class="flex items-center rounded-lg border border-border p-0.5 text-[11px] font-bold">
        {#each langs as l (l)}
          <button
            type="button"
            onclick={() => setLang(l)}
            aria-pressed={lang() === l}
            class="rounded-md px-2 py-1 uppercase tracking-wide transition-colors"
            class:bg-primary={lang() === l}
            class:text-primary-foreground={lang() === l}
            class:text-muted-foreground={lang() !== l}
            class:hover:text-foreground={lang() !== l}
          >{l}</button>
        {/each}
      </div>
      <a class="hidden text-sm font-medium text-muted-foreground transition-colors hover:text-foreground sm:block" href={APP}>{t.header.signIn}</a>
      <a class="press inline-flex items-center gap-2 rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-primary-foreground shadow-[0_8px_30px_-8px_oklch(1_0_0/0.35)] transition-transform hover:-translate-y-0.5" href={APP}>{t.header.getStarted}</a>
    </div>
  </div>
</header>

<main class="mx-auto w-full max-w-6xl px-6">
  <!-- ===== hero ===== -->
  <section class="relative overflow-hidden pt-28 pb-24 md:pt-36">
    <!-- the living vision-network — nodes wired together, signals converging on the eye -->
    <HeroNet />
    <!-- focal bloom: the eye reads as the luminous point the whole network feeds into -->
    <div class="pointer-events-none absolute left-1/2 top-[30%] h-[34rem] w-[34rem] -translate-x-1/2 -translate-y-1/2 rounded-full"
      style="background:radial-gradient(circle, var(--shimmer), transparent 66%);"></div>
    <div class="relative z-10 flex flex-col items-center text-center">
      <!-- the mark, at the centre of concentric signal pings — the network's focal node -->
      <div class="relative mb-9 grid place-items-center">
        <span class="ping-ring absolute size-32 rounded-full border border-primary/25"></span>
        <span class="ping-ring absolute size-32 rounded-full border border-primary/20" style="animation-delay:1.05s"></span>
        <span class="ping-ring absolute size-32 rounded-full border border-primary/15" style="animation-delay:2.1s"></span>
        <OlharMark label="" size={94} class="relative text-primary drop-shadow-[0_0_34px_oklch(1_0_0/0.4)]" />
      </div>

      <span use:reveal class="inline-flex items-center gap-2 rounded-full border border-primary/25 bg-primary/10 px-3.5 py-1.5 text-[11px] font-semibold uppercase tracking-[0.12em] text-primary backdrop-blur-sm">
        <span class="status-live size-1.5 rounded-full bg-primary"></span>
        {t.hero.badge}
      </span>

      <h1 use:reveal={60} class="mt-7 max-w-3xl text-[2.75rem] font-extrabold leading-[1.02] tracking-tight sm:text-6xl md:text-7xl">
        {t.hero.title1}<br /><span class="text-grad">{t.hero.titleAccent}</span>
      </h1>

      <p use:reveal={120} class="mt-7 max-w-xl text-lg leading-relaxed text-muted-foreground">
        {t.hero.subtitle}
      </p>

      <div use:reveal={180} class="mt-10 flex flex-wrap items-center justify-center gap-3.5">
        <a class="press group inline-flex items-center gap-2 rounded-xl bg-primary px-7 py-3.5 text-[15px] font-semibold text-primary-foreground shadow-[0_12px_40px_-12px_oklch(1_0_0/0.3)] transition-transform hover:-translate-y-0.5" href={APP}>
          {t.hero.ctaStart} <ArrowRight class="size-4 transition-transform group-hover:translate-x-0.5" />
        </a>
        <a class="press inline-flex items-center gap-2 rounded-xl border border-border bg-card/40 px-7 py-3.5 text-[15px] font-semibold backdrop-blur transition-colors hover:border-primary/40" href="#how">
          {t.hero.ctaHow}
        </a>
      </div>

      <p use:reveal={240} class="mt-6 flex items-center gap-2 text-[13px] text-muted-foreground/80">
        <Check class="size-4 text-primary" /> {t.hero.trust}
      </p>

      <!-- spaces strip -->
      <div use:reveal={120} class="mt-20">
        <p class="text-[11px] uppercase tracking-[0.16em] text-muted-foreground/60">{t.hero.spacesLabel}</p>
        <div class="mt-5 flex flex-wrap items-center justify-center gap-x-8 gap-y-3 text-sm font-medium text-muted-foreground/75">
          {#each t.hero.spaces as space (space)}
            <span class="inline-flex items-center gap-2">
              <span class="size-1 rounded-full bg-muted-foreground/40"></span>{space}
            </span>
          {/each}
        </div>
      </div>
    </div>
  </section>

  <!-- ===== features ===== -->
  <section id="features" class="py-24">
    <div use:reveal class="mx-auto mb-14 max-w-2xl text-center">
      <span class="text-xs font-semibold uppercase tracking-[0.14em] text-primary">{t.features.eyebrow}</span>
      <h2 class="mt-3 text-3xl font-extrabold tracking-tight md:text-[2.6rem]">{t.features.title}</h2>
      <p class="mt-4 text-lg text-muted-foreground">{t.features.subtitle}</p>
    </div>
    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {#each t.features.items as f, i (i)}
        {@const Icon = featureIcons[i]}
        <div use:reveal={(i % 3) * 80} class="interactive rounded-2xl border border-border bg-card/40 p-6 backdrop-blur-sm">
          <div class="mb-4 grid size-11 place-items-center rounded-xl bg-primary/10 text-primary ring-1 ring-inset ring-primary/20">
            <Icon class="size-5" />
          </div>
          <h3 class="text-[17px] font-bold tracking-tight">{f.title}</h3>
          <p class="mt-2 text-[14.5px] leading-relaxed text-muted-foreground">{f.body}</p>
        </div>
      {/each}
    </div>
  </section>

  <!-- ===== use cases (real workflows → templates) ===== -->
  <section id="use-cases" class="py-24">
    <div use:reveal class="mx-auto mb-14 max-w-2xl text-center">
      <span class="text-xs font-semibold uppercase tracking-[0.14em] text-primary">{t.useCases.eyebrow}</span>
      <h2 class="mt-3 text-3xl font-extrabold tracking-tight md:text-[2.6rem]">{t.useCases.title}</h2>
      <p class="mt-4 text-lg text-muted-foreground">{t.useCases.subtitle}</p>
    </div>
    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {#each t.useCases.items as u, i (i)}
        {@const Icon = useCaseIcons[i]}
        <div use:reveal={(i % 3) * 80} class="interactive group relative overflow-hidden rounded-2xl border border-border bg-card/40 p-6 backdrop-blur-sm">
          <div class="mb-4 flex items-center gap-3">
            <div class="grid size-10 place-items-center rounded-xl bg-primary/10 text-primary ring-1 ring-inset ring-primary/20">
              <Icon class="size-[18px]" />
            </div>
            <span class="rounded-full border border-border px-2.5 py-0.5 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">{u.tag}</span>
          </div>
          <h3 class="text-[16px] font-bold tracking-tight">{u.title}</h3>
          <p class="mt-2 text-[14px] leading-relaxed text-muted-foreground">{u.body}</p>
        </div>
      {/each}
    </div>
  </section>

  <!-- ===== the product, in screenshots ===== -->
  <!-- SCREENSHOT SLOTS. These render captioned placeholders until real captures are dropped into
       landing/static/shots/. See docs/plans/olharai-landing.md for the exact capture list. Each slot
       shows a real screenshot once its file exists; keep the aspect ratio so the layout doesn't jump. -->
  <section id="see-it" class="py-24">
    <div use:reveal class="mx-auto mb-14 max-w-2xl text-center">
      <span class="text-xs font-semibold uppercase tracking-[0.14em] text-primary">{t.shots.eyebrow}</span>
      <h2 class="mt-3 text-3xl font-extrabold tracking-tight md:text-[2.6rem]">{t.shots.title}</h2>
    </div>
    <div class="grid gap-4 md:grid-cols-2">
      {#each t.shots.items as sh, i (i)}
        {@const file = shotFiles[i]}
        <figure use:reveal={(i % 2) * 90} class="overflow-hidden rounded-2xl border border-border bg-card/40 backdrop-blur-sm">
          <div class="relative aspect-[16/10] overflow-hidden border-b border-border bg-gradient-to-br from-card to-background">
            <!-- placeholder until the capture lands -->
            <div class="absolute inset-0 grid place-items-center text-center">
              <div class="space-y-2 px-6">
                <OlharMark label="" size={40} class="mx-auto text-primary/40" />
                <p class="font-mono text-xs text-muted-foreground/60">{file}</p>
              </div>
            </div>
            <span class="absolute left-3 top-3 size-4 border-l-2 border-t-2 border-primary/30"></span>
            <span class="absolute bottom-3 right-3 size-4 border-b-2 border-r-2 border-primary/30"></span>
          </div>
          <figcaption class="p-5">
            <h3 class="text-[15px] font-bold tracking-tight">{sh.title}</h3>
            <p class="mt-1 text-[13.5px] leading-relaxed text-muted-foreground">{sh.body}</p>
          </figcaption>
        </figure>
      {/each}
    </div>
  </section>

  <!-- ===== how it works ===== -->
  <section id="how" class="py-24">
    <div use:reveal class="mx-auto mb-14 max-w-2xl text-center">
      <span class="text-xs font-semibold uppercase tracking-[0.14em] text-primary">{t.how.eyebrow}</span>
      <h2 class="mt-3 text-3xl font-extrabold tracking-tight md:text-[2.6rem]">{t.how.title}</h2>
    </div>
    <div class="grid gap-4 md:grid-cols-3">
      {#each t.how.steps as s, i (i)}
        <div use:reveal={i * 90} class="relative rounded-2xl border border-border bg-card/40 p-7 backdrop-blur-sm">
          <div class="flex items-center gap-3">
            <span class="font-mono text-sm font-semibold text-primary tabular-nums">{stepNums[i]}</span>
            <span class="h-px flex-1 bg-gradient-to-r from-primary/50 to-transparent"></span>
          </div>
          <h3 class="mt-4 text-[17px] font-bold tracking-tight">{s.title}</h3>
          <p class="mt-2 text-[14.5px] leading-relaxed text-muted-foreground">{s.body}</p>
        </div>
      {/each}
    </div>
  </section>

  <!-- ===== privacy ===== -->
  <section id="privacy" class="py-24">
    <div use:reveal class="relative overflow-hidden rounded-3xl border border-primary/20 p-10 text-center md:p-14"
      style="background:radial-gradient(600px 320px at 50% 0%, var(--shimmer), transparent 70%), var(--elev-3);">
      <div class="mb-5 grid place-items-center">
        <OlharMark label="" size={48} class="text-primary drop-shadow-[0_0_18px_oklch(1_0_0/0.35)]" />
      </div>
      <span class="text-xs font-semibold uppercase tracking-[0.14em] text-primary">{t.privacy.eyebrow}</span>
      <h2 class="mx-auto mt-3 max-w-xl text-3xl font-extrabold tracking-tight md:text-[2.4rem]">{t.privacy.title}</h2>
      <p class="mx-auto mt-4 max-w-lg text-[17px] text-muted-foreground">
        {t.privacy.body}
      </p>
      <div class="mt-7 flex flex-wrap justify-center gap-3">
        {#each t.privacy.chips as chip, i (i)}
          {@const Icon = privacyIcons[i]}
          <span class="inline-flex items-center gap-2 rounded-full border border-primary/25 bg-primary/10 px-3.5 py-2 text-[13px] font-semibold text-primary">
            <Icon class="size-3.5" /> {chip}
          </span>
        {/each}
      </div>
    </div>
  </section>

  <!-- ===== final CTA ===== -->
  <section class="py-28 text-center">
    <div use:reveal class="flex flex-col items-center">
      <div class="relative mb-7 grid place-items-center">
        <span class="ping-ring absolute size-20 rounded-full border border-primary/30"></span>
        <OlharMark label="" size={52} class="relative text-primary" />
      </div>
      <h2 class="mx-auto max-w-xl text-4xl font-extrabold tracking-tight md:text-5xl">{t.cta.title}</h2>
      <p class="mx-auto mt-4 max-w-md text-lg text-muted-foreground">{t.cta.subtitle}</p>
      <div class="mt-8 flex flex-wrap items-center justify-center gap-3.5">
        <a class="press inline-flex items-center gap-2 rounded-xl bg-primary px-7 py-4 text-base font-semibold text-primary-foreground shadow-[0_10px_36px_-10px_oklch(1_0_0/0.25)] transition-transform hover:-translate-y-0.5" href={APP}>
          {t.cta.start} <ArrowRight class="size-4" />
        </a>
        <a class="press inline-flex items-center rounded-xl border border-border bg-card/40 px-7 py-4 text-base font-semibold backdrop-blur transition-colors hover:border-primary/40" href={APP}>{t.cta.signIn}</a>
      </div>
    </div>
  </section>
</main>

<!-- ===== footer ===== -->
<footer class="border-t border-border">
  <div class="mx-auto flex w-full max-w-6xl flex-wrap items-center gap-4 px-6 py-10">
    <a href="/" class="flex items-center gap-2 font-semibold">
      <OlharMark label="" size={22} animated={false} class="text-primary" /> Olhar AI
    </a>
    <span class="text-[13px] text-muted-foreground/70">{t.footer.copyright}</span>
    <div class="ml-auto flex gap-6 text-[13px] text-muted-foreground">
      <a class="transition-colors hover:text-foreground" href={APP}>{t.footer.app}</a>
      <a class="transition-colors hover:text-foreground" href="#features">{t.footer.features}</a>
      <a class="transition-colors hover:text-foreground" href="#privacy">{t.footer.privacy}</a>
    </div>
  </div>
</footer>

<style>
  /* sticky-nav border-on-scroll handled with a tiny inline observer below */
  :global(.rv) {
    opacity: 0;
    transform: translateY(18px);
    transition:
      opacity 0.7s cubic-bezier(0.22, 1, 0.36, 1),
      transform 0.7s cubic-bezier(0.22, 1, 0.36, 1);
  }
  :global(.rv-in) {
    opacity: 1;
    transform: none;
  }
  @media (prefers-reduced-motion: reduce) {
    :global(.rv) {
      opacity: 1;
      transform: none;
    }
  }
</style>
