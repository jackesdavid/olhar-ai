// Settings → Edge boxes → Install (/settings/boxes/install): the pairing flow (download the
// installer, enter the code, watch the box come online) and the headless-command fallback.
import { PRODUCT } from "$lib/brand";

export const boxInstall = {
  en: {
    backToBoxes: "Edge boxes",
    connectTitle: (name: string) => `Connect "${name}"`,
    connectSubtitle: `Install ${PRODUCT} on the on-site machine, then enter this code there.`,
    // online
    onlineTitle: (name: string) => `"${name}" is online.`,
    onlineHint: "The appliance checked in. You can add its cameras now.",
    // accepted
    acceptedTitle: (name: string) => `Code accepted — "${name}" is starting up…`,
    acceptedHint: "The appliance is connecting. This usually takes a few seconds.",
    stalledHint:
      "Taking longer than expected. Make sure the appliance is still running — check its logs, or reinstall.",
    // installer
    installStep: "1 · Install on the appliance",
    installDescription: "Download and run this on the on-site machine.",
    downloadFor: (label: string) => `Download for ${label}`,
    oneLineCommand: "one-line command",
    hide: "Hide",
    show: "Show",
    otherPlatforms: "other platforms",
    // pairing code
    pairingStepPrefix: "2 · ",
    pairingCode: "Pairing code",
    pairingDescription:
      "When the installer finishes, it opens a page on the appliance asking for this code.",
    copyCode: "Copy code",
    expired: "This code has expired — generate a new one.",
    expiresIn: (mins: number) => `Expires in ${mins} minute${mins === 1 ? "" : "s"}.`,
    newCode: "New code",
    waitingFor: (name: string) => `Waiting for "${name}" to check in…`,
    // headless
    headlessToggle: "the headless install command",
    headlessTitle: "Headless install",
    headlessDescription:
      "For a machine with no screen: this enrolls the box directly, so no pairing code is needed. It carries the box token — treat it like a password, and note it's shown only once.",
    copyCommand: "Copy command",
    boxToken: "Box token",
    copy: "Copy",
    nothingElse:
      "Nothing else to configure — the box connects to the cloud on its own, outbound only.",
    // footer
    done: "Done",
    later: "I'll do this later",
    // toasts
    codeFailed: "Couldn't create a pairing code",
    copied: "Copied to clipboard",
    copyFailed: "Couldn't copy — select and copy manually.",
  },
  pt: {
    backToBoxes: "Appliances",
    connectTitle: (name: string) => `Conectar "${name}"`,
    connectSubtitle: `Instale o ${PRODUCT} na máquina do local e depois informe este código lá.`,
    // online
    onlineTitle: (name: string) => `"${name}" está online.`,
    onlineHint: "O appliance se conectou. Você já pode adicionar as câmeras dele.",
    // accepted
    acceptedTitle: (name: string) => `Código aceito — "${name}" está iniciando…`,
    acceptedHint: "O appliance está conectando. Isso costuma levar alguns segundos.",
    stalledHint:
      "Está demorando mais que o esperado. Verifique se o appliance ainda está rodando — confira os logs dele, ou reinstale.",
    // installer
    installStep: "1 · Instalar no appliance",
    installDescription: "Baixe e execute isto na máquina do local.",
    downloadFor: (label: string) => `Baixar para ${label}`,
    oneLineCommand: "comando de uma linha",
    hide: "Ocultar",
    show: "Mostrar",
    otherPlatforms: "outras plataformas",
    // pairing code
    pairingStepPrefix: "2 · ",
    pairingCode: "Código de pareamento",
    pairingDescription:
      "Quando o instalador terminar, ele abre uma página no appliance pedindo este código.",
    copyCode: "Copiar código",
    expired: "Este código expirou — gere um novo.",
    expiresIn: (mins: number) => `Expira em ${mins} minuto${mins === 1 ? "" : "s"}.`,
    newCode: "Novo código",
    waitingFor: (name: string) => `Aguardando "${name}" se conectar…`,
    // headless
    headlessToggle: "o comando de instalação headless",
    headlessTitle: "Instalação headless",
    headlessDescription:
      "Para uma máquina sem tela: isto registra o appliance direto, sem precisar de código de pareamento. Ele carrega o token do appliance — trate como uma senha, e note que é exibido apenas uma vez.",
    copyCommand: "Copiar comando",
    boxToken: "Token do appliance",
    copy: "Copiar",
    nothingElse:
      "Nada mais a configurar — o appliance se conecta à nuvem sozinho, apenas de saída.",
    // footer
    done: "Concluído",
    later: "Faço isso depois",
    // toasts
    codeFailed: "Não foi possível criar um código de pareamento",
    copied: "Copiado para a área de transferência",
    copyFailed: "Não foi possível copiar — selecione e copie manualmente.",
  },
};
