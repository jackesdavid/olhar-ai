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

/// Every global template, newest curriculum entry last. Built via the validated domain API.
fn templates() -> Vec<Graph> {
    vec![onboarding_tour()]
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

/// #0 — a documentation-only canvas (Title / Markdown / Comment) that teaches the editor.
/// No ports, no execution — the inaugural lesson and a showcase of the Display nodes.
fn onboarding_tour() -> Graph {
    let mut g = template(
        0x01,
        "Comece aqui — tour do editor",
        "Introdução ao editor de workflows: gatilhos, nós, portas e como rodar. Só documentação — nada executa.",
    );

    let title = g.add_node(NodeKind::Title, 40, 40);
    set_cfg(&mut g, title, json!({ "text": "Bem-vindo ao editor de workflows", "color": "#e5e7eb", "size": "xl" }));

    let intro = g.add_node(NodeKind::Markdown, 40, 150);
    set_cfg(&mut g, intro, json!({
        "text": "## O que é um workflow\n\nUm grafo de **nós** ligados por fios.\n\n- Fios de **execução** definem a ORDEM\n- Fios de **dados** passam valores tipados (imagem, texto, número, câmera…)\n- Adicione nós pelo palete (tecla `n`)\n- Ligue uma saída a uma entrada arrastando entre as bolinhas",
        "width": 430, "height": 240
    }));

    let triggers = g.add_node(NodeKind::Markdown, 510, 150);
    set_cfg(&mut g, triggers, json!({
        "text": "## Como um grafo dispara\n\n- **Manual** — só pelo Run now\n- **Cron** — a cada N segundos\n- **Agenda** — calendário no fuso\n- **On event** — reage a movimento, telegram, rosto…",
        "width": 380, "height": 200
    }));

    let tip = g.add_node(NodeKind::Comment, 40, 420);
    set_cfg(&mut g, tip, json!({
        "text": "Dica: use \"Usar template\" para duplicar este fluxo na sua conta e começar a mexer. Nada aqui executa — é só a aula.",
        "color": "#facc15", "width": 850, "height": 84
    }));

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
            assert!(g.template && g.global, "template {} must be template+global", g.name);
            assert!(!g.nodes.is_empty(), "template {} has no nodes", g.name);
            assert!(ids.insert(g.id), "duplicate stable id in template {}", g.name);
            // Every edge must reference real nodes + valid ports (domain-validated on build).
            let node_ids: HashSet<u32> = g.nodes.iter().map(|n| n.id).collect();
            for e in &g.edges {
                assert!(node_ids.contains(&e.from) && node_ids.contains(&e.to), "dangling edge in {}", g.name);
            }
        }
    }
}
