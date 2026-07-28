// People → enrollment (/people/[id]/enroll): capture from a camera or upload, and the grid of
// enrolled samples rendered as point-cloud fingerprints.
export const enroll = {
  en: {
    backToRecord: "Back to record",
    titleFor: (name: string) => `Enroll — ${name}`,
    person: "Person",
    subtitle:
      "Capture several angles for stronger recognition. We store only the face embedding — never the photo.",
    loading: "Loading…",
    // capture
    capture: "Capture",
    captureDescription: "Grab a frame from a saved camera, or upload a photo as a fallback.",
    noCameras: "No cameras — add one in Devices, or upload a photo below.",
    camera: "Camera",
    selectCamera: "Select a camera",
    working: "Working…",
    captureFromCamera: "Capture from camera",
    approveOnCapture: "Approve on capture (consent given)",
    uploadPhoto: "Upload photo",
    // samples
    enrolledFaces: "Enrolled faces",
    countBadge: (approved: number, total: number) => `${approved} approved / ${total} total`,
    samplesDescription:
      "Each sample is shown as a point-cloud fingerprint derived from its embedding — no photo is kept.",
    emptyTitle: "No faces enrolled",
    emptyDescription: "Capture from a camera or upload a photo above.",
    approved: "Approved",
    pending: "Pending",
    approve: "Approve",
    // toasts
    loadFailed: "Failed to load",
    enrolled: "Face enrolled.",
    enrollFailed: "Enroll failed",
    pickCamera: "Pick a camera first.",
    captureFailed: "Capture failed",
    approveFailed: "Approve failed",
    removeFailed: "Remove failed",
  },
  pt: {
    backToRecord: "Voltar ao registro",
    titleFor: (name: string) => `Cadastrar — ${name}`,
    person: "Pessoa",
    subtitle:
      "Capture vários ângulos para um reconhecimento mais forte. Guardamos apenas o embedding do rosto — nunca a foto.",
    loading: "Carregando…",
    // capture
    capture: "Captura",
    captureDescription: "Pegue um quadro de uma câmera salva, ou envie uma foto como alternativa.",
    noCameras: "Nenhuma câmera — adicione uma em Dispositivos, ou envie uma foto abaixo.",
    camera: "Câmera",
    selectCamera: "Selecione uma câmera",
    working: "Processando…",
    captureFromCamera: "Capturar pela câmera",
    approveOnCapture: "Aprovar na captura (consentimento dado)",
    uploadPhoto: "Enviar foto",
    // samples
    enrolledFaces: "Rostos cadastrados",
    countBadge: (approved: number, total: number) =>
      `${approved} aprovado${approved === 1 ? "" : "s"} / ${total} no total`,
    samplesDescription:
      "Cada amostra aparece como uma impressão em nuvem de pontos derivada do embedding — nenhuma foto é guardada.",
    emptyTitle: "Nenhum rosto cadastrado",
    emptyDescription: "Capture por uma câmera ou envie uma foto acima.",
    approved: "Aprovado",
    pending: "Pendente",
    approve: "Aprovar",
    // toasts
    loadFailed: "Falha ao carregar",
    enrolled: "Rosto cadastrado.",
    enrollFailed: "Falha no cadastro",
    pickCamera: "Escolha uma câmera primeiro.",
    captureFailed: "Falha na captura",
    approveFailed: "Falha ao aprovar",
    removeFailed: "Falha ao remover",
  },
};
