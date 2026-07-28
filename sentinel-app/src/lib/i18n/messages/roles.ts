// Settings → Roles (/settings/roles): the permission matrix, the create-role dialog and the
// self-lockout guard. Permission keys and their descriptions come from the backend catalog
// (`rbac.permissions()`), so they render as the server sends them.
export const roles = {
  en: {
    title: "Roles & permissions",
    description:
      "Access is granted by action, never by role name. Changes save as you toggle.",
    newRole: "New role",
    noPermissionTitle: "You can't manage roles",
    noPermissionDescription:
      "Ask an administrator for the roles.manage action to edit the permission matrix.",
    emptyTitle: "No roles yet",
    emptyDescription: "Create a role, then grant it the actions it needs.",
    action: "Action",
    system: "system",
    footnoteBefore: "A filled dot grants the action to that role. System roles come with sensible defaults — ",
    footnoteAfter: " always receives every action, including new ones.",
    admin: "Admin",
    // create dialog
    createDescription: "Create a custom role, then grant it actions in the matrix.",
    roleName: "Role name",
    roleNamePlaceholder: "e.g. Operator",
    createRole: "Create role",
    // lockout guard
    lockoutTitle: (key: string) => `Remove “${key}”?`,
    lockoutBefore: "If you hold the ",
    lockoutAfter:
      " role, you will lose access to this screen and won't be able to grant it back yourself.",
    removeAnyway: "Remove anyway",
    // toasts
    loadFailed: "Failed to load roles",
    saveFailed: "Failed to save",
    created: (name: string) => `Role "${name}" created`,
    createFailed: "Failed to create role",
  },
  pt: {
    title: "Perfis e permissões",
    description:
      "O acesso é concedido por ação, nunca pelo nome do perfil. As mudanças salvam ao alternar.",
    newRole: "Novo perfil",
    noPermissionTitle: "Você não pode gerenciar perfis",
    noPermissionDescription:
      "Peça a um administrador a ação roles.manage para editar a matriz de permissões.",
    emptyTitle: "Nenhum perfil ainda",
    emptyDescription: "Crie um perfil e depois conceda as ações de que ele precisa.",
    action: "Ação",
    system: "sistema",
    footnoteBefore:
      "Um ponto preenchido concede a ação àquele perfil. Perfis de sistema já vêm com padrões sensatos — ",
    footnoteAfter: " sempre recebe todas as ações, inclusive as novas.",
    admin: "Admin",
    // create dialog
    createDescription: "Crie um perfil personalizado e depois conceda as ações na matriz.",
    roleName: "Nome do perfil",
    roleNamePlaceholder: "ex.: Operador",
    createRole: "Criar perfil",
    // lockout guard
    lockoutTitle: (key: string) => `Remover “${key}”?`,
    lockoutBefore: "Se você tem o perfil ",
    lockoutAfter:
      ", perderá o acesso a esta tela e não conseguirá conceder a permissão de volta sozinho.",
    removeAnyway: "Remover mesmo assim",
    // toasts
    loadFailed: "Falha ao carregar os perfis",
    saveFailed: "Falha ao salvar",
    created: (name: string) => `Perfil "${name}" criado`,
    createFailed: "Falha ao criar o perfil",
  },
};
