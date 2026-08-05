// Published legal documents (#167) — rendered at /terms and /privacy.
//
// THE SOURCE OF TRUTH IS THIS FILE. It is versioned in git next to the code it describes, which is
// the point: the Privacy Policy makes claims about retention, deletion and sub-processors that are
// only true because of specific code, and keeping them in the same repository is what lets a change
// to one surface as a diff against the other.
//
// ⚠️  Bump `TERMS_VERSION` / `PRIVACY_VERSION` in `crates/app-core/src/legal.rs` in the SAME COMMIT
//     as any edit here. Consent rows record the version a user accepted; a document that changes
//     without its version moving makes every stored acceptance point at the wrong text.
//
// ⚠️  UNREVIEWED DRAFT. Every factual claim below was checked against the implementation, but this
//     has NOT been reviewed by a lawyer. The bracketed placeholders must be filled and the whole
//     text reviewed before taking consumer money. See docs/legal/README.md.

export type Section = { heading: string; body: string[] };
export type Document = { title: string; updated: string; intro: string; sections: Section[] };

/** Fill these before publishing. They appear verbatim in both documents. */
export const OPERATOR = {
  entity: "[LEGAL ENTITY NAME]",
  address: "[REGISTERED ADDRESS]",
  contact: "[CONTACT EMAIL]",
  privacyContact: "[PRIVACY / DPO EMAIL]",
};

const UPDATED = "2026-08-04";

export const terms: { en: Document; pt: Document } = {
  en: {
    title: "Terms of Service",
    updated: UPDATED,
    intro:
      `These terms govern your use of Olhar AI, operated by ${OPERATOR.entity} ("we"). By creating ` +
      "an account you agree to them. If you do not agree, do not create an account.",
    sections: [
      {
        heading: "1. What the service does",
        body: [
          "Olhar AI connects to cameras you already own, on a network you control, through an on-premise appliance (the “edge”). It detects motion, recognises objects and faces you have enrolled, records clips, and runs automations you configure.",
          "The service is provided as-is for monitoring and automation. It is not a monitored alarm service, not an emergency service, and not a substitute for one. We do not dispatch help and we do not guarantee that any event will be detected or delivered.",
        ],
      },
      {
        heading: "2. Your account and workspace",
        body: [
          "You are responsible for the accuracy of your account details, for keeping your password and devices secure, and for everything done under your account.",
          "A workspace may have multiple users with different roles. The workspace owner is responsible for who is invited and what permissions they hold.",
        ],
      },
      {
        heading: "3. Your responsibilities when filming people",
        body: [
          "You are the controller of the footage you capture. You are responsible for having a lawful basis to film the areas you point cameras at, and for informing people who may be recorded.",
          "Face recognition creates biometric data. In most jurisdictions — including under the GDPR and the LGPD — enrolling a person’s face requires their explicit, informed consent. You must obtain it. Do not enrol people who have not agreed.",
          "You must not use the service to film areas where people have a reasonable expectation of privacy, to harass or track individuals, or for any purpose prohibited by law.",
        ],
      },
      {
        heading: "4. Plans, trials and payment",
        body: [
          "New workspaces start on a trial. When it ends, continued use of paid features requires an active subscription.",
          "Subscriptions are billed in advance through our payment processor. Prices, camera limits and retention periods are those shown for your plan at the time of purchase.",
          "If a payment fails, the workspace may become read-only until it is resolved. Stored recordings remain subject to the retention period of the plan under which they were recorded.",
        ],
      },
      {
        heading: "5. Cancellation and deletion",
        body: [
          "You may cancel at any time; the subscription runs to the end of the paid period. You may request deletion of your workspace and its data at any time.",
          "On deletion we remove your recordings and stored data from our systems. Backups and payment records retained for legal or accounting obligations are the exception, and are described in the Privacy Policy.",
        ],
      },
      {
        heading: "6. Availability",
        body: [
          "We aim for high availability but do not promise uninterrupted service. Cameras, networks, power and third-party providers fail; the edge appliance continues to record locally when the cloud is unreachable, but no system is continuous.",
          "We may change, suspend or discontinue features. Where a change materially reduces what you have paid for, we will tell you in advance.",
        ],
      },
      {
        heading: "7. Liability",
        body: [
          "To the fullest extent the law allows, we are not liable for indirect or consequential loss, for loss of footage, or for events the service did not detect or notify you about.",
          "Nothing here excludes liability that cannot lawfully be excluded.",
        ],
      },
      {
        heading: "8. Changes to these terms",
        body: [
          "We may update these terms. The version you accepted is recorded with your account, and material changes will be notified in advance.",
        ],
      },
      {
        heading: "9. Contact",
        body: [`${OPERATOR.entity}, ${OPERATOR.address}. Questions: ${OPERATOR.contact}.`],
      },
    ],
  },
  pt: {
    title: "Termos de Serviço",
    updated: UPDATED,
    intro:
      `Estes termos regem o uso do Olhar AI, operado por ${OPERATOR.entity} (“nós”). Ao criar uma ` +
      "conta, você concorda com eles. Se não concordar, não crie uma conta.",
    sections: [
      {
        heading: "1. O que o serviço faz",
        body: [
          "O Olhar AI conecta-se a câmeras que você já possui, em uma rede que você controla, por meio de um appliance local (o “edge”). Ele detecta movimento, reconhece objetos e rostos que você cadastrou, grava clipes e executa automações que você configura.",
          "O serviço é fornecido no estado em que se encontra, para monitoramento e automação. Não é um serviço de alarme monitorado, não é serviço de emergência e não substitui um. Não acionamos socorro e não garantimos que qualquer evento será detectado ou entregue.",
        ],
      },
      {
        heading: "2. Sua conta e seu espaço de trabalho",
        body: [
          "Você é responsável pela veracidade dos seus dados, por manter sua senha e dispositivos seguros e por tudo o que for feito em sua conta.",
          "Um espaço de trabalho pode ter vários usuários com papéis distintos. O responsável pelo espaço responde por quem é convidado e por quais permissões recebe.",
        ],
      },
      {
        heading: "3. Suas responsabilidades ao filmar pessoas",
        body: [
          "Você é o controlador das imagens que captura. É sua a responsabilidade de ter base legal para filmar as áreas para onde aponta as câmeras e de informar as pessoas que podem ser gravadas.",
          "O reconhecimento facial gera dado biométrico. Na maioria das jurisdições — inclusive sob o GDPR e a LGPD — cadastrar o rosto de uma pessoa exige consentimento explícito e informado. É você quem deve obtê-lo. Não cadastre pessoas que não concordaram.",
          "Você não deve usar o serviço para filmar locais onde há expectativa razoável de privacidade, para assediar ou rastrear indivíduos, nem para qualquer finalidade proibida por lei.",
        ],
      },
      {
        heading: "4. Planos, testes e pagamento",
        body: [
          "Novos espaços de trabalho começam em período de teste. Ao final dele, o uso continuado dos recursos pagos exige assinatura ativa.",
          "As assinaturas são cobradas antecipadamente por meio do nosso processador de pagamentos. Preços, limites de câmeras e prazos de retenção são os exibidos para o seu plano no momento da contratação.",
          "Se um pagamento falhar, o espaço pode ficar somente leitura até a regularização. As gravações permanecem sujeitas ao prazo de retenção do plano sob o qual foram gravadas.",
        ],
      },
      {
        heading: "5. Cancelamento e exclusão",
        body: [
          "Você pode cancelar a qualquer momento; a assinatura vigora até o fim do período pago. Você pode solicitar a exclusão do seu espaço de trabalho e dos seus dados a qualquer momento.",
          "Na exclusão, removemos suas gravações e dados dos nossos sistemas. Backups e registros de pagamento mantidos por obrigação legal ou contábil são a exceção, descrita na Política de Privacidade.",
        ],
      },
      {
        heading: "6. Disponibilidade",
        body: [
          "Buscamos alta disponibilidade, mas não prometemos serviço ininterrupto. Câmeras, redes, energia e provedores terceiros falham; o appliance continua gravando localmente quando a nuvem está inacessível, mas nenhum sistema é contínuo.",
          "Podemos alterar, suspender ou descontinuar funcionalidades. Quando uma mudança reduzir de forma relevante aquilo que você pagou, avisaremos com antecedência.",
        ],
      },
      {
        heading: "7. Responsabilidade",
        body: [
          "Na máxima extensão permitida em lei, não respondemos por danos indiretos ou consequenciais, por perda de imagens, nem por eventos que o serviço não detectou ou não notificou.",
          "Nada aqui exclui responsabilidade que não possa ser legalmente excluída.",
        ],
      },
      {
        heading: "8. Alterações destes termos",
        body: [
          "Podemos atualizar estes termos. A versão que você aceitou fica registrada na sua conta, e mudanças relevantes serão avisadas com antecedência.",
        ],
      },
      {
        heading: "9. Contato",
        body: [`${OPERATOR.entity}, ${OPERATOR.address}. Dúvidas: ${OPERATOR.contact}.`],
      },
    ],
  },
};

export const privacy: { en: Document; pt: Document } = {
  en: {
    title: "Privacy Policy",
    updated: UPDATED,
    intro:
      `${OPERATOR.entity} operates Olhar AI. This policy explains what we collect, why, how long we ` +
      "keep it, and how you get it deleted. It describes what the system actually does.",
    sections: [
      {
        heading: "1. What we collect",
        body: [
          "Account data: your email address, workspace name, roles and permissions, and login events.",
          "Video and images: clips recorded by your cameras, snapshots, and the frames attached to events — stored in our object storage.",
          "Events: motion, object and face detections, with timestamps, camera, and detection metadata such as confidence, size, sharpness and head pose.",
          "Biometric data: mathematical representations (embeddings) of faces enrolled in your workspace, plus the face crops they were computed from. This is sensitive personal data.",
          "Device data: cameras discovered on your network, appliance health, and diagnostic logs.",
          "Billing data: subscription and invoice records. Card details are handled by our payment processor and never reach our servers.",
        ],
      },
      {
        heading: "2. What we deliberately do not collect",
        body: [
          "Camera passwords and RTSP URLs never leave your premises. They are held only by the on-premise appliance, in local configuration, and are never persisted in our cloud database. This is an architectural rule of the system, not a policy preference.",
        ],
      },
      {
        heading: "3. Why we process it",
        body: [
          "To provide the service you configured: detecting events, recording clips, running your automations and delivering notifications.",
          "To bill you, support you, keep the service secure, and comply with legal obligations.",
          "Face recognition runs only where you have enrolled people. It is not enabled by default across the service.",
        ],
      },
      {
        heading: "4. Who your data is shared with",
        body: [
          "Cloud hosting and object storage providers, which store the service and your recordings.",
          "Our payment processor, for subscriptions and invoices.",
          "Our email provider, to send account messages such as password resets.",
          "Notification channels you connect yourself, such as a messaging bot, receive only what your automations send them.",
          "AI providers, where you use assistant or vision features that call them. We do not sell your data, and we do not use your footage to train models.",
        ],
      },
      {
        heading: "5. How long we keep it",
        body: [
          "Recordings are stored under the retention period of your plan and are automatically expired by the storage provider’s lifecycle rules once it passes. A clip keeps the retention it was stored under.",
          "Events are swept on a schedule according to your plan’s event retention setting.",
          "Account and billing records are kept while your account exists, and afterwards only where an accounting or legal obligation requires it.",
        ],
      },
      {
        heading: "6. Your rights",
        body: [
          "You can access, correct, export or delete your data. Requesting deletion of a workspace removes its stored objects across every retention tier.",
          "Under the LGPD and the GDPR you may also object to processing, withdraw consent, and complain to a supervisory authority.",
          `To exercise any of these, contact ${OPERATOR.privacyContact}.`,
        ],
      },
      {
        heading: "7. Security",
        body: [
          "Traffic is encrypted in transit. Access is scoped per workspace and per role, appliances authenticate with per-device signing keys, and sessions can be revoked centrally.",
          "No system is perfectly secure. If a breach affects your personal data, we will notify you and the relevant authority as the law requires.",
        ],
      },
      {
        heading: "8. International transfers",
        body: [
          "Our providers may process data outside your country. Where that happens, transfers rely on the safeguards those providers offer under applicable law.",
        ],
      },
      {
        heading: "9. Changes and contact",
        body: [
          "We will publish any update here and record the version you accepted with your account.",
          `${OPERATOR.entity}, ${OPERATOR.address}. Privacy contact: ${OPERATOR.privacyContact}.`,
        ],
      },
    ],
  },
  pt: {
    title: "Política de Privacidade",
    updated: UPDATED,
    intro:
      `${OPERATOR.entity} opera o Olhar AI. Esta política explica o que coletamos, por quê, por ` +
      "quanto tempo guardamos e como você solicita a exclusão. Ela descreve o que o sistema de fato faz.",
    sections: [
      {
        heading: "1. O que coletamos",
        body: [
          "Dados de conta: seu e-mail, nome do espaço de trabalho, papéis e permissões, e eventos de login.",
          "Vídeo e imagens: clipes gravados pelas suas câmeras, capturas e os quadros anexados aos eventos — armazenados em nosso armazenamento de objetos.",
          "Eventos: detecções de movimento, objetos e rostos, com data/hora, câmera e metadados como confiança, tamanho, nitidez e orientação da cabeça.",
          "Dados biométricos: representações matemáticas (embeddings) de rostos cadastrados no seu espaço, e os recortes de rosto a partir dos quais foram calculadas. Trata-se de dado pessoal sensível.",
          "Dados de dispositivo: câmeras descobertas na sua rede, saúde do appliance e logs de diagnóstico.",
          "Dados de cobrança: registros de assinatura e faturas. Os dados do cartão são tratados pelo processador de pagamentos e não chegam aos nossos servidores.",
        ],
      },
      {
        heading: "2. O que deliberadamente não coletamos",
        body: [
          "Senhas de câmera e URLs RTSP nunca saem do seu local. Ficam apenas no appliance, em configuração local, e nunca são persistidas no nosso banco de dados na nuvem. Isso é uma regra de arquitetura do sistema, não uma preferência de política.",
        ],
      },
      {
        heading: "3. Por que tratamos esses dados",
        body: [
          "Para prestar o serviço que você configurou: detectar eventos, gravar clipes, executar suas automações e entregar notificações.",
          "Para cobrar, dar suporte, manter o serviço seguro e cumprir obrigações legais.",
          "O reconhecimento facial só funciona onde você cadastrou pessoas. Não vem ativado por padrão em todo o serviço.",
        ],
      },
      {
        heading: "4. Com quem compartilhamos",
        body: [
          "Provedores de hospedagem e de armazenamento de objetos, que guardam o serviço e suas gravações.",
          "Nosso processador de pagamentos, para assinaturas e faturas.",
          "Nosso provedor de e-mail, para mensagens de conta como redefinição de senha.",
          "Canais de notificação que você mesmo conecta, como um bot de mensagens, recebem apenas o que suas automações enviam.",
          "Provedores de IA, quando você usa recursos de assistente ou visão que os acionam. Não vendemos seus dados e não usamos suas imagens para treinar modelos.",
        ],
      },
      {
        heading: "5. Por quanto tempo guardamos",
        body: [
          "As gravações seguem o prazo de retenção do seu plano e expiram automaticamente pelas regras de ciclo de vida do provedor de armazenamento. Cada clipe mantém a retenção sob a qual foi gravado.",
          "Os eventos são varridos periodicamente conforme a configuração de retenção de eventos do seu plano.",
          "Registros de conta e cobrança são mantidos enquanto a conta existir e, depois, apenas quando houver obrigação contábil ou legal.",
        ],
      },
      {
        heading: "6. Seus direitos",
        body: [
          "Você pode acessar, corrigir, exportar ou excluir seus dados. A exclusão de um espaço de trabalho remove seus objetos armazenados em todos os níveis de retenção.",
          "Sob a LGPD e o GDPR você também pode se opor ao tratamento, revogar consentimento e reclamar à autoridade competente.",
          `Para exercer qualquer desses direitos, escreva para ${OPERATOR.privacyContact}.`,
        ],
      },
      {
        heading: "7. Segurança",
        body: [
          "O tráfego é criptografado em trânsito. O acesso é limitado por espaço de trabalho e por papel, os appliances se autenticam com chaves de assinatura por dispositivo e as sessões podem ser revogadas centralmente.",
          "Nenhum sistema é perfeitamente seguro. Se um incidente afetar seus dados pessoais, notificaremos você e a autoridade competente conforme a lei exigir.",
        ],
      },
      {
        heading: "8. Transferências internacionais",
        body: [
          "Nossos provedores podem tratar dados fora do seu país. Quando isso ocorre, as transferências apoiam-se nas salvaguardas oferecidas por esses provedores conforme a lei aplicável.",
        ],
      },
      {
        heading: "9. Alterações e contato",
        body: [
          "Publicaremos qualquer atualização aqui e registraremos na sua conta a versão que você aceitou.",
          `${OPERATOR.entity}, ${OPERATOR.address}. Contato de privacidade: ${OPERATOR.privacyContact}.`,
        ],
      },
    ],
  },
};
