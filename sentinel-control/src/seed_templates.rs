//! Idempotent seeder for the shared **global workflow templates** — a starter
//! curriculum every tenant sees in the templates picker (card #136).
//!
//! The templates are owned by a stable `platform` tenant so `load_global_templates`
//! surfaces them cross-tenant. Insert is **if-absent** (`ON CONFLICT (id) DO NOTHING`)
//! against a stable per-template UUID, so an operator's later visual edits — assume the
//! platform tenant → open the template in the editor → drag/tweak — are NEVER clobbered
//! by a redeploy. To ship a fresh revision of a template, give it a new stable id.
//!
//! Graphs are built through the validated domain API (`Graph::add_node` / `add_edge`), so
//! a mis-wired template fails to build rather than shipping a broken graph.
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use app_core::workflows::domain::{Graph, NodeKind};

use crate::store;

/// The stable tenant that owns the shared template catalog. Tenants never see this tenant
/// itself — only its `global` templates, surfaced in every picker by `load_global_templates`.
const PLATFORM_SLUG: &str = "platform";

pub async fn run(pool: &PgPool) -> sqlx::Result<()> {
    // Resolve (or create) the platform tenant.
    let tenant_id = match store::tenant_id_by_slug(pool, PLATFORM_SLUG).await? {
        Some(id) => id,
        None => {
            let (id, _user, _perms) = store::provision_tenant_with_admin(
                pool,
                "Platform",
                PLATFORM_SLUG,
                "platform@sentinel.local",
                None,
            )
            .await?;
            log::info!("seed: created Platform tenant (owns the global workflow templates)");
            id
        }
    };

    let mut inserted = 0u64;
    for g in templates() {
        let data = serde_json::to_value(&g).unwrap_or_else(|_| json!({}));
        let res = sqlx::query(
            "INSERT INTO workflows (tenant_id, id, data) VALUES ($1, $2, $3) ON CONFLICT (tenant_id, id) DO NOTHING",
        )
        .bind(tenant_id)
        .bind(g.id)
        .bind(&data)
        .execute(pool)
        .await?;
        inserted += res.rows_affected();
    }
    if inserted > 0 {
        log::info!("seed: inserted {inserted} global workflow template(s)");
    }
    Ok(())
}

/// Every global template, in curriculum order. Built via the validated domain API.
fn templates() -> Vec<Graph> {
    vec![
        onboarding_tour(),  // L0
        alerta_pessoa(),    // L1
        quem_na_porta(),    // L1
        snapshot_anotado(), // L2
        clipe_movimento(),  // L2
        bot_comandos(),     // L3
        bot_linguagem(),    // L3
        relatorio_diario(), // L4
        cruzamento_linha(), // L4
        reconhece_rosto(),  // L5
        luz_presenca(),     // L5
        presenca_mapa(),    // L6
    ]
}

// ---- builders ---------------------------------------------------------------

/// Stable id namespace for seeded templates — keeps re-seeding idempotent across boots.
fn stable_id(n: u128) -> Uuid {
    Uuid::from_u128(0x5e57_1e10_7e37_0000_0000_0000_0000_0000 | n)
}

/// A blank template shell: stable id, marked template + global, grouped under "Aprender".
fn template(id: u128, name: &str, desc: &str) -> Graph {
    let mut g = Graph::new(name);
    g.id = stable_id(id);
    g.template = true;
    g.global = true;
    g.description = desc.to_string();
    g.group = "Aprender".to_string();
    g
}

fn set_cfg(g: &mut Graph, node: u32, cfg: Value) {
    if let Some(n) = g.node_mut(node) {
        n.config = cfg;
    }
}

// Wire an edge; a wrong port name panics the build (caught by the `templates_build_*` test).
fn wire(g: &mut Graph, from: u32, from_port: &str, to: u32, to_port: &str) {
    g.add_edge(from, from_port, to, to_port)
        .unwrap_or_else(|e| panic!("template edge {from}:{from_port} → {to}:{to_port}: {e}"));
}
// Sequence: `from`'s exec output → `to`'s universal `exec_in`.
fn ex(g: &mut Graph, from: u32, from_exec: &str, to: u32) {
    wire(g, from, from_exec, to, "exec_in");
}
// A Title heading doc node.
fn title(g: &mut Graph, x: i32, y: i32, text: &str) -> u32 {
    let n = g.add_node(NodeKind::Title, x, y);
    set_cfg(
        g,
        n,
        json!({ "text": text, "color": "#e5e7eb", "size": "lg" }),
    );
    n
}
// A Markdown explainer doc node.
fn note(g: &mut Graph, x: i32, y: i32, md: &str) -> u32 {
    let n = g.add_node(NodeKind::Markdown, x, y);
    set_cfg(g, n, json!({ "text": md, "width": 380, "height": 150 }));
    n
}

/// #0 — a documentation-only canvas (Title / Markdown / Comment) that teaches the editor.
/// No ports, no execution — the inaugural lesson and a showcase of the Display nodes.
fn onboarding_tour() -> Graph {
    let mut g = template(
        0x01,
        "Comece aqui — tour do editor",
        "Introdução ao editor de workflows: gatilhos, nós, portas e como rodar. Só documentação — nada executa.",
    );

    let title = g.add_node(NodeKind::Title, 40, 40);
    set_cfg(
        &mut g,
        title,
        json!({ "text": "Bem-vindo ao editor de workflows", "color": "#e5e7eb", "size": "xl" }),
    );

    let intro = g.add_node(NodeKind::Markdown, 40, 150);
    set_cfg(
        &mut g,
        intro,
        json!({
            "text": "## O que é um workflow\n\nUm grafo de **nós** ligados por fios.\n\n- Fios de **execução** definem a ORDEM\n- Fios de **dados** passam valores tipados (imagem, texto, número, câmera…)\n- Adicione nós pelo palete (tecla `n`)\n- Ligue uma saída a uma entrada arrastando entre as bolinhas",
            "width": 430, "height": 240
        }),
    );

    let triggers = g.add_node(NodeKind::Markdown, 510, 150);
    set_cfg(
        &mut g,
        triggers,
        json!({
            "text": "## Como um grafo dispara\n\n- **Run now** — você aperta o botão\n- **When Event (evento)** — reage a movimento, telegram, rosto…\n- **When Event (agenda)** — o mesmo nó em modo horário: diário, semanal, mensal, no seu fuso\n\nO horário mora no NÓ, então um grafo pode ter várias agendas independentes.",
            "width": 380, "height": 200
        }),
    );

    let tip = g.add_node(NodeKind::Comment, 40, 420);
    set_cfg(
        &mut g,
        tip,
        json!({
            "text": "Dica: use \"Usar template\" para duplicar este fluxo na sua conta e começar a mexer. Nada aqui executa — é só a aula.",
            "color": "#facc15", "width": 850, "height": 84
        }),
    );

    g
}

// ---- L1 — Fundamentals ------------------------------------------------------

/// #1 — When Event(motion) → frame → person detection → Compare/If → Cooldown → Notify (with photo).
///
/// Used to poll on a 2s graph cron. That trigger is gone (migration `0095`), and movement is the
/// thing the poll was looking for anyway: the graph now wakes when the edge reports motion instead
/// of decoding a frame thirty times a minute to find out nothing happened.
fn alerta_pessoa() -> Graph {
    let mut g = template(
        0x02,
        "Alerta de pessoa",
        "Detecta pessoas quando há movimento na câmera → te avisa no Telegram, com foto e anti-spam.",
    );
    title(&mut g, 40, -120, "Alerta de pessoa");
    note(&mut g, 40, -60, "**When Event(motion)** pega o frame, detecta pessoas e — se houver — te avisa.\n\n- **Compare** count > 0\n- **Cooldown** evita spam (5 min)\n- **Notify** com a foto\n\nEscolha a câmera no nó Get current frame.");
    let onev = g.add_node(NodeKind::OnEvent, -220, 120);
    set_cfg(&mut g, onev, json!({ "event": "motion" }));
    let frame = g.add_node(NodeKind::GetCurrentFrame, 40, 120);
    set_cfg(
        &mut g,
        frame,
        json!({ "camera": "first", "max_age_ms": "2000" }),
    );
    let det = g.add_node(NodeKind::PersonDetection, 300, 120);
    let cmp = g.add_node(NodeKind::Compare, 300, 320);
    set_cfg(&mut g, cmp, json!({ "operator": ">", "a": "0", "b": "0" }));
    let iff = g.add_node(NodeKind::If, 560, 120);
    let cool = g.add_node(NodeKind::Cooldown, 820, 120);
    set_cfg(&mut g, cool, json!({ "window_secs": "300" }));
    let notify = g.add_node(NodeKind::NotifyRecipient, 1080, 120);
    set_cfg(&mut g, notify, json!({ "message": "Pessoa detectada" }));
    ex(&mut g, onev, "exec_out", frame);
    ex(&mut g, frame, "exec_out", det);
    ex(&mut g, det, "exec_out", iff);
    wire(&mut g, det, "count", cmp, "a");
    wire(&mut g, cmp, "result", iff, "condition");
    wire(&mut g, iff, "true", cool, "exec_in");
    ex(&mut g, cool, "exec_out", notify);
    wire(&mut g, frame, "image", notify, "image");
    g
}

/// #2 — Snapshot → Describe image (vision LLM) → Notify.
fn quem_na_porta() -> Graph {
    let mut g = template(
        0x03,
        "Quem está na porta?",
        "Tira uma foto, pergunta pra IA o que está vendo e te manda a resposta.",
    );
    title(&mut g, 40, -120, "Quem está na porta?");
    note(&mut g, 40, -60, "**Snapshot** → **Describe image** (a IA olha a foto e responde a pergunta) → **Notify**.\n\nRode com **Run now**. Precisa de `ANTHROPIC_API_KEY`.");
    let snap = g.add_node(NodeKind::CameraSnapshot, 40, 120);
    set_cfg(&mut g, snap, json!({ "camera": "first" }));
    let desc = g.add_node(NodeKind::DescribeImage, 320, 120);
    set_cfg(
        &mut g,
        desc,
        json!({ "label": "", "question": "Tem alguém na porta? O que você vê?" }),
    );
    let notify = g.add_node(NodeKind::NotifyRecipient, 600, 120);
    set_cfg(&mut g, notify, json!({ "message": "" }));
    ex(&mut g, snap, "exec_out", desc);
    ex(&mut g, desc, "exec_out", notify);
    wire(&mut g, snap, "image", desc, "image");
    wire(&mut g, desc, "description", notify, "message");
    wire(&mut g, snap, "image", notify, "image");
    g
}

// ---- L2 — Richer reactions --------------------------------------------------

/// #3 — Detect objects → For each → Break → Draw BBox/Text → Show image.
fn snapshot_anotado() -> Graph {
    let mut g = template(
        0x04,
        "Snapshot anotado",
        "Detecta objetos, desenha caixa + rótulo em cada um e mostra no canvas.",
    );
    title(&mut g, 40, -120, "Snapshot anotado");
    note(&mut g, 40, -60, "**Detect objects** → **For each** detecção → **Break** → **Draw BBox** + **Draw Text** → **Show image**.\n\nTroque a classe (person/car/…) no nó Detect objects.");
    let snap = g.add_node(NodeKind::CameraSnapshot, 40, 140);
    set_cfg(&mut g, snap, json!({ "camera": "first" }));
    let det = g.add_node(NodeKind::ObjectDetection, 300, 140);
    set_cfg(&mut g, det, json!({ "class": "person" }));
    let fe = g.add_node(NodeKind::ForEach, 540, 140);
    let brk = g.add_node(NodeKind::BreakDetection, 780, 140);
    let dbox = g.add_node(NodeKind::DrawBbox, 1020, 140);
    set_cfg(
        &mut g,
        dbox,
        json!({ "color": "#00FF00", "thickness": "2", "style": "reticle", "expand": "0" }),
    );
    let dtext = g.add_node(NodeKind::DrawText, 1260, 140);
    set_cfg(
        &mut g,
        dtext,
        json!({ "text": "objeto", "position": "top-left", "size": "18", "text_color": "#FFFFFF", "background": true, "bg_color": "#000000" }),
    );
    let show = g.add_node(NodeKind::ShowImage, 1500, 140);
    set_cfg(&mut g, show, json!({ "label": "anotado" }));
    ex(&mut g, snap, "exec_out", det);
    ex(&mut g, det, "exec_out", fe);
    wire(&mut g, det, "detections", fe, "items");
    wire(&mut g, fe, "loop_body", brk, "exec_in");
    ex(&mut g, brk, "exec_out", dbox);
    ex(&mut g, dbox, "exec_out", dtext);
    ex(&mut g, dtext, "exec_out", show);
    wire(&mut g, fe, "item", brk, "detection");
    wire(&mut g, snap, "image", dbox, "image");
    wire(&mut g, brk, "bbox", dbox, "bbox");
    wire(&mut g, dbox, "image", dtext, "image");
    wire(&mut g, dbox, "bbox", dtext, "bbox");
    wire(&mut g, dtext, "image", show, "image");
    g
}

/// #4 — On motion → Delay Trigger (debounce) → Start/Stop recording.
fn clipe_movimento() -> Graph {
    let mut g = template(
        0x05,
        "Clipe em movimento",
        "Ao detectar movimento, grava um clipe — com debounce pra um clipe por rajada.",
    );
    title(&mut g, 40, -120, "Clipe em movimento");
    note(&mut g, 40, -60, "**When Event(motion)** → **Delay Trigger** (leading **start**, trailing **stop**) → **Start/Stop recording**.\n\nUma rajada de movimento = um clipe. Escolha a câmera nos nós de gravação.");
    let onev = g.add_node(NodeKind::OnEvent, 40, 140);
    set_cfg(&mut g, onev, json!({ "event": "motion" }));
    let delay = g.add_node(NodeKind::DelayTrigger, 300, 140);
    set_cfg(&mut g, delay, json!({ "wait_secs": 10, "extend_secs": 10 }));
    let startrec = g.add_node(NodeKind::StartRecording, 560, 60);
    set_cfg(&mut g, startrec, json!({ "timeout_secs": "30" }));
    let stoprec = g.add_node(NodeKind::StopRecording, 560, 240);
    set_cfg(&mut g, stoprec, json!({ "timeout_secs": "15" }));
    ex(&mut g, onev, "exec_out", delay);
    wire(&mut g, delay, "start", startrec, "exec_in");
    wire(&mut g, delay, "stop", stoprec, "exec_in");
    wire(&mut g, startrec, "clip_id", stoprec, "clip_id");
    g
}

// ---- L3 — Telegram bot ------------------------------------------------------

/// #5 — On telegram_message → String switch (commands) → snapshot → reply.
fn bot_comandos() -> Graph {
    let mut g = template(
        0x06,
        "Bot de comandos",
        "Responde comandos do Telegram (/snap, /status) — o começo de um bot.",
    );
    title(&mut g, 40, -120, "Bot de comandos");
    note(&mut g, 40, -60, "**When Event(telegram_message)** → **String switch** por comando → ação → **Reply to sender**.\n\nDepois de clonar, ligue `chat_id` (saída do When Event) no Reply to sender.");
    let onev = g.add_node(NodeKind::OnEvent, 40, 140);
    set_cfg(&mut g, onev, json!({ "event": "telegram_message" }));
    let sw = g.add_node(NodeKind::StringSwitch, 300, 140);
    set_cfg(&mut g, sw, json!({ "arms": ["/snap", "/status"] }));
    let snap = g.add_node(NodeKind::CameraSnapshot, 560, 60);
    set_cfg(&mut g, snap, json!({ "camera": "first" }));
    let reply = g.add_node(NodeKind::ReplyToSender, 820, 60);
    set_cfg(&mut g, reply, json!({ "message": "Aqui está" }));
    let status = g.add_node(NodeKind::ReplyToSender, 560, 240);
    set_cfg(&mut g, status, json!({ "message": "Tudo funcionando ✅" }));
    ex(&mut g, onev, "exec_out", sw);
    wire(&mut g, sw, "/snap", snap, "exec_in");
    ex(&mut g, snap, "exec_out", reply);
    wire(&mut g, snap, "image", reply, "image");
    wire(&mut g, sw, "/status", status, "exec_in");
    g
}

/// #6 — On telegram_message → AI switch (intent) → snapshot / AI answer → reply.
fn bot_linguagem() -> Graph {
    let mut g = template(
        0x07,
        "Bot em linguagem natural",
        "Entende o pedido em português (não só comandos) e responde de acordo.",
    );
    title(&mut g, 40, -120, "Bot em linguagem natural");
    note(&mut g, 40, -60, "**When Event(telegram_message)** → **AI Switch** (roteia pelo sentido) → tirar foto / responder → **Reply**.\n\nLigue `text` (do When Event) no AI Switch e `chat_id` nos Reply. Precisa de `ANTHROPIC_API_KEY`.");
    let onev = g.add_node(NodeKind::OnEvent, 40, 160);
    set_cfg(&mut g, onev, json!({ "event": "telegram_message" }));
    let aisw = g.add_node(NodeKind::AiSwitch, 300, 160);
    set_cfg(
        &mut g,
        aisw,
        json!({ "arms": ["tirar foto", "responder pergunta"], "timeout_secs": "30" }),
    );
    let snap = g.add_node(NodeKind::CameraSnapshot, 560, 60);
    set_cfg(&mut g, snap, json!({ "camera": "first" }));
    let reply = g.add_node(NodeKind::ReplyToSender, 820, 60);
    set_cfg(&mut g, reply, json!({ "message": "Aqui está" }));
    let ans = g.add_node(NodeKind::AiAnswer, 560, 260);
    set_cfg(&mut g, ans, json!({ "prompt": "", "timeout_secs": "30" }));
    let reply2 = g.add_node(NodeKind::ReplyToSender, 820, 260);
    set_cfg(&mut g, reply2, json!({ "message": "" }));
    ex(&mut g, onev, "exec_out", aisw);
    wire(&mut g, aisw, "tirar foto", snap, "exec_in");
    ex(&mut g, snap, "exec_out", reply);
    wire(&mut g, snap, "image", reply, "image");
    wire(&mut g, aisw, "responder pergunta", ans, "exec_in");
    ex(&mut g, ans, "exec_out", reply2);
    wire(&mut g, ans, "response", reply2, "message");
    g
}

// ---- L4 — State & data ------------------------------------------------------

/// #7 — Schedule (daily) → count people → Format → Notify.
fn relatorio_diario() -> Graph {
    let mut g = template(
        0x08,
        "Relatório diário de ocupação",
        "Todo dia num horário, conta as pessoas e te manda um resumo.",
    );
    title(&mut g, 40, -120, "Relatório diário");
    note(&mut g, 40, -60, "**When Event (agenda, diário 08:00)** → frame → conta pessoas → **Format** monta o texto → **Notify**.\n\nO horário fica no próprio nó — abra-o pra mudar hora, dias ou fuso.\n\nExtensão: grave o número numa **Dictionary Table** com Add Row pra ter histórico.");
    // The schedule that used to be the GRAPH's trigger, now on the node that owns it. `payload: []`
    // matters: a schedule fire carries no data, so the node must not wear an event's output ports.
    let onev = g.add_node(NodeKind::OnEvent, -220, 120);
    set_cfg(
        &mut g,
        onev,
        json!({
            "source": "cron",
            "event": "",
            "payload": [],
            "schedule": {
                "freq": "daily",
                "tz": "America/Sao_Paulo",
                "times": ["08:00"],
                "minute": 0,
                "weekdays": [],
                "day_of_month": 1,
                "month": 1,
            },
        }),
    );
    let frame = g.add_node(NodeKind::GetCurrentFrame, 40, 120);
    set_cfg(
        &mut g,
        frame,
        json!({ "camera": "first", "max_age_ms": "2000" }),
    );
    let det = g.add_node(NodeKind::PersonDetection, 300, 120);
    let ntt = g.add_node(NodeKind::NumToText, 300, 300); // Num → Text so Format can use it
    let fmt = g.add_node(NodeKind::Format, 560, 120);
    set_cfg(
        &mut g,
        fmt,
        json!({ "template": "Ocupação agora: {p1} pessoa(s).", "p1": "", "p2": "", "p3": "" }),
    );
    let notify = g.add_node(NodeKind::NotifyRecipient, 820, 120);
    set_cfg(&mut g, notify, json!({ "message": "" }));
    ex(&mut g, frame, "exec_out", det);
    ex(&mut g, det, "exec_out", notify);
    wire(&mut g, det, "count", ntt, "value");
    wire(&mut g, ntt, "text", fmt, "p1");
    wire(&mut g, fmt, "text", notify, "message");
    ex(&mut g, onev, "exec_out", frame);
    g
}

/// #8 — Region (line) + On line_crossed → alert.
fn cruzamento_linha() -> Graph {
    let mut g = template(
        0x09,
        "Cruzamento de linha (perímetro)",
        "Desenhe uma linha; quando algo cruza, dispara um alerta.",
    );
    title(&mut g, 40, -120, "Cruzamento de linha");
    note(&mut g, 40, -60, "**Region** (desenhe a linha) reutilizada em **When Event(line_crossed)** → **Notify**.\n\nDepois de clonar, desenhe a linha no nó Region e ligue-o ao When Event (campo região).");
    let region = g.add_node(NodeKind::Region, 40, 300);
    set_cfg(
        &mut g,
        region,
        json!({ "camera": "first", "name": "linha", "points": [] }),
    );
    let onev = g.add_node(NodeKind::OnEvent, 40, 120);
    set_cfg(&mut g, onev, json!({ "event": "line_crossed" }));
    let notify = g.add_node(NodeKind::NotifyRecipient, 320, 120);
    set_cfg(
        &mut g,
        notify,
        json!({ "message": "Cruzamento de linha detectado" }),
    );
    let _ = region;
    ex(&mut g, onev, "exec_out", notify);
    g
}

// ---- L5 — Faces & smart home ------------------------------------------------

/// #9 — On face_detected → embed → recognize → If(found) → Notify / Store (unknown).
fn reconhece_rosto() -> Graph {
    let mut g = template(
        0x0A,
        "Reconhecimento de rostos",
        "Ao detectar um rosto, embeda, compara com a galeria e avisa — ou guarda o desconhecido.",
    );
    title(&mut g, 40, -140, "Reconhecimento de rostos");
    note(&mut g, 40, -80, "**When Event(face_detected)** → **Extract Face Embedding** → **Recognize Face** → **If(found)** → **Notify** · senão **Store Face** (desconhecido).\n\nLigue o frame (do evento) no Extract Face Embedding. Precisa do serviço de IA.");
    let onev = g.add_node(NodeKind::OnEvent, 40, 160);
    set_cfg(&mut g, onev, json!({ "event": "face_detected" }));
    let emb = g.add_node(NodeKind::FaceEmbedding, 300, 160);
    let rec = g.add_node(NodeKind::RecognizeFace, 560, 160);
    let iff = g.add_node(NodeKind::If, 820, 160);
    let notify = g.add_node(NodeKind::NotifyRecipient, 1080, 60);
    set_cfg(
        &mut g,
        notify,
        json!({ "message": "Rosto conhecido reconhecido" }),
    );
    let store = g.add_node(NodeKind::StoreFace, 1080, 260);
    set_cfg(
        &mut g,
        store,
        json!({ "source": "template", "dedup_distance": "0.15" }),
    );
    ex(&mut g, onev, "exec_out", emb);
    ex(&mut g, emb, "exec_out", rec);
    ex(&mut g, rec, "exec_out", iff);
    wire(&mut g, emb, "embedding", rec, "embedding");
    wire(&mut g, rec, "found", iff, "condition");
    wire(&mut g, iff, "true", notify, "exec_in");
    wire(&mut g, iff, "false", store, "exec_in");
    wire(&mut g, emb, "embedding", store, "embedding");
    g
}

/// #10 — On zone entered/exited → Tuya control (light on/off).
fn luz_presenca() -> Graph {
    let mut g = template(
        0x0B,
        "Luz por presença (casa inteligente)",
        "Entrou na zona → acende a luz; saiu → apaga. Visão vira automação física.",
    );
    title(&mut g, 40, -120, "Luz por presença");
    note(&mut g, 40, -60, "**When Event(object_entered)** → **Control Tuya** (liga) · **When Event(object_exited)** → **Control Tuya** (desliga).\n\nEscolha o dispositivo e o data point Tuya em cada nó (Integrations → Tuya).");
    let onin = g.add_node(NodeKind::OnEvent, 40, 120);
    set_cfg(&mut g, onin, json!({ "event": "object_entered" }));
    let ton = g.add_node(NodeKind::TuyaControl, 320, 120);
    set_cfg(
        &mut g,
        ton,
        json!({ "tuya_device": "", "tuya_code": "", "value": "true", "value_type": "bool", "timeout_secs": "15" }),
    );
    let onout = g.add_node(NodeKind::OnEvent, 40, 300);
    set_cfg(&mut g, onout, json!({ "event": "object_exited" }));
    let toff = g.add_node(NodeKind::TuyaControl, 320, 300);
    set_cfg(
        &mut g,
        toff,
        json!({ "tuya_device": "", "tuya_code": "", "value": "false", "value_type": "bool", "timeout_secs": "15" }),
    );
    ex(&mut g, onin, "exec_out", ton);
    ex(&mut g, onout, "exec_out", toff);
    g
}

// ---- L6 — Map & presence ----------------------------------------------------

/// #11 — Cron → detect → Record presence (map pulse).
fn presenca_mapa() -> Graph {
    let mut g = template(
        0x0C,
        "Presença viva no Mapa",
        "Detecta pessoas e registra a presença — o pulso categorizado no Mapa.",
    );
    title(&mut g, 40, -120, "Presença viva no Mapa");
    note(&mut g, 40, -60, "**When Event(motion)** → frame → detecta pessoas → **Record presence** (categoria `person`).\n\nAbra o **Mapa** pra ver o pulso ao vivo. Escolha a câmera/sala.");
    let onev = g.add_node(NodeKind::OnEvent, -220, 120);
    set_cfg(&mut g, onev, json!({ "event": "motion" }));
    let frame = g.add_node(NodeKind::GetCurrentFrame, 40, 120);
    set_cfg(
        &mut g,
        frame,
        json!({ "camera": "first", "max_age_ms": "2000" }),
    );
    let det = g.add_node(NodeKind::PersonDetection, 300, 120);
    let pres = g.add_node(NodeKind::RecordPresence, 560, 120);
    set_cfg(
        &mut g,
        pres,
        json!({ "category": "person", "label": "", "duration_secs": 8 }),
    );
    ex(&mut g, onev, "exec_out", frame);
    ex(&mut g, frame, "exec_out", det);
    ex(&mut g, det, "exec_out", pres);
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn templates_build_are_global_and_have_stable_unique_ids() {
        let ts = templates();
        assert!(!ts.is_empty());
        let mut ids = HashSet::new();
        for g in &ts {
            assert!(
                g.template && g.global,
                "template {} must be template+global",
                g.name
            );
            assert!(!g.nodes.is_empty(), "template {} has no nodes", g.name);
            assert!(
                ids.insert(g.id),
                "duplicate stable id in template {}",
                g.name
            );
            // Every edge must reference real nodes + valid ports (domain-validated on build).
            let node_ids: HashSet<u32> = g.nodes.iter().map(|n| n.id).collect();
            for e in &g.edges {
                assert!(
                    node_ids.contains(&e.from) && node_ids.contains(&e.to),
                    "dangling edge in {}",
                    g.name
                );
            }
        }
    }
}
