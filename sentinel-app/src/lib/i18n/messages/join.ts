// The page an invite link opens (#181). Written for someone with no account who may never have
// seen this product — so it names the organisation first and asks for as little as it can.
export const join = {
  en: {
    checking: "Checking your invitation…",
    joinTitle: (tenant: string) => `Join ${tenant}`,
    joinBody: "Choose a password and you are in.",
    deadTitle: "This invitation is no longer valid",
    // Deliberately one message for missing / expired / already-used: the endpoint is public and
    // enumerable, so distinguishing them would say which tokens exist.
    deadBody: "It may have expired or already been used. Ask whoever invited you for a new one.",
    email: "Email",
    emailPlaceholder: "you@company.com",
    password: "Password",
    passwordPlaceholder: "At least 8 characters",
    confirm: "Confirm password",
    join: "Join",
    welcome: (tenant: string) => `Welcome to ${tenant}`,
    failed: "Couldn't accept the invitation",
    toLogin: "Go to sign in",
  },
  pt: {
    checking: "Verificando seu convite…",
    joinTitle: (tenant: string) => `Entrar em ${tenant}`,
    joinBody: "Escolha uma senha e pronto.",
    deadTitle: "Este convite não vale mais",
    deadBody: "Ele pode ter expirado ou já ter sido usado. Peça um novo a quem convidou você.",
    email: "E-mail",
    emailPlaceholder: "voce@empresa.com",
    password: "Senha",
    passwordPlaceholder: "No mínimo 8 caracteres",
    confirm: "Confirme a senha",
    join: "Entrar",
    welcome: (tenant: string) => `Bem-vindo a ${tenant}`,
    failed: "Não foi possível aceitar o convite",
    toLogin: "Ir para o login",
  },
};
