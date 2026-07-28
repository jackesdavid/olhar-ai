// The People hub (/people) — the two entry tiles and their live counts.
export const people = {
  en: {
    title: "People",
    description: "Manage the people Sentinel knows, or test who it recognises.",
    directoryTitle: "Directory",
    directoryDesc:
      "Browse, add and edit people, the hierarchy, recipients and permissions.",
    recognitionTitle: "Recognition",
    recognitionDesc:
      "Probe a camera frame or photo against the enrolled people — target vs. match.",
    peopleCount: (n: number) => `${n} ${n === 1 ? "person" : "people"}`,
    enrolledCount: (n: number) => `${n} enrolled`,
  },
  pt: {
    title: "Pessoas",
    description: "Gerencie as pessoas que o Sentinel conhece, ou teste quem ele reconhece.",
    directoryTitle: "Diretório",
    directoryDesc:
      "Navegue, adicione e edite pessoas, a hierarquia, destinatários e permissões.",
    recognitionTitle: "Reconhecimento",
    recognitionDesc:
      "Compare um quadro de câmera ou foto com as pessoas cadastradas — alvo vs. correspondência.",
    peopleCount: (n: number) => `${n} pessoa${n === 1 ? "" : "s"}`,
    enrolledCount: (n: number) => `${n} cadastrada${n === 1 ? "" : "s"}`,
  },
};
