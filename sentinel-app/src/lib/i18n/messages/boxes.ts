// Settings → Edge boxes (/settings/boxes): the box table (version/health/resources/last seen),
// the restart + update actions, the add-a-box form and the remove confirm.
export const boxes = {
  en: {
    title: "Edge boxes",
    description:
      "Install a Sentinel edge appliance on-site. It pulls only this workspace's cameras and streams them privately.",
    noPermission: "You don't have permission to manage edge boxes.",
    yourBoxes: "Your boxes",
    colName: "Name",
    colVersion: "Version",
    colHealth: "Health",
    colResources: "Resources",
    colLastSeen: "Last seen",
    copyIdTitle: (id: string) =>
      `Copy edge ID — filter this box's logs in Datadog with edge_id:${id}`,
    updateTo: (version: string) => `Update to v${version}`,
    updating: "Updating…",
    updateArrow: (version: string) => `Update → v${version}`,
    platformTitle: "Reported platform (OS · arch)",
    gpuDecodeTitle: "GPU video decode available — the box can run HD face capture (#120)",
    gpuDecode: (method: string) => `GPU decode · ${method}`,
    cpuDecodeTitle: "No GPU video decode — HD face capture would be CPU-only",
    cpuDecode: "CPU decode",
    noMediaTitle: "no media backend — snapshots / GetCurrentFrame will fail",
    noMedia: "no media backend",
    neverConnected: "never connected",
    restartTitle: "Restart the box",
    removeTitle: "Remove",
    emptyTitle: "No edge boxes yet",
    emptyDescription:
      "Create a box below, then run its installer on the on-prem machine to connect it.",
    healthHint: (name: string) =>
      `${name}: no media backend — live / snapshots will fail (reinstall to get GStreamer).`,
    // add form
    addTitle: "Add an edge box",
    addDescription:
      "Name it and run the generated command on the on-site machine — private networking is handled for you.",
    boxName: "Box name",
    boxNamePlaceholder: "Front office",
    createBox: "Create box",
    // delete dialog
    deleteTitle: "Remove edge box?",
    deleteDescription: (name: string) =>
      `"${name}" will stop receiving camera config. Its token is revoked; reinstalling needs a new box.`,
    remove: "Remove",
    // toasts
    loadFailed: "Failed to load boxes",
    createFailed: "Failed to create box",
    copied: "Copied to clipboard",
    copyFailed: "Couldn't copy — select and copy manually.",
    removed: (name: string) => `Removed "${name}"`,
    removeFailed: "Failed to remove box",
    restartRequested: (name: string) => `Restart requested for "${name}"`,
    restartHint: "It relaunches and reconnects within ~30s.",
    restartFailed: "Failed to request restart",
    updated: (name: string, version: string) => `"${name}" updated to v${version}`,
    updateSlow: (name: string) =>
      `"${name}" update is taking longer than expected — check the box logs.`,
    updateStarted: (name: string, version: string) =>
      `Update started for "${name}" — waiting for v${version}…`,
    updateFailed: "Failed to request update",
  },
  pt: {
    title: "Appliances",
    description:
      "Instale um appliance Sentinel no local. Ele puxa apenas as câmeras deste workspace e as transmite de forma privada.",
    noPermission: "Você não tem permissão para gerenciar appliances.",
    yourBoxes: "Seus appliances",
    colName: "Nome",
    colVersion: "Versão",
    colHealth: "Saúde",
    colResources: "Recursos",
    colLastSeen: "Visto por último",
    copyIdTitle: (id: string) =>
      `Copiar o ID do appliance — filtre os logs dele no Datadog com edge_id:${id}`,
    updateTo: (version: string) => `Atualizar para v${version}`,
    updating: "Atualizando…",
    updateArrow: (version: string) => `Atualizar → v${version}`,
    platformTitle: "Plataforma informada (SO · arquitetura)",
    gpuDecodeTitle:
      "Decodificação de vídeo por GPU disponível — o appliance consegue capturar rostos em HD (#120)",
    gpuDecode: (method: string) => `Decode por GPU · ${method}`,
    cpuDecodeTitle:
      "Sem decodificação por GPU — a captura de rostos em HD rodaria só na CPU",
    cpuDecode: "Decode por CPU",
    noMediaTitle: "sem backend de mídia — capturas / GetCurrentFrame vão falhar",
    noMedia: "sem backend de mídia",
    neverConnected: "nunca conectou",
    restartTitle: "Reiniciar o appliance",
    removeTitle: "Remover",
    emptyTitle: "Nenhum appliance ainda",
    emptyDescription:
      "Crie um appliance abaixo e rode o instalador na máquina do local para conectá-lo.",
    healthHint: (name: string) =>
      `${name}: sem backend de mídia — ao vivo / capturas vão falhar (reinstale para obter o GStreamer).`,
    // add form
    addTitle: "Adicionar um appliance",
    addDescription:
      "Dê um nome e rode o comando gerado na máquina do local — a rede privada é configurada para você.",
    boxName: "Nome do appliance",
    boxNamePlaceholder: "Recepção",
    createBox: "Criar appliance",
    // delete dialog
    deleteTitle: "Remover appliance?",
    deleteDescription: (name: string) =>
      `"${name}" deixará de receber a configuração das câmeras. O token dele é revogado; reinstalar exige um novo appliance.`,
    remove: "Remover",
    // toasts
    loadFailed: "Falha ao carregar os appliances",
    createFailed: "Falha ao criar o appliance",
    copied: "Copiado para a área de transferência",
    copyFailed: "Não foi possível copiar — selecione e copie manualmente.",
    removed: (name: string) => `"${name}" removido`,
    removeFailed: "Falha ao remover o appliance",
    restartRequested: (name: string) => `Reinício solicitado para "${name}"`,
    restartHint: "Ele reinicia e reconecta em cerca de 30s.",
    restartFailed: "Falha ao solicitar o reinício",
    updated: (name: string, version: string) => `"${name}" atualizado para v${version}`,
    updateSlow: (name: string) =>
      `A atualização de "${name}" está demorando mais que o esperado — verifique os logs do appliance.`,
    updateStarted: (name: string, version: string) =>
      `Atualização iniciada para "${name}" — aguardando a v${version}…`,
    updateFailed: "Falha ao solicitar a atualização",
  },
};
