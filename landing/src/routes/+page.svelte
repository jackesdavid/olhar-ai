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
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import KeyRound from "@lucide/svelte/icons/key-round";

  import OlharMark from "$lib/components/olhar-mark.svelte";
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

  // Capabilities, grounded in what actually ships — person/object/face detection, live tracking, the
  // AI vision + chat, the 65-node visual workflow engine, event-triggered recording, notifications.
  const features = [
    { icon: Eye, title: "Sees what's there", body: "Person, object, and face detection on every camera — running locally on your edge, not a distant server. It knows a person from a passing car from a delivery van." },
    { icon: ScanFace, title: "Knows who belongs", body: "Enroll the people who matter once. Olhar AI recognises family, staff, and regulars — and flags the faces it doesn't." },
    { icon: Workflow, title: "Automates anything", body: "A visual builder with 65 blocks — detect, track, decide, record, notify, ask an AI. Wire them together: “if an unknown person is at the back gate after dark, record it and message me.” No code." },
    { icon: Video, title: "Records what counts", body: "Event-triggered clips with pre-roll, so you catch the seconds before it happened — not endless footage of an empty driveway. Kept local or in the cloud, your choice." },
    { icon: BellRing, title: "Tells you, your way", body: "A clear message to your phone or Telegram — with the annotated snapshot — only for the things you asked about. Or ask your cameras a question and the assistant answers." },
    { icon: MapPinned, title: "Every space, one place", body: "Track who's where on a live floor plan. Map cameras to rooms and sites, and run a home, a shop, and an office from one calm dashboard." },
  ];

  // Real workflows from the product, shown as the templates a new user starts from.
  const useCases = [
    { icon: BellRing, tag: "Alerting", title: "Alert me when someone's at the gate", body: "A person is detected, a box is drawn around them, and a Telegram message lands with the snapshot. Quiet until it matters." },
    { icon: Video, tag: "Recording", title: "Record only when it matters", body: "Motion or a detected person opens a clip — with the seconds before it started — and it closes on its own when the scene goes quiet." },
    { icon: MessageSquare, tag: "AI assistant", title: "Ask your cameras anything", body: "Message the assistant on Telegram — “what's happening in the shop?” — and it looks at the live frame and tells you, in plain words." },
    { icon: Eye, tag: "Tracking", title: "Track everything, live", body: "Follow people and objects across a camera in real time, boxes and labels drawn on the feed, positions plotted on your floor plan." },
    { icon: ScanFace, tag: "Access", title: "Know who belongs", body: "Recognise the faces you enrolled, and raise a flag on the ones you didn't — at the front door, the stockroom, the school gate." },
    { icon: Workflow, tag: "Custom", title: "Or build your own", body: "Every one above is a workflow you can open, change, and extend. 65 blocks, your logic, your cameras — start from a template or a blank canvas." },
  ];

  const steps = [
    { n: "01", title: "Connect your cameras", body: "Install the Olhar AI edge on a small box on-site. It finds your cameras and brings them online — securely, over a private network. No cloud can see them but you." },
    { n: "02", title: "Pick what it should do", body: "Start from a template — alert on a person, record on motion, recognise faces — or wire your own in the visual builder. Tell it which cameras and which people matter." },
    { n: "03", title: "Let it watch", body: "The live view when you want it, a clear alert when you need it, a clip when something happens. Add automations as your needs grow — no code, ever." },
  ];

  // Screenshot slots. `src` is set once a capture is dropped into landing/static/shots/ — until then
  // the slot renders a captioned placeholder (never a broken image). `file` is the exact path to save
  // the capture to; the capture list also lives in docs/plans/olharai-landing.md.
  const shots = [
    { file: "/shots/workflow-editor.png", src: "", title: "The visual builder", body: "Wire detection, logic, recording and notifications into a workflow — 65 blocks, no code. This is where every template above is made." },
    { file: "/shots/live-grid.png",       src: "", title: "Live, all at once", body: "Every camera in one grid, streamed privately from your edge — with a real LIVE badge that only lights when video is actually flowing." },
    { file: "/shots/map-presence.png",    src: "", title: "Who's where, on a floor plan", body: "A top-down blueprint of your space with people and detections plotted live as they move between rooms." },
    { file: "/shots/tracking-overlay.png",src: "", title: "Detection & tracking, drawn on the feed", body: "Boxes and labels on the people and objects it sees, tracked frame to frame — the raw material your automations act on." },
  ];
</script>

<svelte:head>
  <title>Olhar AI — cameras that understand and act</title>
  <meta name="description" content="Olhar AI turns your cameras into a vision platform — person, object and face detection, live tracking, event-triggered recording, a visual automation engine, and an AI you can ask. Private, on your own edge. For home and business." />
  <meta property="og:title" content="Olhar AI — cameras that understand and act" />
  <meta property="og:description" content="Detection, tracking, recording, and a 65-block visual automation engine — running on your own edge. Cameras that understand and act." />
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
      <OlharMark label="" size={26} class="text-primary drop-shadow-[0_0_10px_oklch(1_0_0/0.35)]" />
      <span>Olhar AI</span>
    </a>
    <nav class="ml-3 hidden gap-7 text-sm font-medium text-muted-foreground md:flex">
      <a class="transition-colors hover:text-foreground" href="#features">Features</a>
      <a class="transition-colors hover:text-foreground" href="#use-cases">Use cases</a>
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
      <OlharMark label="" size={78} class="relative text-primary drop-shadow-[0_0_24px_oklch(1_0_0/0.3)]" />
    </div>

    <span use:reveal class="inline-flex items-center gap-2 rounded-full border border-primary/25 bg-primary/10 px-3.5 py-1.5 text-xs font-semibold text-primary">
      <span class="status-live size-1.5 rounded-full bg-primary"></span>
      Cameras that understand
    </span>

    <h1 use:reveal={60} class="mt-6 max-w-3xl text-5xl font-extrabold leading-[1.04] tracking-tight md:text-7xl">
      Cameras that understand,<br /><span class="text-grad">and act.</span>
    </h1>

    <p use:reveal={120} class="mt-6 max-w-xl text-lg text-muted-foreground">
      Olhar AI gives your cameras eyes and a brain. They detect people, objects, and faces,
      track what moves, record what matters, and act on what they see — with a visual builder
      you drive, and an AI you can just ask. Private, on your own edge. For home and business.
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
      <span class="text-sm font-bold tracking-wide text-primary">A platform, not just a camera app</span>
      <h2 class="mt-3 text-3xl font-extrabold tracking-tight md:text-[2.6rem]">Cameras that do the watching — and the thinking</h2>
      <p class="mt-4 text-lg text-muted-foreground">From a live grid to detection, tracking, recording, and automation — it's one system, and it's yours.</p>
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

  <!-- ===== use cases (real workflows → templates) ===== -->
  <section id="use-cases" class="py-24">
    <div use:reveal class="mx-auto mb-14 max-w-2xl text-center">
      <span class="text-sm font-bold tracking-wide text-primary">Start from a template</span>
      <h2 class="mt-3 text-3xl font-extrabold tracking-tight md:text-[2.6rem]">What people build with it</h2>
      <p class="mt-4 text-lg text-muted-foreground">Every one is a ready-made workflow you can open and change — or the starting point for your own.</p>
    </div>
    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {#each useCases as u, i (u.title)}
        {@const Icon = u.icon}
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
      <span class="text-sm font-bold tracking-wide text-primary">See it working</span>
      <h2 class="mt-3 text-3xl font-extrabold tracking-tight md:text-[2.6rem]">One calm surface for everything your cameras see</h2>
    </div>
    <div class="grid gap-4 md:grid-cols-2">
      {#each shots as sh, i (sh.title)}
        <figure use:reveal={(i % 2) * 90} class="overflow-hidden rounded-2xl border border-border bg-card/40 backdrop-blur-sm">
          <div class="relative aspect-[16/10] overflow-hidden border-b border-border bg-gradient-to-br from-card to-background">
            {#if sh.src}
              <img src={sh.src} alt={sh.title} class="size-full object-cover" loading="lazy" />
            {:else}
              <!-- placeholder until the capture lands -->
              <div class="absolute inset-0 grid place-items-center text-center">
                <div class="space-y-2 px-6">
                  <OlharMark label="" size={40} class="mx-auto text-primary/40" />
                  <p class="font-mono text-xs text-muted-foreground/60">{sh.file}</p>
                </div>
              </div>
              <span class="absolute left-3 top-3 size-4 border-l-2 border-t-2 border-primary/30"></span>
              <span class="absolute bottom-3 right-3 size-4 border-b-2 border-r-2 border-primary/30"></span>
            {/if}
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
      <span class="text-sm font-bold tracking-wide text-primary">Up and running in minutes</span>
      <h2 class="mt-3 text-3xl font-extrabold tracking-tight md:text-[2.6rem]">Three steps to cameras that act</h2>
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
      style="background:radial-gradient(600px 320px at 50% 0%, var(--shimmer), transparent 70%), var(--elev-3);">
      <div class="mb-5 grid place-items-center">
        <OlharMark label="" size={48} class="text-primary drop-shadow-[0_0_18px_oklch(1_0_0/0.35)]" />
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
        <OlharMark label="" size={52} class="relative text-primary" />
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
      <OlharMark label="" size={22} animated={false} class="text-primary" /> Olhar AI
    </a>
    <span class="text-[13px] text-muted-foreground/70">© 2026 Olhar AI · Calm, private security intelligence</span>
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

  /* live-feed mock: dot-grid (the halftone) + a soft scanning sweep.
     The "screen" is recessed — elev-1 → elev-0 — so it reads as a display sunk
     into its panel frame, not as another card. */
  .feed {
    background:
      radial-gradient(oklch(1 0 0 / 0.04) 1px, transparent 1.4px) 0 0 / 22px 22px,
      radial-gradient(120% 80% at 50% 0%, var(--elev-1), var(--elev-0));
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
