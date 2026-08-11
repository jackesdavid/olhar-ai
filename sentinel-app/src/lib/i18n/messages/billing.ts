// The Billing page (/billing) — trial banner, current plan, storage-this-period, the plan cards,
// the invoice table and the subscribe dialog. Plan names and invoice periods are backend data.
import { PRODUCT } from "$lib/brand";

export const billing = {
  en: {
    title: "Billing",
    description: "Your plan, payment, and invoices.",
    trialEnded: "Your trial has ended — pick a plan to continue.",
    trialLeft: (days: number) =>
      `Trial — ${days} day${days === 1 ? "" : "s"} left. Pick a plan to keep ${PRODUCT} after it ends.`,
    // current plan
    currentPlanTitle: "Current plan",
    noPlan: "No plan",
    planSummary: (price: string, cameras: number) => `${price}/mo · up to ${cameras} cameras`,
    chooseBelow: "Choose a plan below.",
    // subscription status
    statusActive: "active",
    statusTrialing: "trialing",
    statusPastDue: "past due",
    statusCanceled: "canceled",
    statusExpired: "expired",
    // storage
    storageTitle: "Storage this period",
    storedOf: (gb: number) => ` stored of ${gb} GB`,
    retention: (days: number) => `${days}-day cloud retention`,
    billedThisMonth: "Billed this month",
    gbMonthAvg: " · GB-month avg",
    overageSoFar: "Overage so far",
    withinPlan: " · within plan",
    storageNote: (clips: number) =>
      `${clips} clip${clips === 1 ? "" : "s"} in the cloud. GB-month is the average footage held across the month — the quantity your invoice bills, not the peak.`,
    noCloudPlan:
      "Cloud recording isn't included in your current plan. Choose a plan with cloud retention to keep footage off-site.",
    // plan cards
    current: "current",
    free: "Free",
    perMonth: "/mo",
    upToCameras: (n: number) => `Up to ${n} cameras`,
    trialDays: (n: number) => `${n}-day free trial`,
    currentPlanButton: "Current plan",
    // The provider's hosted page: update the card, see invoices, cancel. The correct answer to a
    // failed payment — subscribing again would open a second subscription.
    managePayment: "Manage payment method",
    manageInstead: "Manage your subscription",
    // Change plan — edits the subscription they have, prorated. Never a second subscription.
    switchToPlan: "Switch to this plan",
    switchNamed: (plan: string) => `Switch to ${plan}`,
    switchSummary: (price: string, cameras: number) =>
      `${price} per month, up to ${cameras} cameras.`,
    prorationNote:
      "We'll charge the difference for the rest of this billing period on your next invoice — same card, same subscription.",
    confirmSwitch: "Switch plan",
    switching: "Switching…",
    cancel: "Cancel",
    planChanged: (plan: string) => `You're on ${plan} now.`,
    planChangeFailed: "Couldn't change the plan",
    portalFailed: "Couldn't open the payment page",
    trialOnly: "Trial only",
    choosePlan: (name: string) => `Choose ${name}`,
    // invoices
    invoices: "Invoices",
    colPeriod: "Period",
    colPlan: "Plan",
    colStatus: "Status",
    colTotal: "Total",
    noInvoicesTitle: "No invoices yet",
    noInvoicesDescription: "Invoices appear here once a billing period closes.",
    // subscribe dialog
    subscribe: "Subscribe",
    subscribeNamed: (plan: string) => `Subscribe — ${plan}`,
    subscribeSummary: (price: string, cameras: number) =>
      `${price}/mo, up to ${cameras} cameras.`,
    paymentMethod: "Payment method",
    noMethods: "No payment method is available right now. Please try again shortly.",
    methodCard: "Card",
    methodBoleto: "Boleto",
    methodPix: "Pix",
    fullName: "Full name",
    fullNamePlaceholder: "Maria Silva",
    email: "Email",
    emailPlaceholder: "maria@empresa.com.br",
    taxIdLabel: "CPF / CNPJ",
    optional: "(optional)",
    confirmSubscription: "Confirm subscription",
    // toasts
    loadFailed: "Failed to load billing",
    openingPayment: "Opening your payment page — your plan activates once payment clears.",
    subscribed: "Subscribed — your charge is on its way.",
    subscribeFailed: "Failed to subscribe",
  },
  pt: {
    title: "Cobrança",
    description: "Seu plano, pagamento e faturas.",
    trialEnded: "Seu período de teste acabou — escolha um plano para continuar.",
    trialLeft: (days: number) =>
      `Teste — ${days} dia${days === 1 ? "" : "s"} restante${days === 1 ? "" : "s"}. Escolha um plano para manter o ${PRODUCT} depois que acabar.`,
    // current plan
    currentPlanTitle: "Plano atual",
    noPlan: "Sem plano",
    planSummary: (price: string, cameras: number) => `${price}/mês · até ${cameras} câmeras`,
    chooseBelow: "Escolha um plano abaixo.",
    // subscription status
    statusActive: "ativa",
    statusTrialing: "em teste",
    statusPastDue: "em atraso",
    statusCanceled: "cancelada",
    statusExpired: "expirada",
    // storage
    storageTitle: "Armazenamento neste período",
    storedOf: (gb: number) => ` armazenados de ${gb} GB`,
    retention: (days: number) => `retenção na nuvem de ${days} dias`,
    billedThisMonth: "Cobrado neste mês",
    gbMonthAvg: " · média GB-mês",
    overageSoFar: "Excedente até agora",
    withinPlan: " · dentro do plano",
    storageNote: (clips: number) =>
      `${clips} clipe${clips === 1 ? "" : "s"} na nuvem. GB-mês é a média de imagens mantidas ao longo do mês — a quantidade que sua fatura cobra, não o pico.`,
    noCloudPlan:
      "A gravação na nuvem não está incluída no seu plano atual. Escolha um plano com retenção na nuvem para manter as imagens fora do local.",
    // plan cards
    current: "atual",
    free: "Grátis",
    perMonth: "/mês",
    upToCameras: (n: number) => `Até ${n} câmeras`,
    trialDays: (n: number) => `${n} dias de teste grátis`,
    currentPlanButton: "Plano atual",
    // Página do provedor: atualizar cartão, ver faturas, cancelar.
    managePayment: "Gerenciar forma de pagamento",
    manageInstead: "Gerencie sua assinatura",
    // Trocar de plano — edita a assinatura existente, com rateio. Nunca cria uma segunda.
    switchToPlan: "Mudar para este plano",
    switchNamed: (plan: string) => `Mudar para ${plan}`,
    switchSummary: (price: string, cameras: number) =>
      `${price} por mês, até ${cameras} câmeras.`,
    prorationNote:
      "Cobramos a diferença do restante deste ciclo na próxima fatura — mesmo cartão, mesma assinatura.",
    confirmSwitch: "Mudar de plano",
    switching: "Mudando…",
    cancel: "Cancelar",
    planChanged: (plan: string) => `Agora você está no ${plan}.`,
    planChangeFailed: "Não foi possível mudar o plano",
    portalFailed: "Não foi possível abrir a página de pagamento",
    trialOnly: "Somente teste",
    choosePlan: (name: string) => `Escolher ${name}`,
    // invoices
    invoices: "Faturas",
    colPeriod: "Período",
    colPlan: "Plano",
    colStatus: "Situação",
    colTotal: "Total",
    noInvoicesTitle: "Nenhuma fatura ainda",
    noInvoicesDescription: "As faturas aparecem aqui quando um período de cobrança fecha.",
    // subscribe dialog
    subscribe: "Assinar",
    subscribeNamed: (plan: string) => `Assinar — ${plan}`,
    subscribeSummary: (price: string, cameras: number) =>
      `${price}/mês, até ${cameras} câmeras.`,
    paymentMethod: "Forma de pagamento",
    noMethods: "Nenhuma forma de pagamento disponível agora. Tente de novo em instantes.",
    methodCard: "Cartão",
    methodBoleto: "Boleto",
    methodPix: "Pix",
    fullName: "Nome completo",
    fullNamePlaceholder: "Maria Silva",
    email: "E-mail",
    emailPlaceholder: "maria@empresa.com.br",
    taxIdLabel: "CPF / CNPJ",
    optional: "(opcional)",
    confirmSubscription: "Confirmar assinatura",
    // toasts
    loadFailed: "Falha ao carregar a cobrança",
    openingPayment:
      "Abrindo sua página de pagamento — seu plano é ativado assim que o pagamento é compensado.",
    subscribed: "Assinatura feita — sua cobrança está a caminho.",
    subscribeFailed: "Falha ao assinar",
  },
};
