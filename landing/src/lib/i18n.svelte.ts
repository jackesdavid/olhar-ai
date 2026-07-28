// Lightweight bilingual (en/pt) i18n for the landing — no library. A single reactive
// `$state` language (persisted to localStorage + browser-detected on first visit),
// toggled from the header. Read the copy via `dict[lang()]` inside a `$derived` so the
// whole page re-renders on switch. Prerender bakes the default (en); the client swaps.

export type Lang = "en" | "pt";

let current = $state<Lang>("en");

/** Current language — reactive; read it inside a component/`$derived`. */
export function lang(): Lang {
  return current;
}

/** Switch language and persist the choice. */
export function setLang(l: Lang) {
  current = l;
  if (typeof localStorage !== "undefined") localStorage.setItem("lang", l);
  if (typeof document !== "undefined") document.documentElement.lang = l;
}

/** Resolve the initial language on the client: saved choice → browser → en. Call once on mount. */
export function initLang() {
  if (typeof window === "undefined") return;
  const saved = localStorage.getItem("lang");
  if (saved === "en" || saved === "pt") {
    setLang(saved);
    return;
  }
  setLang(navigator.language?.toLowerCase().startsWith("pt") ? "pt" : "en");
}

type Copy = {
  nav: { features: string; useCases: string; how: string; privacy: string };
  header: { signIn: string; getStarted: string };
  hero: {
    badge: string;
    title1: string;
    titleAccent: string;
    subtitle: string;
    ctaStart: string;
    ctaHow: string;
    trust: string;
    spacesLabel: string;
    spaces: string[];
  };
  features: { eyebrow: string; title: string; subtitle: string; items: { title: string; body: string }[] };
  useCases: { eyebrow: string; title: string; subtitle: string; items: { tag: string; title: string; body: string }[] };
  shots: { eyebrow: string; title: string; items: { title: string; body: string }[] };
  how: { eyebrow: string; title: string; steps: { title: string; body: string }[] };
  privacy: { eyebrow: string; title: string; body: string; chips: string[] };
  cta: { title: string; subtitle: string; start: string; signIn: string };
  footer: { copyright: string; app: string; features: string; privacy: string };
  meta: { title: string; description: string };
};

const en: Copy = {
  nav: { features: "Features", useCases: "Use cases", how: "How it works", privacy: "Privacy" },
  header: { signIn: "Sign in", getStarted: "Get started" },
  hero: {
    badge: "Cameras that understand",
    title1: "Cameras that understand,",
    titleAccent: "and act.",
    subtitle:
      "Olhar AI gives your cameras eyes and a brain. They detect people, objects, and faces, track what moves, record what matters, and act on what they see — with a visual builder you drive, and an AI you can just ask. Private, on your own edge. For home and business.",
    ctaStart: "Start free",
    ctaHow: "See how it works",
    trust: "Private by design · runs on your own edge · no card to start",
    spacesLabel: "One calm assistant for every space",
    spaces: ["Homes", "Offices", "Retail", "Schools", "Sites"],
  },
  features: {
    eyebrow: "A platform, not just a camera app",
    title: "Cameras that do the watching — and the thinking",
    subtitle: "From a live grid to detection, tracking, recording, and automation — it's one system, and it's yours.",
    items: [
      { title: "Sees what's there", body: "Person, object, and face detection on every camera — running locally on your edge, not a distant server. It knows a person from a passing car from a delivery van." },
      { title: "Knows who belongs", body: "Enroll the people who matter once. Olhar AI recognises family, staff, and regulars — and flags the faces it doesn't." },
      { title: "Automates anything", body: "A visual builder with 65 blocks — detect, track, decide, record, notify, ask an AI. Wire them together: “if an unknown person is at the back gate after dark, record it and message me.” No code." },
      { title: "Records what counts", body: "Event-triggered clips with pre-roll, so you catch the seconds before it happened — not endless footage of an empty driveway. Kept local or in the cloud, your choice." },
      { title: "Tells you, your way", body: "A clear message to your phone or Telegram — with the annotated snapshot — only for the things you asked about. Or ask your cameras a question and the assistant answers." },
      { title: "Every space, one place", body: "Track who's where on a live floor plan. Map cameras to rooms and sites, and run a home, a shop, and an office from one calm dashboard." },
    ],
  },
  useCases: {
    eyebrow: "Start from a template",
    title: "What people build with it",
    subtitle: "Every one is a ready-made workflow you can open and change — or the starting point for your own.",
    items: [
      { tag: "Alerting", title: "Alert me when someone's at the gate", body: "A person is detected, a box is drawn around them, and a Telegram message lands with the snapshot. Quiet until it matters." },
      { tag: "Recording", title: "Record only when it matters", body: "Motion or a detected person opens a clip — with the seconds before it started — and it closes on its own when the scene goes quiet." },
      { tag: "AI assistant", title: "Ask your cameras anything", body: "Message the assistant on Telegram — “what's happening in the shop?” — and it looks at the live frame and tells you, in plain words." },
      { tag: "Tracking", title: "Track everything, live", body: "Follow people and objects across a camera in real time, boxes and labels drawn on the feed, positions plotted on your floor plan." },
      { tag: "Access", title: "Know who belongs", body: "Recognise the faces you enrolled, and raise a flag on the ones you didn't — at the front door, the stockroom, the school gate." },
      { tag: "Custom", title: "Or build your own", body: "Every one above is a workflow you can open, change, and extend. 65 blocks, your logic, your cameras — start from a template or a blank canvas." },
    ],
  },
  shots: {
    eyebrow: "See it working",
    title: "One calm surface for everything your cameras see",
    items: [
      { title: "The visual builder", body: "Wire detection, logic, recording and notifications into a workflow — 65 blocks, no code. This is where every template above is made." },
      { title: "Live, all at once", body: "Every camera in one grid, streamed privately from your edge — with a real LIVE badge that only lights when video is actually flowing." },
      { title: "Who's where, on a floor plan", body: "A top-down blueprint of your space with people and detections plotted live as they move between rooms." },
      { title: "Detection & tracking, drawn on the feed", body: "Boxes and labels on the people and objects it sees, tracked frame to frame — the raw material your automations act on." },
    ],
  },
  how: {
    eyebrow: "Up and running in minutes",
    title: "Three steps to cameras that act",
    steps: [
      { title: "Connect your cameras", body: "Install the Olhar AI edge on a small box on-site. It finds your cameras and brings them online — securely, over a private network. No cloud can see them but you." },
      { title: "Pick what it should do", body: "Start from a template — alert on a person, record on motion, recognise faces — or wire your own in the visual builder. Tell it which cameras and which people matter." },
      { title: "Let it watch", body: "The live view when you want it, a clear alert when you need it, a clip when something happens. Add automations as your needs grow — no code, ever." },
    ],
  },
  privacy: {
    eyebrow: "Private by design",
    title: "Your footage is yours. Full stop.",
    body: "Cameras and recordings stay on your own edge. Live video travels over an encrypted private link — never a public stream. Even we can't watch your feeds.",
    chips: ["On-prem video", "Encrypted private link", "Tenant-private feeds", "Role-based access"],
  },
  cta: {
    title: "Bring calm to your cameras today",
    subtitle: "Create your workspace in a minute. Add a camera, meet your new assistant.",
    start: "Get started free",
    signIn: "Sign in",
  },
  footer: { copyright: "© 2026 Olhar AI · Calm, private security intelligence", app: "App", features: "Features", privacy: "Privacy" },
  meta: {
    title: "Olhar AI — cameras that understand and act",
    description:
      "Olhar AI turns your cameras into a vision platform — person, object and face detection, live tracking, event-triggered recording, a visual automation engine, and an AI you can ask. Private, on your own edge. For home and business.",
  },
};

const pt: Copy = {
  nav: { features: "Recursos", useCases: "Casos de uso", how: "Como funciona", privacy: "Privacidade" },
  header: { signIn: "Entrar", getStarted: "Começar" },
  hero: {
    badge: "Câmeras que entendem",
    title1: "Câmeras que entendem,",
    titleAccent: "e agem.",
    subtitle:
      "O Olhar AI dá olhos e cérebro às suas câmeras. Elas detectam pessoas, objetos e rostos, acompanham o que se move, gravam o que importa e agem sobre o que veem — com um construtor visual que você comanda e uma IA que você só precisa perguntar. Privado, no seu próprio edge. Para casa e empresa.",
    ctaStart: "Comece grátis",
    ctaHow: "Veja como funciona",
    trust: "Privado por design · roda no seu próprio edge · sem cartão para começar",
    spacesLabel: "Um assistente tranquilo para cada espaço",
    spaces: ["Casas", "Escritórios", "Varejo", "Escolas", "Indústrias"],
  },
  features: {
    eyebrow: "Uma plataforma, não só um app de câmera",
    title: "Câmeras que vigiam — e pensam",
    subtitle: "De uma grade ao vivo a detecção, rastreamento, gravação e automação — é um sistema só, e ele é seu.",
    items: [
      { title: "Vê o que está lá", body: "Detecção de pessoas, objetos e rostos em cada câmera — rodando localmente no seu edge, não num servidor distante. Ela distingue uma pessoa de um carro que passa e de uma van de entrega." },
      { title: "Sabe quem pertence", body: "Cadastre uma vez as pessoas que importam. O Olhar AI reconhece família, equipe e clientes habituais — e sinaliza os rostos que não conhece." },
      { title: "Automatiza qualquer coisa", body: "Um construtor visual com 65 blocos — detectar, rastrear, decidir, gravar, notificar, perguntar a uma IA. Conecte-os: “se uma pessoa desconhecida estiver no portão dos fundos depois do anoitecer, grave e me avise.” Sem código." },
      { title: "Grava o que conta", body: "Clipes disparados por evento com pré-roll, para você capturar os segundos antes do que aconteceu — não horas de uma garagem vazia. Guardados localmente ou na nuvem, você escolhe." },
      { title: "Avisa do seu jeito", body: "Uma mensagem clara no seu celular ou Telegram — com o snapshot anotado — só para as coisas que você pediu. Ou pergunte às suas câmeras e o assistente responde." },
      { title: "Cada espaço, num só lugar", body: "Veja quem está onde numa planta baixa ao vivo. Mapeie câmeras para cômodos e locais, e comande uma casa, uma loja e um escritório de um painel só, tranquilo." },
    ],
  },
  useCases: {
    eyebrow: "Comece por um modelo",
    title: "O que as pessoas constroem com ele",
    subtitle: "Cada um é um fluxo pronto que você pode abrir e mudar — ou o ponto de partida para o seu.",
    items: [
      { tag: "Alertas", title: "Me avise quando alguém estiver no portão", body: "Uma pessoa é detectada, uma caixa é desenhada ao redor dela e uma mensagem no Telegram chega com o snapshot. Quieto até importar." },
      { tag: "Gravação", title: "Grave só quando importa", body: "Movimento ou uma pessoa detectada abre um clipe — com os segundos antes de começar — e ele se fecha sozinho quando a cena fica quieta." },
      { tag: "Assistente IA", title: "Pergunte qualquer coisa às suas câmeras", body: "Mande mensagem ao assistente no Telegram — “o que está acontecendo na loja?” — e ele olha o quadro ao vivo e te conta, em palavras simples." },
      { tag: "Rastreamento", title: "Rastreie tudo, ao vivo", body: "Acompanhe pessoas e objetos por uma câmera em tempo real, caixas e rótulos desenhados no feed, posições plotadas na sua planta baixa." },
      { tag: "Acesso", title: "Saiba quem pertence", body: "Reconheça os rostos que você cadastrou e sinalize os que não — na porta da frente, no estoque, no portão da escola." },
      { tag: "Personalizado", title: "Ou construa o seu", body: "Cada um acima é um fluxo que você pode abrir, mudar e estender. 65 blocos, sua lógica, suas câmeras — comece de um modelo ou de uma tela em branco." },
    ],
  },
  shots: {
    eyebrow: "Veja funcionando",
    title: "Uma superfície tranquila para tudo o que suas câmeras veem",
    items: [
      { title: "O construtor visual", body: "Conecte detecção, lógica, gravação e notificações em um fluxo — 65 blocos, sem código. É aqui que cada modelo acima é feito." },
      { title: "Ao vivo, tudo de uma vez", body: "Cada câmera em uma grade, transmitida de forma privada do seu edge — com um selo AO VIVO de verdade que só acende quando há vídeo fluindo." },
      { title: "Quem está onde, na planta baixa", body: "Uma planta em vista superior do seu espaço, com pessoas e detecções plotadas ao vivo enquanto se movem entre os cômodos." },
      { title: "Detecção e rastreamento, desenhados no feed", body: "Caixas e rótulos nas pessoas e objetos que ela vê, rastreados quadro a quadro — a matéria-prima sobre a qual suas automações agem." },
    ],
  },
  how: {
    eyebrow: "No ar em minutos",
    title: "Três passos para câmeras que agem",
    steps: [
      { title: "Conecte suas câmeras", body: "Instale o edge do Olhar AI numa pequena caixa no local. Ele encontra suas câmeras e as coloca online — com segurança, por uma rede privada. Nenhuma nuvem as vê, só você." },
      { title: "Escolha o que ele deve fazer", body: "Comece por um modelo — alertar sobre uma pessoa, gravar em movimento, reconhecer rostos — ou monte o seu no construtor visual. Diga quais câmeras e quais pessoas importam." },
      { title: "Deixe-o observar", body: "A visão ao vivo quando você quer, um alerta claro quando você precisa, um clipe quando algo acontece. Adicione automações conforme sua necessidade cresce — sem código, nunca." },
    ],
  },
  privacy: {
    eyebrow: "Privado por design",
    title: "Suas imagens são suas. Ponto final.",
    body: "Câmeras e gravações ficam no seu próprio edge. O vídeo ao vivo trafega por um link privado criptografado — nunca um stream público. Nem nós conseguimos ver seus feeds.",
    chips: ["Vídeo on-premise", "Link privado criptografado", "Feeds privados por conta", "Acesso por papéis"],
  },
  cta: {
    title: "Traga tranquilidade às suas câmeras hoje",
    subtitle: "Crie seu workspace em um minuto. Adicione uma câmera, conheça seu novo assistente.",
    start: "Comece grátis",
    signIn: "Entrar",
  },
  footer: { copyright: "© 2026 Olhar AI · Inteligência de segurança tranquila e privada", app: "App", features: "Recursos", privacy: "Privacidade" },
  meta: {
    title: "Olhar AI — câmeras que entendem e agem",
    description:
      "O Olhar AI transforma suas câmeras numa plataforma de visão — detecção de pessoas, objetos e rostos, rastreamento ao vivo, gravação por evento, um motor de automação visual e uma IA que você pode perguntar. Privado, no seu próprio edge. Para casa e empresa.",
  },
};

export const dict: Record<Lang, Copy> = { en, pt };
