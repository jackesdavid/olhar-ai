// The Assistant page (/chat) — the voice orb, the transcript and the composer. The assistant's
// own replies come from the model, not from here.
export const chat = {
  en: {
    title: "Assistant",
    description: "Ask Olhar AI about your cameras, alerts, and people.",
    muteReplies: "Mute spoken replies",
    speakReplies: "Speak replies aloud",
    // status line
    listening: "Listening…",
    speaking: "Speaking…",
    thinking: "Thinking…",
    holdToTalkHint: "Hold Ctrl or the orb to talk",
    holdToTalk: "Hold to talk (or hold Ctrl)",
    holdToTalkLabel: "Hold to talk",
    holdToTalkSpace: "Hold to talk (or hold Space)",
    // transcript
    emptyHint:
      'Try "list my cameras", "what\'s on the hall camera?", or "show me recent alerts".',
    openLive: (ip: string) => `Open live view (${ip})`,
    devicesChanged: "Camera list updated.",
    alertsChanged: "Alerts updated.",
    // composer
    placeholder: "Message the assistant…",
    send: "Send",
    // toasts
    noReply: "The assistant didn't respond.",
    micFailed: "Couldn't access the microphone.",
    transcriptionFailed: "Transcription failed.",
  },
  pt: {
    title: "Assistente",
    description: "Pergunte à Olhar AI sobre suas câmeras, alertas e pessoas.",
    muteReplies: "Silenciar as respostas faladas",
    speakReplies: "Falar as respostas em voz alta",
    // status line
    listening: "Ouvindo…",
    speaking: "Falando…",
    thinking: "Pensando…",
    holdToTalkHint: "Segure Ctrl ou o orbe para falar",
    holdToTalk: "Segure para falar (ou segure Ctrl)",
    holdToTalkLabel: "Segure para falar",
    holdToTalkSpace: "Segure para falar (ou segure Espaço)",
    // transcript
    emptyHint:
      'Experimente "liste minhas câmeras", "o que está na câmera do corredor?" ou "mostre os alertas recentes".',
    openLive: (ip: string) => `Abrir a visualização ao vivo (${ip})`,
    devicesChanged: "Lista de câmeras atualizada.",
    alertsChanged: "Alertas atualizados.",
    // composer
    placeholder: "Escreva para o assistente…",
    send: "Enviar",
    // toasts
    noReply: "O assistente não respondeu.",
    micFailed: "Não foi possível acessar o microfone.",
    transcriptionFailed: "Falha na transcrição.",
  },
};
