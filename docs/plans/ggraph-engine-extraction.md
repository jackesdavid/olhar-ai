# ggraph — a engine de grafos num repo próprio, compartilhada com o Redoma

**Board card:** #204 ggraph — extrair a engine de grafos para repo próprio
**Repo da engine:** `git@github.com:jackesdavid/graph-engine.git`
**Goal:** a engine sai do `app-core` para um crate próprio, agnóstica de domínio, sem o Sentinel
mudar de comportamento — e o Redoma começa em cima dela sem esperar a refatoração terminar.

## Context

O Redoma (plataforma de IA local/offline, plano em `~/Downloads/redoma-plano.md`) e o
OlharAI/Sentinel vão compartilhar **exatamente uma coisa**: a engine de execução de grafos.
Nenhum outro código é comum.

Hoje a engine mora em `crates/app-core/src/workflows/` — 13.802 linhas, acoplada ao domínio
de câmeras por *nome* (não por dependência) e ao host por um único parâmetro (`ToolContext`).

**Restrições que mandam neste plano:**

1. O Sentinel continua funcionando **sem alteração de comportamento** — é produção.
2. O Redoma precisa começar **rápido**, não esperar a refatoração terminar.
3. As duas evoluem em paralelo depois, sem uma arrastar a outra.

---

## 1. A regra da divisão

> **O core é tudo que a engine precisa para funcionar sem um único nó de domínio.**
> Os nós padrão — branch, log, time gate — são parte do núcleo, não um extra.
> **Um nó por arquivo**, sempre.

Isso é mais gordo que o corte de sete crates do `redoma-plano.md` §16.3, e substitui ele.
Duas razões concretas, não estéticas:

- `Graph::add_edge` valida um wire **perguntando as portas ao nó**
  (`domain.rs:2751` → `GraphNode::output_port` → `exec_out_ports`). Separar `graph-core` de
  `graph-node` cria um ciclo ou uma validação cega. São o mesmo crate.
- `graph-std` isolado é, pelo próprio plano, "o crate com maior tentação de ganhar um nó
  específico de um dos produtos". Dentro do core, com o teste de vocabulário do §16.4
  rodando sobre ele, essa tentação falha o build.

O que mantém o core sem I/O apesar dos nós com estado (`time_gate`, `delay_trigger`,
`cooldown`, `flip_flop`, `device_gate`) é o seam de host: o nó chama `host.state()` e
`host.schedule()`, que são traits. Postgres, filas e relógio ficam fora. Sem tokio, sem sqlx.

## 2. A descoberta que torna isso barato

`NodeKind` e `PortType` são enums fechados (91 e 23 variantes) — é o que impede a engine de
servir dois domínios. Mas ambos já serializam como string snake_case
(`#[serde(rename_all = "snake_case")]`, `domain.rs:23` e `:393`) e o `slug()` manual
(`:1343`) devolve texto **byte-idêntico** nos 91 kinds — conferido arm a arm.

Trocar `enum NodeKind` por `NodeId(SmolStr)` resolvido em registry, e `enum PortType` por um
newtype aberto com consts para os builtin, é **wire-compatible**: o JSON dos grafos já
gravados no Postgres, o endpoint `GET /workflows/catalog` e o editor Svelte continuam
funcionando sem migração.

Sem isso a alternativa seria genéricos (`Graph<D: Domain>`) contaminando toda a engine —
`List(Vec<PortValue<X>>)` torna o parâmetro recursivo e ele infecta `PortValues`,
`NodeSpec`, o registry, o codec e os ~40 sítios do scheduler. Big-bang, exatamente o que
este plano existe para evitar.

**Duas exceções, ambas armadilha silenciosa:**

- `NotifyRecipient` tem `#[serde(alias = "notify_beacon")]` (`domain.rs:433`) e `from_slug`
  **não** conhece o alias. Hoje há dois caminhos de decode que discordam. `NodeSpec` precisa
  de `aliases: &'static [&'static str]`, senão todo grafo gravado com `notify_beacon` para
  de carregar.
- `NodeKind::ALL` tem 90 dos 91 — `Reroute` está fora de propósito (`domain.rs:1246`).
  Semear o registry a partir de `ALL` produz um registry que não resolve um `reroute` já
  gravado. `NodeSpec` precisa de `hidden: bool`: registrado (91), fora da paleta (90).

## 3. O seam do host é minúsculo

`ToolContext` tem 14 campos; o scheduler usa seis: `tenant_id` 24×, `db` 24×, `variables` 5×,
`active_gates` 3×, `ui_tx` 2×, `run_id` 1×. E encolhe mais quando se olha para que servem:

- os 24 `tenant_id` são todos argumento de `repo::*(&db, tenant_id, …)` — somem quando o
  `StateStore` é construído já com o tenant dentro. O scheduler passa a **não conseguir**
  endereçar outro tenant, nem por engano;
- os três `active_gates` estão todos dentro do bloco do `TimeGate`, que vira um nó.

Sobram quatro capacidades e um run id.

---

## 4. Layout

### `ggraph-core` (repo novo — sem tokio, sem sqlx, sem I/O)

```
ggraph-core/src/
  graph/     Graph<M>, GraphNode, Edge, Viewport, add_edge, topo_order, back_edges, reaches
  port/      PortType (newtype aberto + consts), Port, PortName, compatible
  value/     Value, Num, Bytes, Extern(Arc<dyn ExternValue>), codec tagueado
  spec/      NodeSpec, Ports, ExecOut, Purity, Timeout, Behavior
  registry/  NodeRegistry — resolve slug + alias → spec; decoders de Extern
  host/      Host, StateStore, Observer, ValueIo, Approvals, Llm, Http, TableStore
  exec/      o scheduler (multi-época) + política de checkpoint
  nodes/     UM ARQUIVO POR NÓ — spec + impl + testes juntos
```

`nodes/` recebe ~55 nós:

| Grupo | Nós |
|---|---|
| Controle | `if` `for_each` `wait` `string_switch` `flip_flop` `cooldown` `time_gate` `delay_trigger` |
| Aprovação | `device_gate` (generalizado — §5.4) |
| Eventos | `emit_event` `on_event` |
| Lógica | `compare` `num_lt` `num_lte` `num_gt` `num_gte` `num_eq` `num_ne` `and` `or` `not` `text_compare` |
| Conversão/texto | `num_to_text` `bool_to_text` `format` |
| Variáveis | `get_variable` `set_variable` `multi_get_variable` `multi_set_variable` |
| Log/canvas | `print` `comment` `reroute` `title` `paragraph` `markdown` `text` `gauge` `show_image` |
| Tabelas | `dict_schema` `dict_make_row` `dict_append_row` + os 9 de CRUD |
| Rede | `http_post` |
| LLM | `ask_ai` `ai_switch` `ai_extract` `ai_answer` |

`draw_text` e `get_image_size` são agnósticos mas arrastam `image`/`imageproc`: entram como
`nodes/image/` atrás de `feature = "image"`, desligada por padrão. O Redoma não paga.

### O que fica no bishop

`crates/app-core/src/workflows/` guarda só o Sentinel — também **um nó por arquivo**, o que
hoje não é o caso (`node/vision.rs` tem 15 nós em 1.049 linhas):

```
workflows/
  mod.rs          fachada: re-exporta ggraph com a arity de hoje (§6)
  host.rs         SentinelHost: impl Host sobre ToolContext
  repo.rs         impl StateStore/Observer em Postgres (fica; SQL é do Sentinel)
  sentinel.rs     InstanceScope, on_event_node, resolve_camera_selector, resolve_region
  catalog.rs      catalog_json() — sai do sentinel-control (§6)
  nodes/          ~35 nós de domínio, um por arquivo
```

### Depois (não é Fase 0)

`ggraph-pg` — implementação Postgres de `StateStore` + fila (claim/lease/retry/prioridade/
justiça entre tenants) + histórico de run, com migrações próprias. Genuinamente compartilhado,
mas o `repo.rs` de hoje tem 1.600 linhas de SQL com formato do Sentinel e as 12 migrações da
engine vivem numa sequência numerada única (`sqlx::migrate!`) que não se parte sem renumerar.
Vale fazer com o Redoma pedindo, não antes.

---

## 5. As APIs que decidem tudo

### 5.1 `Graph<M>` — a policy do produto sai do struct compartilhado

`Graph` carrega hoje **doze** campos que são policy de produto, não topologia: `trigger`,
`instance_scope`, `concurrency`, `variables`, `run_orphans`, `enabled`, `description`,
`group`, `template`, `global`, `debug`, `edge_vias` — e `InstanceScope::PerCamera` está
**literalmente no documento gravado**. Não dá para mover
`Graph` sem levar câmeras junto, nem para remover os campos sem migração.

```rust
pub struct Graph<M = ()> {
    pub id: Uuid, pub name: String,
    pub nodes: Vec<GraphNode>, pub edges: Vec<Edge>, pub next_id: u32,
    #[serde(default)] pub viewport: Viewport,
    #[serde(default)] pub edge_vias: HashMap<String, Vec<[i32; 2]>>,   // geometria: é do canvas
    #[serde(flatten)] pub meta: M,          // ← SentinelMeta | RedomaMeta
}
```

`#[serde(flatten)]` recoloca as chaves no mesmo nível: JSON byte-idêntico, tipagem em tempo
de compilação. É a decisão de API mais importante do plano.

Consequência aceita: `add_edge` passa a receber o registry (`add_edge(&reg, from, …)`),
porque validar wire é perguntar portas ao nó. O bishop embrulha com o registry de processo e
os sítios de chamada mantêm a arity.

### 5.2 `Value` — `Extern(Arc<dyn ExternValue>)`

```rust
pub enum Value {
    Text(String), Num(Num), Bool(bool),
    Bytes(Bytes),                  // Image + File colapsam aqui
    Json(serde_json::Value),       // a lacuna de objeto genérico do Redoma
    List(Vec<Value>),
    Map(Vec<(String, Value)>),     // Dict alargado: valores são Value, não String
    Extern(Arc<dyn ExternValue>),
}
pub enum Num { Int(i64), Float(f64) }   // hoje só existe i64

pub trait ExternValue: Debug + Send + Sync + 'static {
    fn type_name(&self) -> &'static str;          // == a string do PortType
    fn as_any(&self) -> &dyn Any;
    fn to_json(&self, io: &dyn ValueIo) -> Option<Value> { None }  // None = descartado
    fn summary(&self) -> String { self.type_name().into() }
    fn as_text(&self) -> Option<&str> { None }
}
```

Os 10 tipos do Sentinel (`Camera`, `Bbox`, `Face`, `Person`, `Region`, `Embedding`,
`Landmarks`, `Recipient`, `Table`, `TrackedObject`) viram `ExternValue` **carregando a tag que
já usam** — a saída de `values.rs::value_to_json` fica byte-a-byte a de hoje.

Não `Arc<dyn Any>`: perde o nome do tipo, que é exatamente o que o codec tagueado e o
`debug_summary` precisam.

**Disciplina obrigatória:** `Num::Float` e `Map<String, Value>` entram como *tipos* numa fase,
e `json_to_port_value` (`engine.rs:51`, hoje degrada float → `Text`) muda em **outra**. Virar
os dois juntos faz `as_text()` passar a devolver `None` em grafo de cliente — mudança de
comportamento silenciosa vestida de refactor.

### 5.3 `NodeSpec` — 14 sítios de despacho viram 1

```rust
pub struct NodeSpec<H: Host> {
    pub id: NodeId,
    pub aliases: &'static [&'static str],     // notify_beacon
    pub label: &'static str,
    pub category: &'static str,
    pub hidden: bool,                          // reroute
    pub inputs: Ports, pub outputs: Ports,     // Static | Dynamic(fn(&cfg) -> Vec<Port>)
    pub exec_in: bool, pub exec_out: ExecOut,  // None | Static | Dynamic
    pub default_config: fn() -> Value,
    pub purity: Purity, pub timeout: Timeout,
    pub behavior: Behavior<H>,
}

pub enum Behavior<H: Host> {
    Inert,                         // comment, reroute, title, paragraph, markdown
    Run(Arc<dyn NodeRun<H>>),      // ~70 nós
    Route(Arc<dyn NodeRoute<H>>),  // if, ask_ai, string_switch, ai_switch, camera_switch
    Step(Arc<dyn NodeStep<H>>),    // os 11 que hoje vivem inline no forward_epoch
}
```

Some: `enum NodeKind`, `ALL`, `slug`, `from_slug`, `label`, `category`, `is_pure`,
`is_pure_source`, `input_ports`, `output_ports`, `exec_out_ports`, `default_config`,
`node_timeout`, `run_node`, `debug_summary`, mais os três overrides em `GraphNode`.

`Ports::Dynamic` conserta um bug de desenho de hoje: `catalog()` chama `NodeKind::input_ports`
(estático) enquanto a engine chama `GraphNode::input_ports` (derivado da config) — **duas
respostas diferentes para o mesmo nó**, para os 8 kinds com portas dinâmicas.

`Step` cobre os 11 blocos `if n.kind == X` de dentro do `forward_epoch`. Conferido bloco a
bloco: todos terminam nos mesmos quatro movimentos —

```rust
pub struct Step {
    pub outputs: PortValues,
    pub arms: Vec<PortName>,   // live_out.insert(...)
    pub reenter: bool,         // só ForEach
    pub log: Option<String>,
    pub halt: bool,            // DeviceGate: o run termina aqui
}
```

### 5.4 `Host` — quatro capacidades e um run id

```rust
pub trait Host: Send + Sync + Clone + 'static {
    type Meta: GraphMeta;
    fn state(&self) -> &dyn StateStore;
    fn vars(&self) -> &Mutex<HashMap<String, Value>>;
    fn io(&self) -> &dyn ValueIo;
    fn observer(&self) -> &dyn Observer;
    fn run_id(&self) -> Uuid;
    fn instance_key(&self, meta: &Self::Meta, payload: &PortValues) -> SmolStr;
    fn schedule(&self, at_epoch_secs: i64, t: NodeTarget) -> Result<(), HostError>;
    // capacidades dos nós de core que tocam o mundo:
    fn approvals(&self) -> &dyn Approvals;   // device_gate
    fn tables(&self) -> &dyn TableStore;     // os 12 dict_*
    fn http(&self) -> &dyn Http;             // http_post
    fn llm(&self) -> &dyn Llm;               // ask_ai, ai_switch, ai_extract, ai_answer
}
```

`instance_key` é o que tira `InstanceScope::PerCamera` do core: o core vê uma string opaca,
`""` continua sendo a instância única, byte a byte.

`Approvals` é o seam do `device_gate`. O protocolo difícil já existe e é bom — settle atômico
com `state='pending'` no WHERE + RETURNING, token só como hash sha256, TTL, e uma varredura
de entrega durável que expira-ao-reivindicar numa única instrução. O que é do Sentinel é
só o canal (push Expo + FK para `push_devices`).

### 5.5 Um scheduler, não dois

O `redoma-plano.md` §16.3 pede `graph-exec-stream` e `graph-exec-durable` separados. Os
primitivos dizem outra coisa: `run_subpath_eventful` já força entrada num nó específico, e o
caminho `WINDOW_STOP_MARKER` já reidrata os outputs dos nós anteriores a partir de
`workflow_node_state`. Durável = o mesmo scheduler com outra política:

```rust
pub trait Checkpoint {
    fn on_enter(&self, run: &RunKey) -> Option<HashMap<u32, PortValues>>;  // reidratar
    fn after_node(&self, run: &RunKey, node: u32, out: &PortValues);       // persistir
}
```

Sentinel pluga a política de hoje (grava no fim do run, reidrata só em `__window_stop`);
Redoma pluga `EveryNode`. Um scheduler, duas políticas, testados pelos mesmos 36 testes.

**Sendo honesto sobre o tamanho:** checkpoint no meio do run **não existe hoje**. `outputs`,
`ran`, `variables` vivem num `HashMap` em memória e só vão para o banco depois que o run
termina; pod morre no meio, `mark_orphaned_runs` marca `failed` e abandona. Isso é código
novo de verdade — só não é um scheduler novo.

Também não existem, e o Redoma pede os quatro (§22): **prioridade**, **justiça entre
tenants** no claim, **heartbeat** (o lease é fixo em 600s e um job lento é roubado) e
**dead-letter durável** (hoje o pg_cron apaga `failed` com 2 dias). Nenhum é da engine —
todos são da fila, e entram no `ggraph-pg`.

---

## 6. Como o bishop não muda

**(a) Um módulo chamado `NodeKind`.** `NodeKind::OnEvent` é um *caminho*, não uma chamada de
método — um módulo resolve igual:

```rust
#[allow(non_upper_case_globals)]
pub mod kinds {
    pub const OnEvent: NodeId = NodeId::new_static("on_event");
    // … 91
    pub static ALL: &[NodeId] = &[ /* 90, sem Reroute */ ];
}
pub use kinds as NodeKind;
```

**(b) Registry de processo + wrappers com a arity de hoje** — `run_dag_eventful`,
`run_subpath_eventful`, `on_event_node`, `Fire`, `WorkflowEngine`.
`pub type Graph = ggraph_core::Graph<SentinelMeta>;`

**(c) Re-export de valores.** `pub use ggraph_core::{Value as PortValue, PortValues};`
`PortValue::{Text,Num,Bool,List}` e `as_text`/`as_num`/`as_bool` sobrevivem literais.

**O diff real nos consumidores:**

- `sentinel-flow`: **~3 linhas**. Só `event_consumer.rs:146` constrói `PortValue::Image(bytes)`,
  que vira `PortValue::image(bytes)`. Todo o resto é fachada.
- `sentinel-control`: **~35 linhas removidas.** `catalog()` chama oito métodos por-kind num
  enum `Copy`; nenhum sobrevive ao registry. A correção é uma melhora: `catalog()` vira
  `app_core::workflows::catalog_json()` e o control fica com três linhas. O control não tem
  por que saber o que é uma `Port`.

---

## 7. Fases

Cada fase é um MR (ou uma pilha curta), mergeável sozinha, com um oráculo.

### Fase 0 — fazer os testes existirem (½ dia, ~20 linhas) — **bloqueia tudo**

`check:rust` roda `cargo check --workspace --all-targets`, que **compila** os 110 testes da
engine e nunca os **executa**. Os únicos `cargo test` do pipeline são `sql_prepare`,
`tenant_isolation` e o `media_smoke` do edge — nenhum toca `workflows::`. Com a regra de não
construir nada local, hoje **não existe rede de segurança**.

1. `cargo test -p app-core --lib workflows` no `check:rust`. Roda sem banco: `Db::lazy` tem
   `acquire_timeout(3s)` e os `repo::*` fazem `.ok().flatten()`.
2. **Snapshot do catálogo** — o JSON de `catalog_json()` commitado como fixture. Qualquer
   mudança de slug/label/porta/default vira diff, não chamado de cliente.
3. **Grafos-ouro** — um grafo real exportado de produção por forma (`if`, `for_each`,
   `delay_trigger`, `string_switch`, `on_event`), desserializado e rodado contra um host de
   mentira, com asserção de quais nós rodaram e em que ordem.

**Prova:** renomeie um slug num branch descartável. O CI tem que ficar vermelho. Se ficar
verde, o oráculo é falso e toda fase seguinte está sem verificação.

Vale por si só, mesmo que a extração seja abandonada.

### Fase 1 — nasce o repo `ggraph` (~3 dias) — **desbloqueia o Redoma**

Cria `git@github.com:jackesdavid/graph-engine.git` com `ggraph-core` contendo o que já é agnóstico hoje mais o desenho
novo: `graph/`, `port/`, `value/`, `spec/`, `registry/`, `host/`. Sem nós ainda, ou com os
puros (`logic`, `format`, `print`) para provar o formato de `nodes/`.

Os 34 testes que já são agnósticos (22 de `domain.rs` sobre validação de wire/topo/back-edge,
12 de `values.rs` sobre round-trip do codec) mudam de repo junto — é a prova da extração.

**O bishop não consome nada ainda.** O Redoma começa aqui, na `v0.1.0`.

### Fase 2 — um nó por arquivo + registry, in place no bishop (~900 linhas, 8–12 MRs)

`NodeKind` → `NodeId`, `PortType` → newtype aberto, `NodeRegistry` + `NodeSpec` introduzidos
**dentro** do app-core, uma família de nós por MR (lógica, controle, variáveis, texto/debug,
display, tabelas, câmera, visão, LLM, alertas/recipients/telegram, gravação/presença/tuya).
Cada MR tira aquela família dos 14 sítios de despacho e apaga os arms.

Aqui entram `aliases`, `hidden`, `Ports::Dynamic` para os 8 kinds de portas derivadas de
config, e `ExecOut::Dynamic` para os três switches (atenção: `camera_switch` nomeia arms
`arm_1..arm_N` posicionalmente e `string_switch`/`ai_switch` usam o rótulo — o wire depende
dos dois).

**Prova:** snapshot do catálogo **byte-idêntico** depois de cada MR. Testes verdes. Um grafo
real rodado em staging por família.

### Fase 3 — `Behavior::Step` (~600 linhas, 3 MRs)

Os 11 blocos engine-driven saem do `forward_epoch` para seus arquivos de nó. `engine.rs` cai
de 2.645 para ~900. `loop_state` vira `scratch` no `StepCx`.

**Prova:** os 36 testes de `engine.rs` inalterados e verdes; `grep -c "n.kind ==" engine.rs`
→ 0; grafos-ouro inalterados.

### Fase 4 — `Value` e codec (~450 linhas, 2 MRs)

`Extern` entra; os 10 tipos do Sentinel viram `ExternValue` com suas tags atuais; `ValueIo`
substitui `crate::media`; `Num::Float` e `Map<String,Value>` entram **só como tipos**.

**Prova:** os 12 testes de round-trip inalterados, mais um por tag, contra fixtures de linhas
reais de `workflow_node_state` puxadas de staging.

### Fase 5 — `Host` (~350 linhas, 1 MR)

`StateStore`/`Observer`/`ValueIo`/`schedule`/`instance_key`/`approvals`/`tables`/`http`/`llm`.
`SentinelHost` embrulha `ToolContext`. `resolve_camera_selector`, `resolve_region`,
`on_event_node`, `InstanceScope` saem para `workflows/sentinel.rs`.

**Prova mecânica, e vira job de CI:**
`grep -o "crate::[a-z_]*" engine.rs | grep -v workflows` devolve **nada** (hoje: 8 módulos).

### Fase 6 — a troca (~0 lógica, 2 MRs)

`git subtree split` leva os arquivos já desacoplados para o `ggraph` **preservando histórico**
(importa: vários comentários no scheduler explicam incidentes — `domain.rs:1691-1717` são 26
linhas explicando por que sub-runs matavam sources órfãos, #114/#115). Tag `v0.2.0`. O bishop
adiciona a dep git, apaga os originais e mantém `app_core::workflows` como fachada.

**Prova:** snapshot do catálogo ainda idêntico; grafos-ouro passam; `cargo check --workspace`
e `cargo check --manifest-path sentinel-edge/Cargo.toml` verdes; um deploy de staging com
grafo de cliente rodando.

### Fase 7 — o ternário (§16.6 opção B) (~150 linhas)

`if` ganha `ExecOut::Dynamic`: dois arms por padrão, três (`true`/`false`/`unknown`) quando
`config.unknown_arm == true`. `ask_ai` idem — "o modelo não soube dizer" é exatamente o caso
colapsado. `compare`/`text_compare` ganham a saída tri-estado.

Precedente que já existe no código: `EXEC_OUT_DEVICE_GATE = [positive, negative, failure]`
(`domain.rs:1235`), com o comentário *"a separação é deliberada: `negative` é uma RESPOSTA,
`failure` é a ausência dela"* — que é o §16.6 escrito antes do §16.6. Por isso o terceiro arm
do `if` se chama **`unknown`**, não `error`.

Tem que ser opcional: `if`'s arms vêm de um `&'static [Port; 2]`, então um terceiro pino
incondicional faria **todo `if` de todo cliente crescer um pino no editor** via `catalog()`.

### Fase 8 — as lacunas do Redoma, no `ggraph-pg` (paralelo, não bloqueia)

`Checkpoint::EveryNode`, prioridade, justiça entre tenants no claim, heartbeat, dead-letter
durável, custo por job. Nenhuma existe hoje.

---

## 8. Repo, versionamento e CI

**Repo: `git@github.com:jackesdavid/graph-engine.git`** (GitHub), e o projeto inteiro voltando para lá depois.

**Tag git, não branch.** `git = "https://github.com/jackesdavid/graph-engine.git", tag = "v0.1.0"`.
Com `branch = "main"` o `Cargo.lock` vira o único pino e um `cargo update` distraído move os
dois produtos de uma vez.

**Dia a dia com `[patch]` local, mas fora do `Cargo.toml` do workspace** — em `.cargo/config.toml`
**gitignorado**, com um `.cargo/config.toml.example` commitado. No workspace ele valeria
também no CI e no Docker, onde o path não existe. E um job de guarda de quatro linhas:
`grep -q 'path+file' Cargo.lock` → falha. Build com patch reescreve o lock com
`path+file:///Users/...`; commitar isso quebra o CI de todo mundo.

**Três sítios de autenticação, não um** — e é aqui que dói se `ggraph` for privado:

1. `check:rust` / `check:sql` — `git config url.insteadOf` no `before_script`.
2. **O estágio builder do Docker.** O fetch acontece *dentro* do contêiner, durante
   `cargo chef cook`, onde a credencial do CI não existe. Tem que ser **BuildKit secret**
   (`--mount=type=secret`), nunca `ARG` (fica no histórico da imagem), e o `git config` não
   pode ser `--global` (escreve `/root/.gitconfig`, que *entra* na layer).
3. **O runner shell do macOS**, que roda como seu usuário na sua máquina. `git config --global`
   ali persiste um token rotativo no seu gitconfig pessoal. Usar `GIT_CONFIG_GLOBAL` escopado
   ao workspace do job.

**Recomendação: `graph-engine` público.** Elimina os três de uma vez, e é código sem segredo de
negócio — topologia, portas e nós padrão. Se for privado, os três são trabalho real e
precisam entrar na Fase 6.

**Duas armadilhas que vão morder na primeira adição:**

- `sentinel-edge` depende de `app-core` **por path** e está fora do workspace, com
  `Cargo.lock` próprio. O job `verify:edge-lock` roda `--locked` e vai falhar sem nenhum
  arquivo do edge ter mudado — foi exatamente o que aconteceu com o `lettre` (#165). Todo
  bump do ggraph regenera **dois** lockfiles no mesmo commit.
- `cargo chef` é chaveado no `Cargo.lock` e **não há cache de registry cargo nenhum** no
  pipeline. Cada tag do ggraph vira recompilação fria de tract/ONNX nas quatro imagens
  (~16-18 min por builder, pelos comentários do Dockerfile). Agrupar bumps; não bumpar a cada MR.

**Um item de sequenciamento:** se o bishop vai voltar para o GitHub de qualquer forma, fazer a
migração **antes** da Fase 6 economiza montar a autenticação de dep privada duas vezes. Isso é
decisão sua sobre prioridade — não bloqueia as Fases 0 a 5, que são todas dentro do bishop.

---

## 9. Verificação

Sem os oráculos, este plano é um refactor de 3.400 linhas sem rede. Os quatro:

1. **`cargo test -p app-core --lib workflows`** no `check:rust` (Fase 0). Sem isso, nada.
2. **Snapshot do catálogo** — fixture JSON commitado, comparado byte a byte. É o contrato com
   o editor Svelte, que hoje não é verificado por nada além de convenção, e que **já derivou**:
   `portColors.ts` lista 22 tipos de porta, o Rust tem 23 (`landmarks` falta).
3. **Grafos-ouro** — grafos reais de produção, rodados contra um host de mentira, asserção
   sobre quais nós rodaram e em que ordem.
4. **Grep de acoplamento como job de CI** — `crate::` fora de `workflows` nos arquivos
   destinados ao core tem que dar zero, e o vocabulário de domínio (`frame`, `rtsp`, `camera`,
   `pdf`, `email`, `chunk`) tem que falhar o build dentro do `ggraph-core`. É o teste do
   §16.4 do plano do Redoma, e é o único que impede a fronteira de apagar em três meses.

Fim a fim, depois da Fase 6: subir o `sentinel-flow` em staging, disparar um grafo de cliente
com `if` + `for_each` + `delay_trigger`, conferir o histórico do run e as linhas de log; e no
`ggraph`, rodar os dois grafos de exemplo (um contínuo, um durável) que servem de documentação
executável e de detector de regressão de agnosticismo.


---

# Registro: o que a Fase 1 mudou no plano (2026-08-24)

O `ggraph-core` existe e roda grafos de verdade —
[github.com/jackesdavid/graph-engine](https://github.com/jackesdavid/graph-engine), 21 nós,
117 testes, clippy limpo, os dois guardas de agnosticismo do §16.4 rodando no CI. Três coisas
que este plano dizia e que o código desmentiu.

## 1. Não são dois schedulers. É um scheduler e uma política.

O §5.5 apostava nisso e a aposta se confirmou mais forte do que estava escrito. `graph-exec-stream`
e `graph-exec-durable` diferem em **uma** coisa, e não é a ordenação dos nós: é com que frequência
o run é gravado.

```rust
RunOptions::default()   // Checkpoint::None      — nada gravado durante o run
RunOptions::durable()   // Checkpoint::EveryNode — cada nó gravado ao completar
```

A alternativa era duas cópias do laço de épocas, da regra de ramo morto e da semântica de pull,
divergindo em silêncio. **O crate `ggraph-exec-durable` não existe e não deve existir.**

Três regras caíram de escrever isso, cada uma um bug se fosse ao contrário:

- Run novo **nunca** é semeado com sobras de um run suspenso — valores velhos pareceriam vivos e
  ressuscitariam ramos que a engine deixou mortos de propósito.
- O nó reentrado **não** é restaurado. Uma retomada entrega algo *ao* nó que esperava; devolver a
  ele o estado que tinha antes de perguntar é o oposto do que ele precisa.
- Run que termina limpa os próprios checkpoints. No disco deve haver ou um run em voo ou nada.

## 2. O checkpoint no meio do run era o maior risco, e ele está resolvido

O §22 listava isso como "o único item que muda o tamanho da fase". Está feito, com seis testes
que mostram inclusive o que o `Checkpoint::None` custa — o nó de baixo lê um branco.

Continuam faltando, e são todos da **fila**, não da engine: prioridade, justiça entre tenants,
heartbeat e dead-letter durável.

## 3. Uma decisão apareceu cinco vezes, e o plano só previa uma

O §16.6 pedia saída ternária no branch. Escrevendo os nós, a mesma decisão reapareceu em
`llm_decide`, em `approval`, em `compare` (devolve *nada* em vez de valor errado quando falta
operando) e em `table_find`. **Não saber é um desfecho comum**, e a regra geral é mais ampla do
que "todo nó de comparação tem três saídas": todo nó que extrai ou decide precisa poder dizer
que não sabe.

O terceiro braço do `if` é opcional (`config.unknown_arm`), porque braço de exec é pino no
canvas — ligar por padrão faria todo `if` já salvo crescer um pino na frente de todo mundo.

## 4. Duas armadilhas confirmadas, uma nova

Confirmadas: o alias `notify_beacon` (o `NodeSpec` tem `aliases`) e o `ALL` com 90 de 91 (tem
`hidden`). Nova: `SmolStr::new_static` guarda o ponteiro e **nunca** aloca, qualquer que seja o
tamanho — o orçamento de 23 bytes só vale para id construído em runtime. É por isso que arquivo
de nó declara o id como constante.

## Próximo passo no bishop

Fase 2: um nó por arquivo e o registry, **in place** no app-core, uma família por MR, com o
snapshot do catálogo tendo que ficar byte-idêntico depois de cada uma.


---

# Registro: `Graph<SentinelMeta>` não é uma fatia (2026-08-25)

**Tentado e revertido.** O plano listava `Graph<SentinelMeta>` e a conversão `NodeKind` → `NodeId`
como fatias separadas, a segunda depois da primeira. **Elas são uma só**, e a razão é estrutural:

```
Graph<M>  contém  Vec<GraphNode>  contém  kind: NodeId
```

Adotar o `Graph` da engine adota o `GraphNode` dela, que carrega identificador aberto — então
todo lugar que hoje faz `n.kind == NodeKind::X`, `n.kind.slug()`, `match self.kind { ... }` ou
`impl GraphNode` entra junto. São os 828 braços de despacho, não um refactor de container.

## O que a tentativa mediu

Fui de ~200 erros de compilação até 28 antes de a próxima camada aparecer. A distribuição é útil
para quem for fazer de verdade:

| | |
|---|---|
| `add_node(NodeKind::X, ..)` | 144 — **resolvido na engine**, não aqui |
| acessos a campo do produto (`g.debug` → `g.meta.debug`) | 80, guiados pelo compilador |
| `.add_edge(` → `.wire(` | 92, rename mecânico |
| `n.kind == NodeKind::X` | 16 — **resolvidos por um impl**, não um por um |
| métodos em `NodeId`/`GraphNode` | ~25, à mão |

## A técnica que funciona, e vale reusar

Duas linhas absorveram 16 sítios sem tocar em nenhum:

```rust
impl PartialEq<NodeKind> for ggraph_core::NodeId { … compara slug … }
impl From<NodeKind> for ggraph_core::NodeId { … new_static(k.slug()) … }
```

Mais um trait `AsKind` devolvendo `Option<NodeKind>` — `None` para um kind que este build não
conhece, em vez de uma resposta errada.

**Isso é o que torna a conversão grande viável**: o enum vira uma *vista* sobre o identificador
aberto, e o código que ainda raciocina em enum continua legível enquanto é migrado família por
família. Sem isso, é reescrever 828 braços de uma vez.

## Melhoria que já foi para a engine

`add_node` passou a aceitar `impl Into<NodeId>` (v0.9.1). Um consumidor migrando de enum fechado
tem uma chamada por nó que já criou — 144 aqui — e fazer cada uma dizer `.into()` é ruído que
sobrevive à migração por anos.

## Ordem correta, revisada

1. `NodeKind` → `NodeId` **com** `Graph`/`GraphNode`, numa fatia só, usando a camada de
   compatibilidade acima e migrando família por família com o snapshot do catálogo provando cada
   passo.
2. Depois a troca do scheduler, que o teste diferencial já sabe medir.

E a razão de reverter em vez de continuar: cada rodada revelava a camada seguinte, o que é o
sinal de que a estimativa estava errada — não de que faltava pouco.


---

# A troca do scheduler, medida (2026-08-25)

O teste diferencial (`workflows/differential.rs`) roda o mesmo grafo pelos dois schedulers e
exige a mesma sequência de nós. **`KNOWN_DIFFERENCES` está vazio** e cinco formas concordam:
cadeia, ramo, laço, cooldown e flip-flop.

Isso torna a troca pagável em parcelas: cada kind convertido em `NodeStep` vira mais uma
concordância medida, e o que falta é uma lista com dificuldade real em vez de estimativa.

## Convertidos

| kind | onde | o que exercita |
|---|---|---|
| `ForEach` | `steps/for_each.rs` | reentrada; o índice saiu do `loop_state` do driver para o `scratch` da engine |
| `Cooldown` | `steps/cooldown.rs` | estado durável; o carimbo fica em ms sob `last_ms` por compatibilidade |
| `FlipFlop` | `steps/flip_flop.rs` | estado durável alternado |
| `Comment` | registry | `Behavior::Inert` — real, ligável, sem comportamento |

## O que falta, por dificuldade medida

| kind | linhas inline | o que torna difícil |
|---|---|---|
| ~~`EmitEvent`~~ | ~~10~~ | **feito** (`steps/emit_event.rs`) — e a mudança estrutural que ele exigia valeu: `EventEmitter` é um trait no `ToolContext`, populado pelo flow, do jeito que `ui_tx` já era. O nó alcança direto e nenhum trait da engine aprendeu o que é um evento aqui. Contexto sem barramento deixa `None` e o nó **diz** que não disparou, em vez de reportar um disparo que não chegou a ninguém. |
| ~~`Gauge`/`Text`~~ | ~~24~~ | **feito** (`steps/display.rs`) — e a lição está lá: o nó alcança `ui_tx` pelo `tools()`, direto. O `UiEvent` da engine tem duas variantes e teria de crescer uma por produto para carregar um `ShowValue`, que é exatamente o acoplamento que a extração existe para evitar. |
| ~~`ShowImage`~~ | ~~32~~ | **feito** (`steps/show_image.rs`) |
| `OnEvent` | 44 | semeadura de entrada; é o gatilho, não um nó comum |
| `DelayTrigger` | 132 | janela durável com aresta de descida |
| `DeviceGate` | 195 | aprovação humana; suspende o run e retoma noutro |
| `TimeGate` | — | **cria uma thread guardiã** e usa `active_gates` em memória |

Os três últimos são os que decidem o tamanho do resto. `TimeGate` é o pior: uma thread viva não
é algo que `NodeStep` expresse, e resolvê-lo provavelmente significa que ele vira `schedule()` +
reentrada, como o `wait` da engine — o que seria uma melhoria, mas é mudança de comportamento e
precisa do próprio card.

## O que NÃO fazer

Converter um kind e deixar o scheduler daqui usando o bloco inline **é o estado correto** durante
a migração: os dois caminhos existem, e o diferencial prova que concordam. A troca em si é o
último passo, quando os doze estiverem convertidos e a lista de formas cobrir o que importa.
