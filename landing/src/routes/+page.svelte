<script lang="ts">
  import { onMount } from "svelte";
  import ScanFace from "@lucide/svelte/icons/scan-face";
  import Video from "@lucide/svelte/icons/video";
  import BellRing from "@lucide/svelte/icons/bell-ring";
  import Workflow from "@lucide/svelte/icons/workflow";
  import Cpu from "@lucide/svelte/icons/cpu";
  import MapPinned from "@lucide/svelte/icons/map-pinned";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Check from "@lucide/svelte/icons/check";
  import Lock from "@lucide/svelte/icons/lock";
  import Eye from "@lucide/svelte/icons/eye";
  import KeyRound from "@lucide/svelte/icons/key-round";

  import SentinelMark from "$lib/components/sentinel-mark.svelte";
  import SensorField from "$lib/components/sensor-field.svelte";

  const APP = "https://app.thesentinels.online";

  // Sticky nav: warm the border once the page scrolls a touch.
  let hdr = $state<HTMLElement>();
  onMount(() => {
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

  const features = [
    { icon: ScanFace, title: "Knows who belongs", body: "Enroll the people who matter once. Sentinel recognises family, staff, and regulars — and flags the faces it doesn't." },
    { icon: Video, title: "Private live feeds", body: "Watch any camera in real time from anywhere, over an encrypted private link. Your video never sits on a public server." },
    { icon: BellRing, title: "Alerts that matter", body: "No more endless motion pings. A clear message — to your phone or Telegram — only for the things you asked to hear about." },
    { icon: Workflow, title: "Visual automations", body: "Drag-and-drop rules: if an unknown person appears at the back gate after dark, snap a photo and alert me. No code." },
    { icon: MapPinned, title: "Every space, one place", body: "Map cameras to rooms and sites. Run a home, a shop, and an office from a single calm dashboard." },
    { icon: Cpu, title: "Runs on your edge", body: "A small on-prem appliance does the heavy lifting next to your cameras — fast, resilient, and yours. The cloud just keeps you connected." },
  ];

  const steps = [
    { n: "01", title: "Connect your cameras", body: "Install the Sentinel edge on a small box on-site. It finds your cameras and brings them online — securely, over a private network." },
    { n: "02", title: "Teach it who matters", body: "Add the people you trust and the zones you care about. Sentinel starts recognising faces and watching the right corners." },
    { n: "03", title: "Relax", body: "Get the live view when you want it and a clear alert when you need it. Build automations as your needs grow." },
  ];
</script>

<svelte:head>
  <title>Sentinel — calm, private security intelligence</title>
  <meta name="description" content="Sentinel turns your cameras into a calm, private security assistant — AI that knows who belongs, private live feeds, alerts that matter, and visual automations. For home and business." />
  <meta property="og:title" content="Sentinel — calm, private security intelligence" />
  <meta property="og:description" content="Your cameras, made calm. On-prem edge, AI recognition, private live feeds, smart alerts — private by design." />
  <meta property="og:type" content="website" />
  <meta property="og:url" content="https://thesentinels.online" />
</svelte:head>

<SensorField />

<!-- ===== nav ===== -->
<header class="sticky top-0 z-50 border-b border-transparent backdrop-blur-xl transition-colors duration-300 data-[s=true]:border-border data-[s=true]:bg-background/70"
  data-s="false"
  bind:this={hdr}>
  <div class="mx-auto flex h-16 w-full max-w-6xl items-center gap-7 px-6">
    <a href="/" class="flex items-center gap-2.5 font-semibold tracking-tight">
      <SentinelMark size={26} class="text-primary drop-shadow-[0_0_10px_oklch(1_0_0/0.35)]" />
      <span>Sentinel</span>
    </a>
    <nav class="ml-3 hidden gap-7 text-sm font-medium text-muted-foreground md:flex">
      <a class="transition-colors hover:text-foreground" href="#features">Features</a>
      <a class="transition-colors hover:text-foreground" href="#how">How it works</a>
      <a class="transition-colors hover:text-foreground" href="#privacy">Privacy</a>
    </nav>
    <div class="ml-auto flex items-center gap-3">
      <a class="hidden text-sm font-medium text-muted-foreground transition-colors hover:text-foreground sm:block" href={APP}>Sign in</a>
      <a class="press inline-flex items-center gap-2 rounded-xl bg-primary px-4 py-2 text-sm font-semibold text-primary-foreground shadow-[0_8px_30px_-8px_oklch(1_0_0/0.35)] transition-transform hover:-translate-y-0.5" href={APP}>Get started</a>
    </div>
  </div>
</header>

<main class="mx-auto w-full max-w-6xl px-6">
  <!-- ===== hero ===== -->
  <section class="relative flex flex-col items-center pt-24 pb-16 text-center md:pt-32">
    <!-- the mark, wrapped in concentric signal pings -->
    <div class="relative mb-8 grid place-items-center">
      <span class="ping-ring absolute size-28 rounded-full border border-primary/30" style="animation-delay:0s"></span>
      <span class="ping-ring absolute size-28 rounded-full border border-primary/20" style="animation-delay:1.6s"></span>
      <SentinelMark size={78} class="relative text-primary drop-shadow-[0_0_24px_oklch(1_0_0/0.3)]" />
    </div>

    <span use:reveal class="inline-flex items-center gap-2 rounded-full border border-primary/25 bg-primary/10 px-3.5 py-1.5 text-xs font-semibold text-primary">
      <span class="status-live size-1.5 rounded-full bg-primary"></span>
      Calm security intelligence
    </span>

    <h1 use:reveal={60} class="mt-6 max-w-3xl text-5xl font-extrabold leading-[1.04] tracking-tight md:text-7xl">
      Your cameras,<br /><span class="text-grad">made calm.</span>
    </h1>

    <p use:reveal={120} class="mt-6 max-w-xl text-lg text-muted-foreground">
      Sentinel turns ordinary cameras into a private security assistant — it recognises the
      people who matter, watches for what doesn't belong, and tells you only when it counts.
      For your home and your business.
    </p>

    <div use:reveal={180} class="mt-9 flex flex-wrap items-center justify-center gap-3.5">
      <a class="press inline-flex items-center gap-2 rounded-xl bg-primary px-6 py-3.5 text-[15px] font-semibold text-primary-foreground shadow-[0_10px_36px_-10px_oklch(1_0_0/0.25)] transition-transform hover:-translate-y-0.5" href={APP}>
        Start free <ArrowRight class="size-4" />
      </a>
      <a class="press inline-flex items-center gap-2 rounded-xl border border-border bg-card/40 px-6 py-3.5 text-[15px] font-semibold backdrop-blur transition-colors hover:border-primary/40" href="#how">
        See how it works
      </a>
    </div>

    <p use:reveal={240} class="mt-5 flex items-center gap-2 text-[13px] text-muted-foreground/80">
      <Check class="size-4 text-primary" /> Private by design · runs on your own edge · no card to start
    </p>

    <!-- ===== product preview: a live feed mock ===== -->
    <div use:reveal={300} class="mt-16 w-full max-w-4xl">
      <div class="rounded-2xl border border-border bg-card/50 p-3 shadow-[0_50px_140px_-50px_oklch(0_0_0/0.85)] backdrop-blur-xl">
        <div class="flex items-center gap-2 px-2 py-2.5">
          <span class="size-2.5 rounded-full bg-muted-foreground/30"></span>
          <span class="size-2.5 rounded-full bg-muted-foreground/30"></span>
          <span class="size-2.5 rounded-full bg-muted-foreground/30"></span>
          <span class="ml-2 font-mono text-xs text-muted-foreground">front_entrance · live</span>
          <span class="ml-auto flex items-center gap-1.5 text-[11px] font-semibold text-primary">
            <span class="status-live size-1.5 rounded-full bg-primary"></span> LIVE
          </span>
        </div>
        <div class="feed relative aspect-[16/9] overflow-hidden rounded-xl border border-border">
          <span class="sweep absolute inset-y-0 w-2/5"></span>
          <!-- corner ticks -->
          <span class="absolute left-3 top-3 size-4 border-l-2 border-t-2 border-primary/50"></span>
          <span class="absolute right-3 top-3 size-4 border-r-2 border-t-2 border-primary/50"></span>
          <span class="absolute bottom-3 left-3 size-4 border-b-2 border-l-2 border-primary/50"></span>
          <span class="absolute bottom-3 right-3 size-4 border-b-2 border-r-2 border-primary/50"></span>
          <!-- detection -->
          <div class="absolute left-[26%] top-[22%] h-[58%] w-[30%] rounded-lg border-[1.5px] border-primary shadow-[0_0_28px_-4px_oklch(1_0_0/0.35)]">
            <span class="absolute -top-3 left-0 -translate-y-full rounded-md bg-primary px-2 py-1 text-[11px] font-bold text-primary-foreground whitespace-nowrap">Recognised · Maya</span>
            <span class="absolute -bottom-3 right-0 translate-y-full font-mono text-[10.5px] text-primary tabular-nums">99.2%</span>
            <span class="ping-ring absolute left-1/2 top-1/2 size-10 -translate-x-1/2 -translate-y-1/2 rounded-full border border-primary/50"></span>
          </div>
        </div>
      </div>
    </div>

    <!-- spaces strip -->
    <div use:reveal={120} class="mt-14">
      <p class="text-[11px] uppercase tracking-[0.16em] text-muted-foreground/70">One calm assistant for every space</p>
      <div class="mt-4 flex flex-wrap items-center justify-center gap-x-8 gap-y-3 text-sm font-medium text-muted-foreground/80">
        <span>🏠 Homes</span><span>🏢 Offices</span><span>🏪 Retail</span><span>🏫 Schools</span><span>🏭 Sites</span>
      </div>
    </div>
  </section>

  <!-- ===== features ===== -->
  <section id="features" class="py-24">
    <div use:reveal class="mx-auto mb-14 max-w-2xl text-center">
      <span class="text-sm font-bold tracking-wide text-primary">Everything, quietly handled</span>
      <h2 class="mt-3 text-3xl font-extrabold tracking-tight md:text-[2.6rem]">Security that thinks, so you don't have to watch</h2>
      <p class="mt-4 text-lg text-muted-foreground">Sentinel does the watching. You get clarity — who's there, what changed, and what to do about it.</p>
    </div>
    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {#each features as f, i (f.title)}
        {@const Icon = f.icon}
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

  <!-- ===== how it works ===== -->
  <section id="how" class="py-24">
    <div use:reveal class="mx-auto mb-14 max-w-2xl text-center">
      <span class="text-sm font-bold tracking-wide text-primary">Up and running in minutes</span>
      <h2 class="mt-3 text-3xl font-extrabold tracking-tight md:text-[2.6rem]">Three steps to a calmer space</h2>
    </div>
    <div class="grid gap-4 md:grid-cols-3">
      {#each steps as s, i (s.n)}
        <div use:reveal={i * 90} class="relative rounded-2xl border border-border bg-card/40 p-7 backdrop-blur-sm">
          <div class="flex items-center gap-3">
            <span class="font-mono text-sm font-semibold text-primary tabular-nums">{s.n}</span>
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
      style="background:radial-gradient(600px 320px at 50% 0%, oklch(1 0 0 / 0.04), transparent 70%), oklch(0.11 0 0);">
      <div class="mb-5 grid place-items-center">
        <SentinelMark size={48} class="text-primary drop-shadow-[0_0_18px_oklch(1_0_0/0.35)]" />
      </div>
      <span class="text-sm font-bold tracking-wide text-primary">Private by design</span>
      <h2 class="mx-auto mt-3 max-w-xl text-3xl font-extrabold tracking-tight md:text-[2.4rem]">Your footage is yours. Full stop.</h2>
      <p class="mx-auto mt-4 max-w-lg text-[17px] text-muted-foreground">
        Cameras and recordings stay on your own edge. Live video travels over an encrypted
        private link — never a public stream. Even we can't watch your feeds.
      </p>
      <div class="mt-7 flex flex-wrap justify-center gap-3">
        {#each [{ i: Lock, t: "On-prem video" }, { i: ShieldCheck, t: "Encrypted private link" }, { i: Eye, t: "Tenant-private feeds" }, { i: KeyRound, t: "Role-based access" }] as p (p.t)}
          {@const Icon = p.i}
          <span class="inline-flex items-center gap-2 rounded-full border border-primary/25 bg-primary/10 px-3.5 py-2 text-[13px] font-semibold text-primary">
            <Icon class="size-3.5" /> {p.t}
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
        <SentinelMark size={52} class="relative text-primary" />
      </div>
      <h2 class="mx-auto max-w-xl text-4xl font-extrabold tracking-tight md:text-5xl">Bring calm to your cameras today</h2>
      <p class="mx-auto mt-4 max-w-md text-lg text-muted-foreground">Create your workspace in a minute. Add a camera, meet your new assistant.</p>
      <div class="mt-8 flex flex-wrap items-center justify-center gap-3.5">
        <a class="press inline-flex items-center gap-2 rounded-xl bg-primary px-7 py-4 text-base font-semibold text-primary-foreground shadow-[0_10px_36px_-10px_oklch(1_0_0/0.25)] transition-transform hover:-translate-y-0.5" href={APP}>
          Get started free <ArrowRight class="size-4" />
        </a>
        <a class="press inline-flex items-center rounded-xl border border-border bg-card/40 px-7 py-4 text-base font-semibold backdrop-blur transition-colors hover:border-primary/40" href={APP}>Sign in</a>
      </div>
    </div>
  </section>
</main>

<!-- ===== footer ===== -->
<footer class="border-t border-border">
  <div class="mx-auto flex w-full max-w-6xl flex-wrap items-center gap-4 px-6 py-10">
    <a href="/" class="flex items-center gap-2 font-semibold">
      <SentinelMark size={22} animated={false} class="text-primary" /> Sentinel
    </a>
    <span class="text-[13px] text-muted-foreground/70">© 2026 Sentinel · Calm, private security intelligence</span>
    <div class="ml-auto flex gap-6 text-[13px] text-muted-foreground">
      <a class="transition-colors hover:text-foreground" href={APP}>App</a>
      <a class="transition-colors hover:text-foreground" href="#features">Features</a>
      <a class="transition-colors hover:text-foreground" href="#privacy">Privacy</a>
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

  /* live-feed mock: dot-grid (the halftone) + a soft scanning sweep */
  .feed {
    background:
      radial-gradient(oklch(1 0 0 / 0.04) 1px, transparent 1.4px) 0 0 / 22px 22px,
      radial-gradient(120% 80% at 50% 0%, oklch(0.16 0 0), oklch(0.08 0 0));
  }
  .sweep {
    background: linear-gradient(90deg, transparent, oklch(1 0 0 / 0.06), transparent);
    animation: feed-sweep 4.5s cubic-bezier(0.45, 0, 0.55, 1) infinite;
  }
  @keyframes feed-sweep {
    0% {
      transform: translateX(-60%);
    }
    100% {
      transform: translateX(260%);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .sweep {
      animation: none;
    }
  }
</style>
