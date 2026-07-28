// Settings → Users (/settings/users): the members matrix and the invite form. Role *names* are
// tenant data (user-created), so they stay as stored.
export const users = {
  en: {
    title: "Users",
    description: "Invite people into this workspace and assign their roles.",
    noPermission: "You don't have permission to manage users.",
    members: "Members",
    email: "Email",
    master: "master",
    emptyTitle: "No users yet",
    emptyDescription:
      "Invite teammates below — they sign in with their email and the roles you assign.",
    inviteTitle: "Invite a user",
    inviteDescription: "They sign in with this email and password.",
    emailPlaceholder: "teammate@example.com",
    tempPassword: "Temporary password",
    tempPasswordPlaceholder: "At least 8 characters",
    roles: "Roles",
    invite: "Invite",
    // toasts
    loadFailed: "Failed to load users",
    rolesFailed: "Failed to update roles",
    invited: "User invited",
    inviteFailed: "Failed to invite user",
  },
  pt: {
    title: "Usuários",
    description: "Convide pessoas para este workspace e atribua seus perfis.",
    noPermission: "Você não tem permissão para gerenciar usuários.",
    members: "Membros",
    email: "E-mail",
    master: "master",
    emptyTitle: "Nenhum usuário ainda",
    emptyDescription:
      "Convide colegas abaixo — eles entram com o e-mail e os perfis que você atribuir.",
    inviteTitle: "Convidar um usuário",
    inviteDescription: "A pessoa entra com este e-mail e senha.",
    emailPlaceholder: "colega@exemplo.com",
    tempPassword: "Senha temporária",
    tempPasswordPlaceholder: "Pelo menos 8 caracteres",
    roles: "Perfis",
    invite: "Convidar",
    // toasts
    loadFailed: "Falha ao carregar os usuários",
    rolesFailed: "Falha ao atualizar os perfis",
    invited: "Usuário convidado",
    inviteFailed: "Falha ao convidar o usuário",
  },
};
