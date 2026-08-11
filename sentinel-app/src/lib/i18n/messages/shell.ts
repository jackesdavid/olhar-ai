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
    // Billing banner (#63). One line per state, because the ACTION differs: a failed payment is
    // fixed with a card, a lapsed one with a plan. Says what is at stake — recordings stop
    // reaching the cloud while a workspace is read-only — so the banner is worth reading once.
    pastDueTitle: "Your last payment failed.",
    pastDueBody:
      "Olhar AI is read-only and new recordings are staying on the appliance instead of reaching the cloud. Update your card to restore it.",
    updatePayment: "Update payment method",
    lapsedTitle: "Your subscription has ended.",
    lapsedBody:
      "Olhar AI is read-only and new recordings are staying on the appliance instead of reaching the cloud.",
    portalFailed: "Couldn't open the payment page",
    updateAvailable: "Update available — reload to get the latest version.",
    reload: "Reload",
    newAlert: (level: string) => `New ${level} alert`,
    // boot splash
    splashConnecting: "Initializing Olhar AI…",
    splashConnectingDetail: "Bringing services online",
    splashError: "Waiting for database…",
    splashErrorDetail: "Can't reach Postgres. Start it with “task db-up” — retrying…",
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
    // Banner de cobrança (#63) — uma linha por estado, porque a AÇÃO muda.
    pastDueTitle: "O último pagamento falhou.",
    pastDueBody:
      "O Olhar AI está somente leitura e as novas gravações estão ficando no appliance em vez de subir para a nuvem. Atualize seu cartão para restabelecer.",
    updatePayment: "Atualizar forma de pagamento",
    lapsedTitle: "Sua assinatura terminou.",
    lapsedBody:
      "O Olhar AI está somente leitura e as novas gravações estão ficando no appliance em vez de subir para a nuvem.",
    portalFailed: "Não foi possível abrir a página de pagamento",
    updateAvailable: "Atualização disponível — recarregue para obter a versão mais recente.",
    reload: "Recarregar",
    newAlert: (level: string) => `Novo alerta ${level}`,
    // boot splash
    splashConnecting: "Inicializando a Olhar AI…",
    splashConnectingDetail: "Colocando os serviços no ar",
    splashError: "Aguardando o banco de dados…",
    splashErrorDetail: "Não é possível alcançar o Postgres. Inicie com “task db-up” — tentando de novo…",
  },
};
