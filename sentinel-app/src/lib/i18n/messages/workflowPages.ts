// The workflow list views that sit around the editor: Templates (/workflows/templates),
// Instances (/workflows/instances) and Dictionary Tables (/workflows/dictionaries).
// Template/graph/dictionary names and node labels are tenant data.
export const workflowPages = {
  en: {
    workflows: "Workflows",
    backToWorkflows: "Back to workflows",
    // templates
    templatesTitle: "Templates",
    templatesDescription:
      "Ready-made workflows to start from — cloned into your workspace to fill in and turn on.",
    loadingTemplates: "Loading templates…",
    templatesEmptyTitle: "No templates yet",
    templatesEmptyDescription:
      "Save a workflow as a template with the bookmark on its row, and it'll appear here to reuse.",
    global: "Global",
    asksFor: (inputs: string) => `Asks for ${inputs}`,
    creating: "Creating…",
    useTemplate: "Use template",
    templatesLoadFailed: "Couldn't load templates",
    templateCreated: (name: string) => `Created “${name}” — fill in its inputs and turn it on.`,
    templateUseFailed: "Couldn't use template",
    // instances
    instances: "Instances",
    instancesNamed: (graph: string) => `Instances — ${graph}`,
    instancesDescription:
      "Each run context that has saved data, and the last value every node produced in it.",
    refresh: "Refresh",
    noGraphTitle: "No workflow selected",
    noGraphDescription: "Open a workflow's instances from its row on the Workflows page.",
    loadingInstances: "Loading instances…",
    instancesEmptyTitle: "No saved run contexts yet",
    instancesEmptyDescription:
      "Run this workflow, then its instances and their saved node values appear here.",
    defaultInstance: "Default (single)",
    pickInstance: "Pick an instance to see its saved values.",
    loadingValues: "Loading values…",
    noValues: "No node values saved in this instance.",
    nodeN: (id: number | string) => `Node ${id}`,
    noOutputs: "— no outputs —",
    instancesLoadFailed: "Couldn't load instances",
    valuesLoadFailed: "Couldn't load instance values",
    // dictionaries
    editDictionary: "Edit dictionary table",
    newDictionary: "New dictionary table",
    dictionaryEditorDescription:
      "Define the columns (the structure), then add rows of typed values.",
    saving: "Saving…",
    saveDictionary: "Save dictionary table",
    name: "Name",
    namePlaceholder: "e.g. Store hours",
    dictionariesTitle: "Dictionary Tables",
    dictionariesDescription:
      "Reusable typed tables — reference one from a workflow variable; edit it once and every workflow sees it.",
    newDictionaryButton: "New dictionary table",
    loadingDictionaries: "Loading dictionary tables…",
    dictionariesEmptyTitle: "No dictionary tables yet",
    dictionariesEmptyDescription:
      "Create a typed table of values you can reference from workflow variables and mutate through graphs.",
    deleteDictionary: "Delete dictionary table",
    colsCount: (n: number) => `${n} cols`,
    rowsCount: (n: number) => `${n} rows`,
    dictionariesLoadFailed: "Couldn't load dictionary tables",
    dictionarySaved: (name: string) => `Saved “${name}”`,
    dictionarySaveFailed: "Couldn't save",
    dictionaryDeleted: (name: string) => `Deleted “${name}”`,
    dictionaryDeleteFailed: "Couldn't delete",
  },
  pt: {
    workflows: "Fluxos",
    backToWorkflows: "Voltar para os fluxos",
    // templates
    templatesTitle: "Modelos",
    templatesDescription:
      "Fluxos prontos para começar — clonados no seu workspace para preencher e ligar.",
    loadingTemplates: "Carregando os modelos…",
    templatesEmptyTitle: "Nenhum modelo ainda",
    templatesEmptyDescription:
      "Salve um fluxo como modelo pelo marcador na linha dele, e ele aparecerá aqui para reutilizar.",
    global: "Global",
    asksFor: (inputs: string) => `Pede ${inputs}`,
    creating: "Criando…",
    useTemplate: "Usar modelo",
    templatesLoadFailed: "Não foi possível carregar os modelos",
    templateCreated: (name: string) => `“${name}” criado — preencha as entradas e ligue-o.`,
    templateUseFailed: "Não foi possível usar o modelo",
    // instances
    instances: "Instâncias",
    instancesNamed: (graph: string) => `Instâncias — ${graph}`,
    instancesDescription:
      "Cada contexto de execução com dados salvos, e o último valor que cada nó produziu nele.",
    refresh: "Atualizar",
    noGraphTitle: "Nenhum fluxo selecionado",
    noGraphDescription: "Abra as instâncias de um fluxo pela linha dele na página Fluxos.",
    loadingInstances: "Carregando as instâncias…",
    instancesEmptyTitle: "Nenhum contexto de execução salvo ainda",
    instancesEmptyDescription:
      "Execute este fluxo e então as instâncias e os valores salvos dos nós aparecem aqui.",
    defaultInstance: "Padrão (única)",
    pickInstance: "Escolha uma instância para ver os valores salvos dela.",
    loadingValues: "Carregando os valores…",
    noValues: "Nenhum valor de nó salvo nesta instância.",
    nodeN: (id: number | string) => `Nó ${id}`,
    noOutputs: "— sem saídas —",
    instancesLoadFailed: "Não foi possível carregar as instâncias",
    valuesLoadFailed: "Não foi possível carregar os valores da instância",
    // dictionaries
    editDictionary: "Editar tabela de dicionário",
    newDictionary: "Nova tabela de dicionário",
    dictionaryEditorDescription:
      "Defina as colunas (a estrutura) e depois adicione linhas de valores tipados.",
    saving: "Salvando…",
    saveDictionary: "Salvar tabela de dicionário",
    name: "Nome",
    namePlaceholder: "ex.: Horário de funcionamento",
    dictionariesTitle: "Tabelas de dicionário",
    dictionariesDescription:
      "Tabelas tipadas reutilizáveis — referencie uma a partir de uma variável de fluxo; edite uma vez e todos os fluxos veem.",
    newDictionaryButton: "Nova tabela de dicionário",
    loadingDictionaries: "Carregando as tabelas de dicionário…",
    dictionariesEmptyTitle: "Nenhuma tabela de dicionário ainda",
    dictionariesEmptyDescription:
      "Crie uma tabela tipada de valores que você pode referenciar em variáveis de fluxo e alterar pelos grafos.",
    deleteDictionary: "Excluir tabela de dicionário",
    colsCount: (n: number) => `${n} colunas`,
    rowsCount: (n: number) => `${n} linhas`,
    dictionariesLoadFailed: "Não foi possível carregar as tabelas de dicionário",
    dictionarySaved: (name: string) => `“${name}” salvo`,
    dictionarySaveFailed: "Não foi possível salvar",
    dictionaryDeleted: (name: string) => `“${name}” excluído`,
    dictionaryDeleteFailed: "Não foi possível excluir",
  },
};
