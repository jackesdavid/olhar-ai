// App chrome: the sidebar frame, the session footer, the top banners and the app-wide toasts
// that +layout.svelte owns. Page copy does NOT belong here — it goes in that page's module.
export const shell = {
  en: {
    expandSidebar: "Expand sidebar",
    collapseSidebar: "Collapse sidebar",
    signedIn: "Signed in",
    signOut: "Sign out",
    switchToLight: "Switch to light",
    switchToDark: "Switch to dark",
    deployedCommit: "Deployed commit",
    assumingBanner:
      "Acting as another tenant — you have full access to this workspace, including live feeds.",
    exit: "Exit",
    choosePlan: "Choose a plan",
    updateAvailable: "Update available — reload to get the latest version.",
    reload: "Reload",
    newAlert: (level: string) => `New ${level} alert`,
  },
  pt: {
    expandSidebar: "Expandir menu",
    collapseSidebar: "Recolher menu",
    signedIn: "Conectado",
    signOut: "Sair",
    switchToLight: "Mudar para claro",
    switchToDark: "Mudar para escuro",
    deployedCommit: "Commit publicado",
    assumingBanner:
      "Operando como outra organização — você tem acesso total a este workspace, inclusive às câmeras ao vivo.",
    exit: "Sair",
    choosePlan: "Escolher um plano",
    updateAvailable: "Atualização disponível — recarregue para obter a versão mais recente.",
    reload: "Recarregar",
    newAlert: (level: string) => `Novo alerta ${level}`,
  },
};
