// The page SvelteKit renders for a bad URL or a failed load (+error.svelte).
//
// Written for someone who did not cause this. The 404 and the fault say different things on
// purpose: "page not found" for a server error sends a person looking for their own mistake.
export const errorPage = {
  en: {
    notFoundTitle: "That page doesn't exist",
    notFoundBody: "The link may be old, or mistyped. Everything else is working.",
    faultTitle: "Something went wrong on our side",
    faultBody: "This isn't something you did. Try again — if it keeps happening, tell us.",
    goHome: "Go to the dashboard",
    retry: "Try again",
  },
  pt: {
    notFoundTitle: "Essa página não existe",
    notFoundBody: "O link pode estar velho ou com erro de digitação. O resto está funcionando.",
    faultTitle: "Algo deu errado do nosso lado",
    faultBody: "Não foi nada que você fez. Tente de novo — se persistir, nos avise.",
    goHome: "Ir para o painel",
    retry: "Tentar de novo",
  },
};
