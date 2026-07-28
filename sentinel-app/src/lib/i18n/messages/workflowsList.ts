// The /workflows landing list (WorkflowsList.svelte) and the shared run-status/relative-time
// helpers in $lib/workflows/format. Workflow names, groups and node kinds are not from here —
// node vocabulary comes from the backend catalog (`/workflows/catalog`).
export const workflowsList = {
  en: {
    title: "Workflows",
    description: "Graph automations that run on a trigger or on demand.",
    newWorkflow: "New workflow",
    // tabs
    tabMine: "My workflows",
    tabTemplates: "Templates",
    tabDictionaries: "Dictionary Tables",
    // filters
    searchPlaceholder: "Search workflows…",
    allGroups: "All groups",
    clear: "Clear",
    noMatches: "No workflows match your filters.",
    clearFilters: "Clear filters",
    emptyTitle: "No workflows yet",
    emptyDescription:
      "Start from a ready-made template, or build one from scratch to chain cameras, alerts, vision, and recipients.",
    browseTemplates: "Browse templates",
    // row actions
    duplicate: "Duplicate",
    liveInstances: "Live instances — saved run contexts",
    recentRuns: "Recent runs",
    delete: "Delete",
    unmakeTemplate: "A template — click to make it a normal workflow",
    makeTemplate: "Save as a template",
    unpublishGlobal: "Published globally — click to unpublish",
    publishGlobal: "Publish to every tenant (global template)",
    // run status / trigger source (shared with the editor)
    running: "Running",
    triggerCron: "Cron",
    triggerEvent: "Event",
    triggerManual: "Manual",
    // relative time (shared with the editor)
    justNow: "just now",
    minutesAgo: (n: number) => `${n}m ago`,
    hoursAgo: (n: number) => `${n}h ago`,
    daysAgo: (n: number) => `${n}d ago`,
  },
  pt: {
    title: "Fluxos",
    description: "Automações em grafo que rodam por gatilho ou sob demanda.",
    newWorkflow: "Novo fluxo",
    // tabs
    tabMine: "Meus fluxos",
    tabTemplates: "Modelos",
    tabDictionaries: "Tabelas de dicionário",
    // filters
    searchPlaceholder: "Buscar fluxos…",
    allGroups: "Todos os grupos",
    clear: "Limpar",
    noMatches: "Nenhum fluxo corresponde aos seus filtros.",
    clearFilters: "Limpar filtros",
    emptyTitle: "Nenhum fluxo ainda",
    emptyDescription:
      "Comece por um modelo pronto, ou monte um do zero para encadear câmeras, alertas, visão e destinatários.",
    browseTemplates: "Ver modelos",
    // row actions
    duplicate: "Duplicar",
    liveInstances: "Instâncias ativas — contextos de execução salvos",
    recentRuns: "Execuções recentes",
    delete: "Excluir",
    unmakeTemplate: "É um modelo — clique para torná-lo um fluxo normal",
    makeTemplate: "Salvar como modelo",
    unpublishGlobal: "Publicado globalmente — clique para despublicar",
    publishGlobal: "Publicar para todas as organizações (modelo global)",
    // run status / trigger source (shared with the editor)
    running: "Executando",
    triggerCron: "Agendado",
    triggerEvent: "Evento",
    triggerManual: "Manual",
    // relative time (shared with the editor)
    justNow: "agora mesmo",
    minutesAgo: (n: number) => `há ${n}min`,
    hoursAgo: (n: number) => `há ${n}h`,
    daysAgo: (n: number) => `há ${n}d`,
  },
};
