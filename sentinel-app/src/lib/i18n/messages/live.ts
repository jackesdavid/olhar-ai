// The Live page — camera picker, the honest playback badge/overlays (card #89) and its errors.
export const live = {
  en: {
    title: "Live",
    descriptionCloud:
      "Live camera feeds over your private edge link — never a public stream.",
    descriptionLocal:
      "Real-time camera video, streamed straight from your box.",
    refresh: "Refresh",
    refreshing: "Refreshing…",
    loadingStreams: "Loading streams…",
    emptyTitle: "No camera streams yet",
    emptyDescription:
      "Add a camera and set its credentials on the Devices page — your edge resolves the stream automatically.",
    goToDevices: "Go to Devices",
    camera: "Camera",
    selectCamera: "Select a camera",
    selectToStart: "Select a camera to start streaming.",
    liveCameraFrame: (name: string) => `Live camera ${name}`,
    // badge + overlays
    badgeLive: "LIVE",
    badgeConnecting: "Connecting…",
    badgeReconnecting: "Reconnecting…",
    badgeNoSignal: "No signal",
    connectingToCamera: "Connecting to camera…",
    retry: "Retry",
    trySd: "Try SD",
    // errors
    errHdCodec:
      "Couldn't play the HD stream — this camera's HD codec may not be supported in your browser. Try SD.",
    errConnect:
      "Couldn't connect to the live stream. The camera or your edge's live video may be unreachable.",
    errKeepsDropping:
      "The live stream keeps dropping. Check the camera and that your edge is online.",
    errUnsupportedBrowser: "Live video isn't supported in this browser.",
    errNoCodecs: "This browser can't play any of the camera's video codecs.",
    errClosedEarly:
      "The live connection closed before video started. Your edge's live video may be unreachable.",
    // toasts + footnote
    noStreamsTitle: "No camera streams yet.",
    noStreamsHint: "Add a camera with credentials on the Devices page.",
    streamsReady: (n: number) => `${n} stream${n === 1 ? "" : "s"} ready.`,
    loadFailed: "Failed to load live streams",
    unvalidated: (n: number, names: string) =>
      `${n} saved camera${n === 1 ? "" : "s"} without a stream yet (${names}). Set ${n === 1 ? "its" : "their"} credentials on the Devices page.`,
  },
  pt: {
    title: "Ao vivo",
    descriptionCloud:
      "Câmeras ao vivo pelo seu link privado com o appliance — nunca um stream público.",
    descriptionLocal:
      "Vídeo em tempo real, transmitido direto do seu box.",
    refresh: "Atualizar",
    refreshing: "Atualizando…",
    loadingStreams: "Carregando transmissões…",
    emptyTitle: "Nenhuma transmissão ainda",
    emptyDescription:
      "Adicione uma câmera e defina suas credenciais na página Dispositivos — seu appliance resolve a transmissão automaticamente.",
    goToDevices: "Ir para Dispositivos",
    camera: "Câmera",
    selectCamera: "Selecione uma câmera",
    selectToStart: "Selecione uma câmera para começar a transmitir.",
    liveCameraFrame: (name: string) => `Câmera ao vivo ${name}`,
    // badge + overlays
    badgeLive: "AO VIVO",
    badgeConnecting: "Conectando…",
    badgeReconnecting: "Reconectando…",
    badgeNoSignal: "Sem sinal",
    connectingToCamera: "Conectando à câmera…",
    retry: "Tentar de novo",
    trySd: "Tentar SD",
    // errors
    errHdCodec:
      "Não foi possível reproduzir o stream HD — o codec HD desta câmera pode não ser suportado no seu navegador. Tente SD.",
    errConnect:
      "Não foi possível conectar à transmissão ao vivo. A câmera ou o vídeo do seu appliance pode estar inacessível.",
    errKeepsDropping:
      "A transmissão ao vivo continua caindo. Verifique a câmera e se o seu appliance está online.",
    errUnsupportedBrowser: "Vídeo ao vivo não é suportado neste navegador.",
    errNoCodecs: "Este navegador não reproduz nenhum dos codecs de vídeo da câmera.",
    errClosedEarly:
      "A conexão ao vivo fechou antes do vídeo começar. O vídeo do seu appliance pode estar inacessível.",
    // toasts + footnote
    noStreamsTitle: "Nenhuma transmissão de câmera ainda.",
    noStreamsHint: "Adicione uma câmera com credenciais na página Dispositivos.",
    streamsReady: (n: number) =>
      `${n} transmiss${n === 1 ? "ão pronta" : "ões prontas"}.`,
    loadFailed: "Falha ao carregar as transmissões ao vivo",
    unvalidated: (n: number, names: string) =>
      `${n} câmera${n === 1 ? "" : "s"} salva${n === 1 ? "" : "s"} ainda sem transmissão (${names}). Defina ${n === 1 ? "a credencial dela" : "as credenciais delas"} na página Dispositivos.`,
  },
};
