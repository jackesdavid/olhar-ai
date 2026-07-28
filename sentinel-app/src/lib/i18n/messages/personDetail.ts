// People → person record (/people/[id]): details, hierarchy, recipient, permissions over
// subordinates and the biometrics summary. Permission slugs come from the backend catalog.
export const personDetail = {
  en: {
    backToPeople: "Back to People",
    newPerson: "New person",
    person: "Person",
    newSubtitle: "Create a record — face enrollment comes later.",
    editSubtitle: "Edit this person's record.",
    delete: "Delete",
    saving: "Saving…",
    loading: "Loading…",
    goneTitle: "That person no longer exists.",
    backButton: "Back to People",
    // details
    details: "Details",
    name: "Name",
    namePlaceholder: "Full name",
    // hierarchy
    hierarchy: "Hierarchy",
    hierarchyDescription:
      "The guardian/superior above this person, and who sits beneath them.",
    guardian: "Guardian (superior)",
    none: "None",
    subordinates: "Subordinates",
    noSubordinates: "No one reports to this person yet.",
    // recipient
    recipient: "Recipient",
    recipientDescription: "This person's own notification target (their device).",
    noRecipients: "No recipients paired yet — add one under Recipients.",
    // permissions
    permissions: "Permissions over subordinates",
    permissionsDescription: "What this person may do for the people beneath them.",
    // biometrics
    biometrics: "Biometrics",
    biometricsDescription:
      "Face enrollment for recognition at entrances/exits. We store only the embedding, never the photo.",
    saveFirst: "Save this person first to enroll.",
    facesEnrolled: (approved: number, total: number) =>
      `${approved} face${approved === 1 ? "" : "s"} enrolled (${total} total)`,
    noFaces: "No faces enrolled yet",
    openEnrollment: "Open enrollment",
    // delete dialog
    removeTitle: "Remove person?",
    removeBefore: "Remove ",
    removeAfter:
      ". Anyone whose guardian is this person will be detached. This can't be undone.",
    removing: "Removing…",
    remove: "Remove",
    // toasts
    loadFailed: "Failed to load person",
    nameEmpty: "Name can't be empty.",
    added: "Person added.",
    updated: "Person updated.",
    saveFailed: "Save failed",
    removed: "Person removed.",
    deleteFailed: "Delete failed",
  },
  pt: {
    backToPeople: "Voltar para Pessoas",
    newPerson: "Nova pessoa",
    person: "Pessoa",
    newSubtitle: "Crie o registro — o cadastro de rosto vem depois.",
    editSubtitle: "Edite o registro desta pessoa.",
    delete: "Excluir",
    saving: "Salvando…",
    loading: "Carregando…",
    goneTitle: "Essa pessoa não existe mais.",
    backButton: "Voltar para Pessoas",
    // details
    details: "Dados",
    name: "Nome",
    namePlaceholder: "Nome completo",
    // hierarchy
    hierarchy: "Hierarquia",
    hierarchyDescription:
      "O responsável/superior acima desta pessoa, e quem está abaixo dela.",
    guardian: "Responsável (superior)",
    none: "Nenhum",
    subordinates: "Subordinados",
    noSubordinates: "Ninguém se reporta a esta pessoa ainda.",
    // recipient
    recipient: "Destinatário",
    recipientDescription: "O destino de notificações desta pessoa (o dispositivo dela).",
    noRecipients: "Nenhum destinatário pareado ainda — adicione um em Destinatários.",
    // permissions
    permissions: "Permissões sobre subordinados",
    permissionsDescription: "O que esta pessoa pode fazer pelas pessoas abaixo dela.",
    // biometrics
    biometrics: "Biometria",
    biometricsDescription:
      "Cadastro de rosto para reconhecimento em entradas/saídas. Guardamos apenas o embedding, nunca a foto.",
    saveFirst: "Salve esta pessoa antes de cadastrar o rosto.",
    facesEnrolled: (approved: number, total: number) =>
      `${approved} rosto${approved === 1 ? "" : "s"} cadastrado${approved === 1 ? "" : "s"} (${total} no total)`,
    noFaces: "Nenhum rosto cadastrado ainda",
    openEnrollment: "Abrir cadastro",
    // delete dialog
    removeTitle: "Remover pessoa?",
    removeBefore: "Remover ",
    removeAfter:
      ". Quem tem esta pessoa como responsável ficará sem vínculo. Isso não pode ser desfeito.",
    removing: "Removendo…",
    remove: "Remover",
    // toasts
    loadFailed: "Falha ao carregar a pessoa",
    nameEmpty: "O nome não pode ficar vazio.",
    added: "Pessoa adicionada.",
    updated: "Pessoa atualizada.",
    saveFailed: "Falha ao salvar",
    removed: "Pessoa removida.",
    deleteFailed: "Falha ao excluir",
  },
};
