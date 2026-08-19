// Settings → Devices (/settings/devices): the saved-camera table, the edge-discovered list, the
// detail dialog (name + credentials + stream mode + the #88 credential probe) and add-by-hand.
export const devices = {
  en: {
    // Camera status (#155) — the stage, in the user's words. Each maps to a DIFFERENT action:
    // "locked" means power-cycle, "auth_failed" means fix the password. Conflating them is what
    // had people re-typing correct credentials at a camera that had stopped listening.
    status: {
      streaming: "Streaming",
      ok: "Connected, not yet streaming",
      unreachable: "Can't reach it on the network",
      timeout: "Not responding",
      auth_failed: "Credentials rejected",
      no_credentials: "No credentials set",
      locked: "Refusing all connections",
      not_found: "No stream at that path",
      not_on_this_box: "On a different network from this box",
      error: "Unrecognised response",
      unknown: "Not checked yet",
    },
    statusHint: {
      unreachable: "Check it's powered on and on the same network.",
      timeout: "It's reachable but stalled — try power-cycling it.",
      auth_failed: "Update the username and password.",
      no_credentials: "Add a username and password for this camera.",
      locked: "It's refusing every client — power-cycle it. Don't retry, that's what locks it.",
      not_found: "The stream path is wrong for this camera.",
      not_on_this_box: "Another box in this tenant serves it.",
    },
    checkedAt: "checked",
    never: "never",
    title: "Devices",
    description:
      "Save and manage the cameras your edge appliance finds on its local network.",
    rescan: "Rescan",
    rescanning: "Rescanning…",
    rescanTitle:
      "Re-run ONVIF discovery on your edge box now (fixes cameras that changed IP)",
    addManually: "Add manually",
    addManuallyTitle:
      "Add a camera by its IP when discovery can't find it (e.g. the network blocks discovery)",
    // saved table
    savedTitle: "Saved devices",
    savedDescription: "Cameras you've added, with stored credentials.",
    loadingDevices: "Loading devices…",
    emptyTitle: "No devices yet",
    emptyDescription:
      "When your edge appliance discovers cameras on its network, they'll appear below to add.",
    colName: "Name",
    colIp: "IP",
    colMakeModel: "Make / Model",
    colCredentials: "Credentials",
    colStatus: "Status",
    colActions: "Actions",
    colAction: "Action",
    codecTitle: "Video codec this camera is streaming, as detected by the edge",
    configured: "Configured",
    setupNeeded: "Setup needed",
    viewLogs: "View this camera's edge logs",
    editSnapshot: "Edit / snapshot",
    delete: "Delete",
    // camera status tooltips
    stateOnline:
      "Online — the edge has a working stream (credentials valid, camera reachable)",
    stateConfiguredOnLan:
      "Credentials set and camera visible, but no stream yet — check credentials, or it's still resolving",
    stateConfiguredOffline:
      "Credentials set, but no stream and not visible on the network (offline, or its IP changed — try Rescan)",
    stateOnLan: "Visible on the network — set its credentials to start streaming",
    stateUnknown: "Not set up, and not visible on the network",
    // edge-found
    foundTitle: "Found on your edge",
    foundDescription:
      "Cameras your edge appliance discovered on its network. Add one, then open it to set credentials.",
    add: "Add",
    // detail dialog
    device: "Device",
    displayName: "Display name",
    username: "Username",
    password: "Password",
    credsHint:
      "Set the camera's credentials — your edge resolves the stream on its own network. The RTSP URL stays on the edge.",
    streamMode: "Stream mode",
    split: "Split",
    splitHint: "Substream for preview + motion, main for HD/recording. Default.",
    unified: "Unified (HD)",
    unifiedHint: "Main stream only — sharp HD faces, no substream. One camera session.",
    unifiedNote:
      "Motion still runs (on a downscaled copy of the main frame). Use this when the camera can't serve two streams at once, or when you want HD faces.",
    provingCreds: (ip: string) =>
      `Asking ${ip} to prove these credentials… nothing is saved until it does.`,
    provingDetails: (ip: string) =>
      `Asking ${ip} to prove these details… nothing is saved until it does.`,
    liveFrameAlt: (ip: string) => `Live frame from ${ip}`,
    cameraSaid: (detail: string) => `The camera said: ${detail}`,
    testing: "Testing…",
    testAgain: "Test again",
    testAndSave: "Test & save",
    saving: "Saving…",
    saveWithoutTesting: "Save without testing",
    saveWithoutTestingTitle:
      "Send these credentials to the box without a live camera test — for a box that can't be reached for a probe, such as an edge running on a phone.",
    untestedHint:
      "Couldn't reach a box to test these credentials — no edge is online. If you're setting up an edge that checks in on its own schedule, you can save the credentials without testing and it'll pick them up on its next check-in.",
    // probe verdicts
    vOk: "Camera verified — the credentials work.",
    vAuthFailed: "The camera rejected these credentials.",
    vLocked: "The camera has locked its video stream.",
    vNotFound: "Signed in, but found no video stream.",
    vUnreachable: "Couldn't reach the camera.",
    vTimeout: "The camera didn't answer.",
    vError: "Couldn't test this camera.",
    aNone: "Something went wrong before we could reach the camera. Nothing was saved.",
    aAuthFailed:
      "Double-check the username and password. Many cameras need a separate account you create in their own app, not your cloud login. Nothing was saved.",
    aLocked:
      "It's refusing every connection, not just ours — cameras do this after repeated failed logins. Unplug it for about 30 seconds, then test again. Your saved credentials weren't changed.",
    aNotFound:
      "The credentials are right, but the camera didn't offer a stream at the expected path. It may need RTSP enabled in its own settings.",
    aUnreachable:
      "Nothing answered at this address. Check the camera is powered on and on the same network as your box — if its IP changed, try Rescan.",
    aTimeout:
      "It accepted the connection but never replied. It may be busy or half-crashed; a power cycle usually clears it.",
    aError: "Nothing was saved.",
    portAdvice: (port: string) =>
      `If the camera is reachable but this keeps failing, double-check the ONVIF port — it's ${port} here; Tapo uses 2020, many cameras use 80.`,
    // delete dialog
    deleteTitle: "Delete device?",
    deleteBefore: "Remove ",
    deleteAfter: (ip: string) => ` (${ip}) and its saved credentials. This can't be undone.`,
    deleting: "Deleting…",
    // add-by-hand dialog
    manualTitle: "Add a camera by hand",
    manualDescription:
      "For a camera your box can reach but can't discover — usually when the network blocks camera discovery. Enter its address and credentials; your edge resolves the stream the same way.",
    namePlaceholder: "e.g. Hall",
    ipAddress: "IP address",
    onvifPort: "ONVIF port",
    // source mode — ONVIF discovery vs a stream URL typed by hand
    sourceMode: "How your box reaches it",
    sourceOnvif: "ONVIF",
    sourceRtsp: "Stream URL",
    sourceOnvifHint:
      "The normal path. Your box asks the camera where its stream is, so you only supply the address.",
    sourceRtspHint:
      "For a source with no ONVIF at all — a phone running an RTSP app, or a camera whose ONVIF is broken. You supply the stream URL directly.",
    rtspUrl: "RTSP URL",
    rtspUrlPlaceholder: "rtsp://192.168.1.50:8554/live",
    rtspUrlHint:
      "Copy it from the app that is streaming. Any credentials it needs can be embedded in the URL (rtsp://user:pass@host/…); like the password, it is relayed to your box and never stored in the cloud.",
    rtspUrlInvalid: "Enter a URL starting with rtsp:// that includes a host.",
    portHintBefore: "The ONVIF port is the camera's control port — Tapo uses ",
    portHintMiddle: ", many others use ",
    portHintAfter:
      ". The camera's credentials are usually a separate account you create in its own app, not your cloud login.",
    addAndTest: "Add & test",
    done: "Done",
    // toasts
    manualAddFailed: "Couldn't add the camera",
    listFailed: "Failed to load devices",
    rescanRequested: "Rescan requested",
    rescanRequestedHint:
      "Your edge box is re-running discovery — moved cameras self-heal by ONVIF id.",
    rescanFailed: "Rescan failed",
    added: (name: string) => `Added ${name} — open it to add credentials.`,
    addFailed: "Failed to add camera",
    testFailed: "Couldn't test the camera",
    savedWithCreds: "Saved — credentials sent to your edge.",
    savedDevice: "Device saved.",
    saveFailed: "Failed to save device",
    removed: (name: string) => `Removed ${name}.`,
    deleteFailed: "Failed to delete device",
  },
  pt: {
    status: {
      streaming: "Transmitindo",
      ok: "Conectada, ainda sem transmitir",
      unreachable: "Não é possível alcançá-la na rede",
      timeout: "Sem resposta",
      auth_failed: "Credenciais recusadas",
      no_credentials: "Sem credenciais",
      locked: "Recusando todas as conexões",
      not_found: "Nenhum stream nesse caminho",
      not_on_this_box: "Em outra rede, fora desta box",
      error: "Resposta não reconhecida",
      unknown: "Ainda não verificada",
    },
    statusHint: {
      unreachable: "Verifique se está ligada e na mesma rede.",
      timeout: "Está acessível mas travada — tente reiniciá-la.",
      auth_failed: "Atualize o usuário e a senha.",
      no_credentials: "Adicione usuário e senha para esta câmera.",
      locked: "Está recusando todos os clientes — reinicie. Não tente de novo, é isso que trava.",
      not_found: "O caminho do stream está errado para esta câmera.",
      not_on_this_box: "Outra box deste tenant a atende.",
    },
    checkedAt: "verificado",
    never: "nunca",
    title: "Dispositivos",
    description:
      "Salve e gerencie as câmeras que o seu appliance encontra na rede local.",
    rescan: "Buscar de novo",
    rescanning: "Buscando…",
    rescanTitle:
      "Rodar a descoberta ONVIF no appliance agora (corrige câmeras que mudaram de IP)",
    addManually: "Adicionar manualmente",
    addManuallyTitle:
      "Adicione uma câmera pelo IP quando a descoberta não a encontra (por exemplo, a rede bloqueia a descoberta)",
    // saved table
    savedTitle: "Dispositivos salvos",
    savedDescription: "Câmeras que você adicionou, com credenciais armazenadas.",
    loadingDevices: "Carregando dispositivos…",
    emptyTitle: "Nenhum dispositivo ainda",
    emptyDescription:
      "Quando o seu appliance descobrir câmeras na rede, elas aparecerão abaixo para adicionar.",
    colName: "Nome",
    colIp: "IP",
    colMakeModel: "Marca / Modelo",
    colCredentials: "Credenciais",
    colStatus: "Status",
    colActions: "Ações",
    colAction: "Ação",
    codecTitle: "Codec de vídeo que esta câmera transmite, detectado pelo appliance",
    configured: "Configurada",
    setupNeeded: "Precisa configurar",
    viewLogs: "Ver os logs desta câmera no appliance",
    editSnapshot: "Editar / captura",
    delete: "Excluir",
    // camera status tooltips
    stateOnline:
      "Online — o appliance tem uma transmissão funcionando (credenciais válidas, câmera acessível)",
    stateConfiguredOnLan:
      "Credenciais definidas e câmera visível, mas ainda sem transmissão — verifique as credenciais, ou ainda está resolvendo",
    stateConfiguredOffline:
      "Credenciais definidas, mas sem transmissão e invisível na rede (offline, ou o IP mudou — tente Buscar de novo)",
    stateOnLan: "Visível na rede — defina as credenciais para começar a transmitir",
    stateUnknown: "Não configurada e não visível na rede",
    // edge-found
    foundTitle: "Encontradas pelo seu appliance",
    foundDescription:
      "Câmeras que o seu appliance descobriu na rede. Adicione uma e depois abra para definir as credenciais.",
    add: "Adicionar",
    // detail dialog
    device: "Dispositivo",
    displayName: "Nome de exibição",
    username: "Usuário",
    password: "Senha",
    credsHint:
      "Defina as credenciais da câmera — o seu appliance resolve a transmissão na própria rede. A URL RTSP fica no appliance.",
    streamMode: "Modo de transmissão",
    split: "Dividido",
    splitHint: "Substream para prévia + movimento, principal para HD/gravação. Padrão.",
    unified: "Unificado (HD)",
    unifiedHint: "Só o stream principal — rostos nítidos em HD, sem substream. Uma sessão por câmera.",
    unifiedNote:
      "A detecção de movimento continua rodando (sobre uma cópia reduzida do quadro principal). Use quando a câmera não consegue servir dois streams ao mesmo tempo, ou quando você quer rostos em HD.",
    provingCreds: (ip: string) =>
      `Pedindo a ${ip} que comprove estas credenciais… nada é salvo até que comprove.`,
    provingDetails: (ip: string) =>
      `Pedindo a ${ip} que comprove estes dados… nada é salvo até que comprove.`,
    liveFrameAlt: (ip: string) => `Quadro ao vivo de ${ip}`,
    cameraSaid: (detail: string) => `A câmera respondeu: ${detail}`,
    testing: "Testando…",
    testAgain: "Testar de novo",
    testAndSave: "Testar e salvar",
    saving: "Salvando…",
    saveWithoutTesting: "Salvar sem testar",
    saveWithoutTestingTitle:
      "Enviar estas credenciais ao appliance sem um teste ao vivo na câmera — para um appliance que não pode ser sondado, como um edge rodando em um celular.",
    untestedHint:
      "Não foi possível alcançar um appliance para testar estas credenciais — nenhum edge está online. Se você está configurando um edge que se reporta no próprio ritmo, pode salvar as credenciais sem testar e ele as pegará na próxima consulta.",
    // probe verdicts
    vOk: "Câmera verificada — as credenciais funcionam.",
    vAuthFailed: "A câmera rejeitou estas credenciais.",
    vLocked: "A câmera bloqueou a transmissão de vídeo.",
    vNotFound: "Autenticou, mas nenhuma transmissão de vídeo foi encontrada.",
    vUnreachable: "Não foi possível alcançar a câmera.",
    vTimeout: "A câmera não respondeu.",
    vError: "Não foi possível testar esta câmera.",
    aNone: "Algo deu errado antes de conseguirmos alcançar a câmera. Nada foi salvo.",
    aAuthFailed:
      "Confira o usuário e a senha. Muitas câmeras exigem uma conta separada criada no aplicativo delas, não o seu login da nuvem. Nada foi salvo.",
    aLocked:
      "Ela está recusando todas as conexões, não só as nossas — câmeras fazem isso após várias tentativas de login falhas. Desligue-a por uns 30 segundos e teste de novo. Suas credenciais salvas não foram alteradas.",
    aNotFound:
      "As credenciais estão certas, mas a câmera não ofereceu transmissão no caminho esperado. Pode ser necessário habilitar RTSP nas configurações dela.",
    aUnreachable:
      "Nada respondeu neste endereço. Verifique se a câmera está ligada e na mesma rede do appliance — se o IP mudou, tente Buscar de novo.",
    aTimeout:
      "Ela aceitou a conexão, mas nunca respondeu. Pode estar ocupada ou travada; reiniciar costuma resolver.",
    aError: "Nada foi salvo.",
    portAdvice: (port: string) =>
      `Se a câmera está acessível e isso continua falhando, confira a porta ONVIF — aqui está ${port}; a Tapo usa 2020, muitas câmeras usam 80.`,
    // delete dialog
    deleteTitle: "Excluir dispositivo?",
    deleteBefore: "Remover ",
    deleteAfter: (ip: string) =>
      ` (${ip}) e suas credenciais salvas. Isso não pode ser desfeito.`,
    deleting: "Excluindo…",
    // add-by-hand dialog
    manualTitle: "Adicionar uma câmera manualmente",
    manualDescription:
      "Para uma câmera que o appliance alcança mas não descobre — normalmente quando a rede bloqueia a descoberta. Informe o endereço e as credenciais; o appliance resolve a transmissão do mesmo jeito.",
    namePlaceholder: "ex.: Corredor",
    ipAddress: "Endereço IP",
    onvifPort: "Porta ONVIF",
    // modo da fonte — descoberta ONVIF ou URL de transmissão digitada
    sourceMode: "Como o appliance chega nela",
    sourceOnvif: "ONVIF",
    sourceRtsp: "URL de transmissão",
    sourceOnvifHint:
      "O caminho normal. O appliance pergunta à câmera onde está a transmissão, então você informa só o endereço.",
    sourceRtspHint:
      "Para uma fonte sem ONVIF nenhum — um celular rodando um app de RTSP, ou uma câmera com ONVIF quebrado. Você informa a URL da transmissão direto.",
    rtspUrl: "URL RTSP",
    rtspUrlPlaceholder: "rtsp://192.168.1.50:8554/live",
    rtspUrlHint:
      "Copie do app que está transmitindo. Se precisar de credenciais, elas podem ir na própria URL (rtsp://usuario:senha@host/…); como a senha, ela é repassada ao appliance e nunca fica guardada na nuvem.",
    rtspUrlInvalid: "Informe uma URL começando com rtsp:// e que inclua um host.",
    portHintBefore: "A porta ONVIF é a porta de controle da câmera — a Tapo usa ",
    portHintMiddle: ", muitas outras usam ",
    portHintAfter:
      ". As credenciais da câmera geralmente são de uma conta separada criada no aplicativo dela, não o seu login da nuvem.",
    addAndTest: "Adicionar e testar",
    done: "Concluído",
    // toasts
    manualAddFailed: "Não foi possível adicionar a câmera",
    listFailed: "Falha ao carregar os dispositivos",
    rescanRequested: "Nova busca solicitada",
    rescanRequestedHint:
      "Seu appliance está rodando a descoberta de novo — câmeras que mudaram de IP se corrigem pelo id ONVIF.",
    rescanFailed: "Falha ao buscar de novo",
    added: (name: string) => `${name} adicionada — abra para informar as credenciais.`,
    addFailed: "Falha ao adicionar a câmera",
    testFailed: "Não foi possível testar a câmera",
    savedWithCreds: "Salvo — credenciais enviadas ao seu appliance.",
    savedDevice: "Dispositivo salvo.",
    saveFailed: "Falha ao salvar o dispositivo",
    removed: (name: string) => `${name} removida.`,
    deleteFailed: "Falha ao excluir o dispositivo",
  },
};
