// Settings → General (/settings): the three tabs — General (language, audio, voice, connection),
// Cameras (live/recording behaviour + storage) and Detection (MOG2 motion tuning).
export const settings = {
  en: {
    title: "Settings",
    description: "Audio, voice, and connection preferences.",
    saving: "Saving…",
    // tabs
    tabGeneral: "General",
    tabCameras: "Cameras",
    tabMotion: "Detection",
    // audio
    audio: "Audio",
    audioDescription: "Output gains, from muted to full.",
    gainMaster: "Master",
    gainVoice: "Voice",
    gainUi: "UI",
    gainAmbient: "Ambient",
    // voice
    voiceTitle: "Voice & Interaction",
    voiceDescription: "Speech synthesis and input behavior.",
    voiceEnabled: "Voice enabled",
    voiceEnabledHint: "Allow the console to speak responses aloud.",
    typingSpeed: "Typing speed",
    typingSpeedHint: "How fast streamed text is revealed (1–8).",
    autoSend: "Auto-send transcripts",
    autoSendHint: "Submit speech-to-text input automatically when you stop speaking.",
    // connection
    connection: "Connection",
    connectionDescription: "Network transport for the backend link.",
    transport: "Transport",
    transportAuto: "Auto",
    // automations — approvals (#176)
    approvals: "Approval requests",
    approvalsDescription:
      "When an automation asks someone to approve an action on their phone.",
    approvalsCap: "Most requests waiting on one phone",
    approvalsCapHint:
      "An automation that keeps asking can bury a phone in requests. Past this many unanswered, further asks stop and the automation takes its Failure path until some are answered or expire.",
    // cameras
    camerasTitle: "Cameras",
    camerasDescription: "Live feed and recording behaviour.",
    alwaysWarm: "Keep camera feeds always on",
    alwaysWarmHint:
      "Maintains a rolling pre-roll buffer per camera so recordings start with footage from before the trigger. Uses more bandwidth when nothing is watching.",
    hdFaceCapture: "HD face capture",
    hdFaceCaptureHint:
      "Capture faces from the camera's main (HD) stream instead of the low-res preview — much larger, sharper faces. Heavier to decode; best on a box with GPU video decode.",
    gpuDecodeYes: (method: string) => `This box supports GPU decode (${method}).`,
    gpuDecodeNo: "No GPU-decode-capable box detected — HD would run on CPU (heavier).",
    takesEffectRestart: "Takes effect after the box reloads its config (Restart).",
    maxPreRoll: "Max pre-roll",
    maxPreRollHint:
      "How many seconds of footage the ring buffer keeps before a recording trigger (0–60 s). The recording node can request up to this amount.",
    deleteLocal: "Delete local copy once saved to the cloud",
    deleteLocalNoPlan:
      "Your plan doesn't include cloud recording, so clips only ever exist on the appliance — there's nothing to fall back to.",
    deleteLocalHint:
      "The appliance keeps a clip only until the cloud confirms it's safe, then frees the space. Clips still play and download from the cloud.",
    spoolSize: "Local spool size",
    maxStorage: "Max recording storage",
    spoolHint:
      "Headroom for clips waiting to reach the cloud (e.g. while the internet is down). Clips already in the cloud take no space here.",
    maxStorageHint:
      "Old clips are auto-deleted (oldest first) when total recording size exceeds this limit.",
    fullDiskBefore: "A workflow event ",
    fullDiskAfter: " fires when the disk is 90% full, or recordings reach their limit.",
    recordingsFolder: "Recordings folder",
    recordingsFolderHint:
      "Directory on the edge appliance where .mp4 clips are stored. Takes effect after the edge restarts.",
    currentUsage: "Current storage usage",
    clipsUsed: (clips: number, pct: number) =>
      `${clips} clip${clips === 1 ? "" : "s"} · ${pct}% used`,
    // motion
    motionTitle: "Motion detection",
    motionDescription:
      "MOG2 background-subtraction tuning, per box. Takes effect after the edge restarts.",
    sensitivity: "Sensitivity",
    sensitivityHint:
      "MOG2 varThreshold — how much pixel change triggers motion. Lower = more sensitive.",
    sensVeryHigh: "very high",
    sensHigh: "high",
    sensMedium: "medium",
    sensLow: "low",
    sensVeryLow: "very low",
    fps: "Frames per second",
    fpsHint:
      "How many frames/second feed the detector. Higher = more responsive, but more decode load.",
    motionFloor: "Motion floor",
    motionFloorHint:
      "Minimum share of the frame that must change to count as motion. Lower = catches subtler movement.",
    debounce: "Event debounce",
    debounceHint: "Minimum seconds between motion events for a camera.",
    history: "Background history",
    historyFrames: (frames: number, secs: number) => `${frames} frames (~${secs}s)`,
    historyHint:
      "How many frames the background model adapts over. Longer = slower to absorb new static objects.",
    minBlobArea: "Min blob area",
    minBlobAreaHint: "Processing px",
    procWidth: "Proc width",
    procWidthHint: "Downscale W",
    procHeight: "Proc height",
    procHeightHint: "Downscale H",
    // toasts
    loadFailed: "Failed to load settings",
    saved: "Settings saved",
    saveFailed: "Failed to save settings",
  },
  pt: {
    title: "Configurações",
    description: "Preferências de áudio, voz e conexão.",
    saving: "Salvando…",
    // tabs
    tabGeneral: "Geral",
    tabCameras: "Câmeras",
    tabMotion: "Detecção",
    // audio
    audio: "Áudio",
    audioDescription: "Volumes de saída, do mudo ao máximo.",
    gainMaster: "Geral",
    gainVoice: "Voz",
    gainUi: "Interface",
    gainAmbient: "Ambiente",
    // voice
    voiceTitle: "Voz e interação",
    voiceDescription: "Síntese de fala e comportamento de entrada.",
    voiceEnabled: "Voz ativada",
    voiceEnabledHint: "Permite que o console fale as respostas em voz alta.",
    typingSpeed: "Velocidade de digitação",
    typingSpeedHint: "Quão rápido o texto transmitido aparece (1–8).",
    autoSend: "Enviar transcrições automaticamente",
    autoSendHint: "Envia o texto ditado automaticamente quando você para de falar.",
    // connection
    connection: "Conexão",
    connectionDescription: "Transporte de rede para o link com o backend.",
    transport: "Transporte",
    transportAuto: "Automático",
    // automations — approvals (#176)
    approvals: "Pedidos de aprovação",
    approvalsDescription:
      "Quando uma automação pede que alguém aprove uma ação pelo celular.",
    approvalsCap: "Máximo de pedidos aguardando em um celular",
    approvalsCapHint:
      "Uma automação que pede sem parar pode soterrar um celular de pedidos. Passando desse número de pedidos sem resposta, novos pedidos deixam de ser enviados e a automação segue pelo caminho de Falha até que alguns sejam respondidos ou expirem.",
    // cameras
    camerasTitle: "Câmeras",
    camerasDescription: "Comportamento da transmissão ao vivo e da gravação.",
    alwaysWarm: "Manter as câmeras sempre conectadas",
    alwaysWarmHint:
      "Mantém um buffer contínuo por câmera para que as gravações comecem com imagens de antes do gatilho. Usa mais banda quando ninguém está assistindo.",
    hdFaceCapture: "Captura de rostos em HD",
    hdFaceCaptureHint:
      "Captura rostos do stream principal (HD) da câmera em vez da prévia de baixa resolução — rostos bem maiores e mais nítidos. Mais pesado de decodificar; ideal em um appliance com decodificação por GPU.",
    gpuDecodeYes: (method: string) => `Este appliance suporta decodificação por GPU (${method}).`,
    gpuDecodeNo:
      "Nenhum appliance com decodificação por GPU detectado — o HD rodaria na CPU (mais pesado).",
    takesEffectRestart: "Entra em vigor depois que o appliance recarrega a configuração (Reiniciar).",
    maxPreRoll: "Pré-gravação máxima",
    maxPreRollHint:
      "Quantos segundos de imagem o buffer guarda antes de um gatilho de gravação (0–60 s). O nó de gravação pode pedir até esse valor.",
    deleteLocal: "Excluir a cópia local depois de salvar na nuvem",
    deleteLocalNoPlan:
      "Seu plano não inclui gravação na nuvem, então os clipes só existem no appliance — não há para onde recorrer.",
    deleteLocalHint:
      "O appliance guarda o clipe apenas até a nuvem confirmar que está seguro, e então libera o espaço. Os clipes continuam sendo reproduzidos e baixados da nuvem.",
    spoolSize: "Tamanho da fila local",
    maxStorage: "Armazenamento máximo de gravações",
    spoolHint:
      "Folga para clipes aguardando envio à nuvem (por exemplo, com a internet fora). Clipes já na nuvem não ocupam espaço aqui.",
    maxStorageHint:
      "Clipes antigos são excluídos automaticamente (do mais antigo) quando o total de gravações passa deste limite.",
    fullDiskBefore: "Um evento de fluxo ",
    fullDiskAfter: " dispara quando o disco está 90% cheio, ou as gravações atingem o limite.",
    recordingsFolder: "Pasta de gravações",
    recordingsFolderHint:
      "Diretório no appliance onde os clipes .mp4 são guardados. Entra em vigor depois que o appliance reinicia.",
    currentUsage: "Uso de armazenamento atual",
    clipsUsed: (clips: number, pct: number) =>
      `${clips} clipe${clips === 1 ? "" : "s"} · ${pct}% usado`,
    // motion
    motionTitle: "Detecção de movimento",
    motionDescription:
      "Ajuste da subtração de fundo MOG2, por appliance. Entra em vigor depois que o appliance reinicia.",
    sensitivity: "Sensibilidade",
    sensitivityHint:
      "varThreshold do MOG2 — quanta mudança de pixel dispara movimento. Menor = mais sensível.",
    sensVeryHigh: "muito alta",
    sensHigh: "alta",
    sensMedium: "média",
    sensLow: "baixa",
    sensVeryLow: "muito baixa",
    fps: "Quadros por segundo",
    fpsHint:
      "Quantos quadros por segundo alimentam o detector. Mais = mais responsivo, mas mais carga de decodificação.",
    motionFloor: "Piso de movimento",
    motionFloorHint:
      "Fração mínima do quadro que precisa mudar para contar como movimento. Menor = capta movimentos mais sutis.",
    debounce: "Intervalo entre eventos",
    debounceHint: "Segundos mínimos entre eventos de movimento de uma câmera.",
    history: "Histórico de fundo",
    historyFrames: (frames: number, secs: number) => `${frames} quadros (~${secs}s)`,
    historyHint:
      "Sobre quantos quadros o modelo de fundo se adapta. Maior = mais lento para absorver novos objetos parados.",
    minBlobArea: "Área mínima do blob",
    minBlobAreaHint: "px de processamento",
    procWidth: "Largura de proc.",
    procWidthHint: "Redução L",
    procHeight: "Altura de proc.",
    procHeightHint: "Redução A",
    // toasts
    loadFailed: "Falha ao carregar as configurações",
    saved: "Configurações salvas",
    saveFailed: "Falha ao salvar as configurações",
  },
};
