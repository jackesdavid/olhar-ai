// The landing page (/) — page header, the four stat cards and the quick-action tiles.
export const dashboard = {
  en: {
    title: "Dashboard",
    description: "Overview of your devices, alerts, and automations.",
    systemReady: "System ready",
    // ---- first run: no appliance yet (#42 reworked) ----
    // The first screen after signup used to read 0/0/0/0 under a green "System ready". Both
    // halves were false, and neither offered a way forward.
    setupTitle: "Connect your first appliance",
    setupBody:
      "Cameras live on your network, not ours. A small on-premise appliance talks to them and sends only what you ask for — clips, detections, alerts. Nothing else leaves your premises.",
    setupCta: "Set up an appliance",
    setupSkip: "Look around first",
    // ---- the view cannot be trusted ----
    loadFailedTitle: "Couldn't load your account",
    loadFailedBody:
      "These numbers may be wrong or missing. This is our side, not yours — try again in a moment.",
    retry: "Try again",
    statDevices: "Devices",
    statCamerasLive: "Cameras live",
    statOpenAlerts: "Open alerts",
    statRecipients: "Recipients",
    quickActions: "Quick actions",
    askTitle: "Ask Olhar AI",
    askCaption: "Chat with the assistant",
    workflowTitle: "Build a workflow",
    workflowCaption: "Automate a response",
    mapTitle: "Open the map",
    mapCaption: "See your floor plan",
    liveTitle: "Watch live",
    liveCaption: "Real-time camera video",
  },
  pt: {
    title: "Painel",
    description: "Visão geral dos seus dispositivos, alertas e automações.",
    systemReady: "Sistema pronto",
    setupTitle: "Conecte seu primeiro appliance",
    setupBody:
      "As câmeras ficam na sua rede, não na nossa. Um pequeno appliance local conversa com elas e envia só o que você pedir — clipes, detecções, alertas. Nada além disso sai daí.",
    setupCta: "Configurar um appliance",
    setupSkip: "Prefiro olhar antes",
    loadFailedTitle: "Não foi possível carregar sua conta",
    loadFailedBody:
      "Estes números podem estar errados ou faltando. O problema é do nosso lado — tente de novo em instantes.",
    retry: "Tentar de novo",
    statDevices: "Dispositivos",
    statCamerasLive: "Câmeras ao vivo",
    statOpenAlerts: "Alertas abertos",
    statRecipients: "Destinatários",
    quickActions: "Ações rápidas",
    askTitle: "Perguntar à Olhar AI",
    askCaption: "Converse com o assistente",
    workflowTitle: "Criar um fluxo",
    workflowCaption: "Automatize uma resposta",
    mapTitle: "Abrir o mapa",
    mapCaption: "Veja sua planta baixa",
    liveTitle: "Assistir ao vivo",
    liveCaption: "Vídeo das câmeras em tempo real",
  },
};
