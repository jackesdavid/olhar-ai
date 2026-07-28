// The Playback timeline (/playback) — day picker, the camera stage, the transport bar and the
// multi-camera timeline with its event lanes. Camera names and event kinds are backend data.
export const playback = {
  en: {
    title: "Playback",
    description:
      "Every camera on one timeline — scrub the day. Clips are ink blocks; the dashed lane is a gap with no footage.",
    // toolbar
    previousDay: "Previous day",
    nextDay: "Next day",
    today: "Today",
    allCameras: "All cameras",
    eventLaneTitle: "Which events show on the marker lane",
    eventsAll: (mode: string) => `Events: ${mode}`,
    modeAll: "all",
    modeFootage: "footage",
    // empty
    emptyTitle: "No footage on this day",
    emptyDescription: (day: string) =>
      `Nothing was recorded on ${day}. Pick another day, or set a workflow to record on motion.`,
    // stage
    noFootageAt: (camera: string, time: string) =>
      `No footage on ${camera} at ${time} — jump to the next clip.`,
    thisCamera: "this camera",
    recording: "recording",
    noFootage: "no footage",
    // transport
    previousClip: "Previous clip (,)",
    pause: "Pause (Space)",
    play: "Play (Space)",
    nextClip: "Next clip (.)",
    previousEvent: "Previous event ([)",
    nextEvent: "Next event (])",
    showAllCameras: "Show all cameras",
    shortcutHint: ", / . clip · [ / ] event · Space play",
    zoomOut: "Zoom out",
    zoomIn: "Zoom in",
    rulerStep: "Ruler step at this zoom",
    // timeline
    noEvents: "No events",
    clipCount: (n: number) => `${n} clip${n === 1 ? "" : "s"}`,
    exitSolo: "Exit solo",
    soloCamera: "Solo this camera",
    hideCamera: "Hide this camera",
    playheadLabel: "Playhead — click or use arrow keys to scrub",
    markerLabel: (count: number, kind: string, time: string) =>
      `${count} ${kind} event(s) at ${time}`,
    now: "now",
    dragToScrub: "Drag to scrub",
    dragPlayhead: "Drag playhead",
    hoverEvents: (time: string, n: number) =>
      `${time} · ${n} event${n === 1 ? "" : "s"}`,
    moreEvents: (n: number) => `+${n} more`,
    segTitle: (from: string, to: string, clips: number, edgeOnly: boolean) =>
      `${from}–${to} · ${clips} clip${clips === 1 ? "" : "s"}${edgeOnly ? " · has edge-only footage" : ""}`,
    // toasts
    loadFailed: "Failed to load the timeline",
    noLaterClip: "No later clip on this camera.",
    noEarlierClip: "No earlier clip on this camera.",
    noLaterEvent: "No later event.",
    noEarlierEvent: "No earlier event.",
  },
  pt: {
    title: "Reprodução",
    description:
      "Todas as câmeras numa linha do tempo — percorra o dia. Os clipes são blocos sólidos; a faixa tracejada é um intervalo sem imagens.",
    // toolbar
    previousDay: "Dia anterior",
    nextDay: "Próximo dia",
    today: "Hoje",
    allCameras: "Todas as câmeras",
    eventLaneTitle: "Quais eventos aparecem na faixa de marcadores",
    eventsAll: (mode: string) => `Eventos: ${mode}`,
    modeAll: "todos",
    modeFootage: "gravações",
    // empty
    emptyTitle: "Nenhuma gravação neste dia",
    emptyDescription: (day: string) =>
      `Nada foi gravado em ${day}. Escolha outro dia, ou configure um fluxo para gravar ao detectar movimento.`,
    // stage
    noFootageAt: (camera: string, time: string) =>
      `Sem imagens em ${camera} às ${time} — pule para o próximo clipe.`,
    thisCamera: "esta câmera",
    recording: "gravando",
    noFootage: "sem imagens",
    // transport
    previousClip: "Clipe anterior (,)",
    pause: "Pausar (Espaço)",
    play: "Reproduzir (Espaço)",
    nextClip: "Próximo clipe (.)",
    previousEvent: "Evento anterior ([)",
    nextEvent: "Próximo evento (])",
    showAllCameras: "Mostrar todas as câmeras",
    shortcutHint: ", / . clipe · [ / ] evento · Espaço reproduzir",
    zoomOut: "Afastar",
    zoomIn: "Aproximar",
    rulerStep: "Passo da régua neste zoom",
    // timeline
    noEvents: "Sem eventos",
    clipCount: (n: number) => `${n} clipe${n === 1 ? "" : "s"}`,
    exitSolo: "Sair do modo solo",
    soloCamera: "Isolar esta câmera",
    hideCamera: "Ocultar esta câmera",
    playheadLabel:
      "Cursor de reprodução — clique ou use as setas para percorrer",
    markerLabel: (count: number, kind: string, time: string) =>
      `${count} evento(s) de ${kind} às ${time}`,
    now: "agora",
    dragToScrub: "Arraste para percorrer",
    dragPlayhead: "Arrastar o cursor",
    hoverEvents: (time: string, n: number) =>
      `${time} · ${n} evento${n === 1 ? "" : "s"}`,
    moreEvents: (n: number) => `+${n} a mais`,
    segTitle: (from: string, to: string, clips: number, edgeOnly: boolean) =>
      `${from}–${to} · ${clips} clipe${clips === 1 ? "" : "s"}${edgeOnly ? " · tem imagens só no appliance" : ""}`,
    // toasts
    loadFailed: "Falha ao carregar a linha do tempo",
    noLaterClip: "Nenhum clipe posterior nesta câmera.",
    noEarlierClip: "Nenhum clipe anterior nesta câmera.",
    noLaterEvent: "Nenhum evento posterior.",
    noEarlierEvent: "Nenhum evento anterior.",
  },
};
