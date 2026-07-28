// The Recordings page — the clip table (filters, sorting, grouping, bulk select), the cloud
// storage strip, the player dialog and the delete confirms.
export const recordings = {
  en: {
    title: "Recordings",
    countDescription: (n: number) =>
      `${n} recording${n === 1 ? "" : "s"} saved from your cameras.`,
    emptyDescription: "Clips saved from your cameras.",
    deleteCount: (n: number) => `Delete ${n}`,
    refresh: "Refresh",
    // storage strip
    storedOf: (gb: number, days: number) => `stored of ${gb} GB · ${days}-day cloud retention`,
    clipsInCloud: (n: number) => `${n} clip${n === 1 ? "" : "s"} in the cloud`,
    overQuota: (amount: string) =>
      `Over quota this month — about ${amount} in overage so far.`,
    uploadNote: (days: number) =>
      `New clips upload automatically. Past ${days} days, cloud copies expire on their own.`,
    noCloudPlan:
      "Your plan keeps clips on the box only. Cloud backup survives the box being stolen or wiped.",
    seePlans: "See plans",
    // filters
    camera: "Camera",
    allCameras: "All cameras",
    from: "From",
    to: "To",
    apply: "Apply",
    clear: "Clear",
    groupBy: "Group by",
    groupNone: "None",
    groupCamera: "Camera",
    groupDay: "Day",
    // table
    emptyTitle: "No recordings found",
    emptyHint:
      "Recordings appear here once a workflow triggers the Start Recording node and a clip is finalised.",
    selectAll: "Select all",
    deselectAll: "Deselect all",
    colName: "Name",
    colDate: "Date",
    colDuration: "Duration",
    colSize: "Size",
    colStorage: "Storage",
    colActions: "Actions",
    play: "Play",
    download: "Download",
    delete: "Delete",
    // clip state badges
    badgeCloud: "Cloud",
    badgeInTransit: "In transit",
    badgeLocal: "Local",
    badgeFailed: "Failed",
    badgeDeleting: "Deleting",
    // pagination
    pageOf: (page: number, count: number, total: number) =>
      `Page ${page} of ${count} (${total} recordings)`,
    prev: "Prev",
    next: "Next",
    // dialogs
    deleteTitle: "Delete recording?",
    deleteDescription: (name: string) =>
      `"${name}" will be permanently removed from the edge disk. This cannot be undone.`,
    bulkDeleteTitle: (n: number) => `Delete ${n} recording${n === 1 ? "" : "s"}?`,
    bulkDeleteDescription:
      "These clips will be permanently removed from the edge disk. This cannot be undone.",
    cannotPlay: "Your browser cannot play this clip.",
    // toasts
    playFailed: "Could not open this clip for playback.",
    deletionStarted: "Deletion started — the clip is being removed.",
    bulkDeleting: (n: number) => `Deleting ${n} recording${n === 1 ? "" : "s"}…`,
  },
  pt: {
    title: "Gravações",
    countDescription: (n: number) =>
      `${n} gravaç${n === 1 ? "ão salva" : "ões salvas"} das suas câmeras.`,
    emptyDescription: "Clipes salvos das suas câmeras.",
    deleteCount: (n: number) => `Excluir ${n}`,
    refresh: "Atualizar",
    // storage strip
    storedOf: (gb: number, days: number) =>
      `armazenados de ${gb} GB · retenção na nuvem de ${days} dias`,
    clipsInCloud: (n: number) => `${n} clipe${n === 1 ? "" : "s"} na nuvem`,
    overQuota: (amount: string) =>
      `Acima da cota neste mês — cerca de ${amount} de excedente até agora.`,
    uploadNote: (days: number) =>
      `Novos clipes sobem automaticamente. Depois de ${days} dias, as cópias na nuvem expiram sozinhas.`,
    noCloudPlan:
      "Seu plano guarda os clipes apenas no appliance. O backup na nuvem sobrevive ao appliance ser roubado ou apagado.",
    seePlans: "Ver planos",
    // filters
    camera: "Câmera",
    allCameras: "Todas as câmeras",
    from: "De",
    to: "Até",
    apply: "Aplicar",
    clear: "Limpar",
    groupBy: "Agrupar por",
    groupNone: "Nenhum",
    groupCamera: "Câmera",
    groupDay: "Dia",
    // table
    emptyTitle: "Nenhuma gravação encontrada",
    emptyHint:
      "As gravações aparecem aqui quando um fluxo aciona o nó Iniciar Gravação e um clipe é finalizado.",
    selectAll: "Selecionar tudo",
    deselectAll: "Desmarcar tudo",
    colName: "Nome",
    colDate: "Data",
    colDuration: "Duração",
    colSize: "Tamanho",
    colStorage: "Armazenamento",
    colActions: "Ações",
    play: "Reproduzir",
    download: "Baixar",
    delete: "Excluir",
    // clip state badges
    badgeCloud: "Nuvem",
    badgeInTransit: "Em trânsito",
    badgeLocal: "Local",
    badgeFailed: "Falhou",
    badgeDeleting: "Excluindo",
    // pagination
    pageOf: (page: number, count: number, total: number) =>
      `Página ${page} de ${count} (${total} gravações)`,
    prev: "Anterior",
    next: "Próxima",
    // dialogs
    deleteTitle: "Excluir gravação?",
    deleteDescription: (name: string) =>
      `"${name}" será removido permanentemente do disco do appliance. Isso não pode ser desfeito.`,
    bulkDeleteTitle: (n: number) => `Excluir ${n} gravaç${n === 1 ? "ão" : "ões"}?`,
    bulkDeleteDescription:
      "Estes clipes serão removidos permanentemente do disco do appliance. Isso não pode ser desfeito.",
    cannotPlay: "Seu navegador não consegue reproduzir este clipe.",
    // toasts
    playFailed: "Não foi possível abrir este clipe para reprodução.",
    deletionStarted: "Exclusão iniciada — o clipe está sendo removido.",
    bulkDeleting: (n: number) => `Excluindo ${n} gravaç${n === 1 ? "ão" : "ões"}…`,
  },
};
