//! Card #157 — `PREPARE` every sqlx statement against a real, migrated schema.
//!
//! WHY THIS EXISTS. app-core is built WITHOUT sqlx's `query!` macro feature (deliberately — the
//! build must not need a live database), so all 320 `sqlx::query*` call sites are plain strings the
//! compiler never reads. On 2026-08-01 a removed bind in an `UPDATE` shifted every later
//! placeholder one argument right: `gstreamer_ok` (boolean) received `platform` (text), Postgres
//! refused the COALESCE, and every box token lookup failed. `cargo check` passed. All 296 tests
//! passed. The edge could not authenticate until a fix shipped.
//!
//! Two independent failures are checked here, because `PREPARE` alone only catches the first:
//!
//!   1. The statement is wrong against the schema — renamed column, type mismatch, bad SQL.
//!   2. The Rust side disagrees with the statement — N `.bind()` calls for M placeholders.
//!      Postgres never sees this one; it is the exact shape of the outage above.
//!
//! SKIPPED unless `TEST_DATABASE_URL` is set, so `cargo test` stays DB-free like `cargo check`.
//! CI sets it (`check:sql` in .gitlab-ci.yml) against the real `sentinel-postgres` image — stock
//! Postgres cannot run the migrations, which need pg_cron, vector, citext and pgcrypto.
//!
//! Locally:
//!   TEST_DATABASE_URL=postgres://sentinel:sentinel@127.0.0.1:5432/sentinel \
//!     cargo test -p app-core --test sql_prepare -- --nocapture

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Crates whose sources are scanned. `sentinel-edge` is absent on purpose: it has no database.
const SCAN_DIRS: &[&str] = &[
    "crates",
    "sentinel-control",
    "sentinel-flow",
    "sentinel-gateway",
    "sentinel-notification",
];

/// Leading keywords `PREPARE` accepts. Anything else (LISTEN, SET, TRUNCATE, DDL) is reported as
/// skipped rather than failed — Postgres rejects it for what it is, not for being wrong.
const PREPARABLE: &[&str] = &["SELECT", "INSERT", "UPDATE", "DELETE", "WITH", "VALUES", "MERGE"];

/// Chain methods that may sit between the SQL literal and the executor without ending the bind
/// chain. Anything not listed here (and not an executor) makes the chain "unparsed": the statement
/// is still PREPAREd, only the bind-count comparison is skipped.
const CHAIN_PASSTHROUGH: &[&str] = &["persistent"];

/// Methods that end a bind chain.
const CHAIN_TERMINATORS: &[&str] = &[
    "fetch_one",
    "fetch_optional",
    "fetch_all",
    "fetch_many",
    "fetch",
    "execute",
    "execute_many",
    "map",
    "try_map",
];

#[derive(Debug)]
struct Site {
    file: String,
    line: usize,
    sql: String,
    /// `None` when the call chain did not parse as a straight-line `.bind()…executor` chain.
    binds: Option<usize>,
}

#[derive(Debug)]
struct DynamicSite {
    file: String,
    line: usize,
}

#[test]
fn every_sqlx_statement_prepares() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!(
            "sql_prepare: SKIPPED — set TEST_DATABASE_URL to a sentinel-postgres instance to run it."
        );
        return;
    };

    let root = repo_root();
    let mut files = Vec::new();
    for dir in SCAN_DIRS {
        collect_rs(&root.join(dir), &mut files);
    }
    assert!(
        !files.is_empty(),
        "sql_prepare: found no .rs files under {} — the scan roots are wrong",
        root.display()
    );

    let mut sites = Vec::new();
    let mut dynamic = Vec::new();
    for path in &files {
        let src = std::fs::read_to_string(path).expect("read source");
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string();
        scan_file(&rel, &src, &mut sites, &mut dynamic);
    }

    assert!(
        sites.len() > 100,
        "sql_prepare: only {} statements extracted — the scanner is broken, not the codebase",
        sites.len()
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async move { run_checks(&url, sites, dynamic).await });
}

async fn run_checks(url: &str, sites: Vec<Site>, dynamic: Vec<DynamicSite>) {
    use sqlx::{Connection, PgConnection, Row};

    let mut conn = PgConnection::connect(url)
        .await
        .expect("connect to TEST_DATABASE_URL");

    // The schema under test is the migrations themselves, never a dump that can drift from them.
    sqlx::migrate!("./migrations")
        .run(&mut conn)
        .await
        .expect("run migrations");

    let mut failures: Vec<String> = Vec::new();
    let mut not_preparable = 0usize;
    let mut unparsed_chain = 0usize;
    let mut checked = 0usize;
    let mut bind_checked = 0usize;

    for (i, site) in sites.iter().enumerate() {
        let sql = site.sql.trim().trim_end_matches(';').trim();
        let first = sql
            .split(|c: char| !c.is_ascii_alphabetic())
            .find(|w| !w.is_empty())
            .unwrap_or("")
            .to_ascii_uppercase();
        if !PREPARABLE.contains(&first.as_str()) {
            not_preparable += 1;
            continue;
        }

        let name = format!("sqlcheck_{i}");
        // No transaction: a failed PREPARE must not poison the checks that follow it, so every
        // statement is reported in one run instead of the first one aborting the rest.
        if let Err(e) = sqlx::query(&format!("PREPARE {name} AS {sql}"))
            .execute(&mut conn)
            .await
        {
            failures.push(format!(
                "PREPARE failed  {}:{}\n    {}\n    {}",
                site.file,
                site.line,
                one_line(&e.to_string()),
                snippet(sql)
            ));
            continue;
        }
        checked += 1;

        let params: i32 = sqlx::query(
            "SELECT coalesce(array_length(parameter_types, 1), 0) FROM pg_prepared_statements WHERE name = $1",
        )
        .bind(&name)
        .fetch_one(&mut conn)
        .await
        .expect("read pg_prepared_statements")
        .get(0);

        match site.binds {
            None => unparsed_chain += 1,
            Some(binds) => {
                bind_checked += 1;
                if binds != params as usize {
                    failures.push(format!(
                        "bind mismatch   {}:{}\n    statement takes {} parameter(s), the call site binds {}\n    {}",
                        site.file, site.line, params, binds, snippet(sql)
                    ));
                }
            }
        }

        let _ = sqlx::query(&format!("DEALLOCATE {name}"))
            .execute(&mut conn)
            .await;
    }

    let mut report = String::new();
    let _ = writeln!(
        report,
        "\nsql_prepare: {} statements extracted, {checked} prepared, {bind_checked} bind-count \
         checked, {not_preparable} not preparable (LISTEN/SET/DDL), {unparsed_chain} with an \
         unparsed bind chain.",
        sites.len()
    );
    // Never silently drop coverage: what this check CANNOT see is as load-bearing as what it can.
    let _ = writeln!(
        report,
        "sql_prepare: {} site(s) build their SQL at runtime and are NOT covered:",
        dynamic.len()
    );
    for d in &dynamic {
        let _ = writeln!(report, "    {}:{}", d.file, d.line);
    }
    eprintln!("{report}");

    // A checker that skips everything passes everything. The first version of this test did exactly
    // that — the bind chain never parsed, all 298 comparisons were skipped, and it reported ok.
    // These floors make that state a failure instead of a green tick.
    assert!(
        checked > 250,
        "sql_prepare: only {checked} statements reached PREPARE — the scanner regressed"
    );
    assert!(
        bind_checked * 4 > checked * 3,
        "sql_prepare: only {bind_checked} of {checked} statements got a bind-count check — the \
         chain parser regressed, and the check this test exists for is not running"
    );

    assert!(
        failures.is_empty(),
        "\n{} sqlx statement(s) are wrong:\n\n{}\n",
        failures.len(),
        failures.join("\n\n")
    );
}

// ---------------------------------------------------------------------------------------------
// Source scanning
//
// Every statement in the tree is a string literal at the call site — no `format!`, no accumulated
// SQL (12 sites pass a variable; those are reported as uncovered, not parsed). That is what makes
// static extraction honest here rather than a best-effort regex.
// ---------------------------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root is two levels above crates/app-core")
        .to_path_buf()
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name == "target" || name == "node_modules" || name.starts_with('.') {
                continue;
            }
            collect_rs(&path, out);
        } else if name.ends_with(".rs") && !path.to_string_lossy().contains("/tests/") {
            // Skip test sources — this file's own `PREPARE`/`DEALLOCATE` queries are not product
            // statements, and scanning them would report the checker as uncovered by itself.
            out.push(path);
        }
    }
}

fn scan_file(rel: &str, src: &str, sites: &mut Vec<Site>, dynamic: &mut Vec<DynamicSite>) {
    let bytes: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    while let Some(found) = find_from(&bytes, i, "sqlx::query") {
        i = found + "sqlx::query".len();
        let mut p = i;
        for suffix in ["_as", "_scalar"] {
            if starts_with(&bytes, p, suffix) {
                p += suffix.len();
                break;
            }
        }
        // Optional turbofish: `::<_, Device>`.
        if starts_with(&bytes, p, "::<") {
            p += 3;
            let mut depth = 1usize;
            while p < bytes.len() && depth > 0 {
                match bytes[p] {
                    '<' => depth += 1,
                    '>' => depth -= 1,
                    _ => {}
                }
                p += 1;
            }
        }
        p = skip_trivia(&bytes, p);
        if p >= bytes.len() || bytes[p] != '(' {
            continue;
        }
        p = skip_trivia(&bytes, p + 1);

        let line = 1 + bytes[..found].iter().filter(|c| **c == '\n').count();
        match read_string_literal(&bytes, p) {
            Some((sql, after)) => {
                // Consume the `)` that closes `query(…)` (and a trailing comma if the arg list has
                // one) before the chain starts. Forgetting this made EVERY chain unparsable, and
                // the test still passed — it had skipped all 298 comparisons.
                let mut q = skip_trivia(&bytes, after);
                if q < bytes.len() && bytes[q] == ',' {
                    q = skip_trivia(&bytes, q + 1);
                }
                let binds = if q < bytes.len() && bytes[q] == ')' {
                    count_binds(&bytes, q + 1)
                } else {
                    None
                };
                sites.push(Site {
                    file: rel.to_string(),
                    line,
                    sql,
                    binds,
                });
                i = after;
            }
            None => dynamic.push(DynamicSite {
                file: rel.to_string(),
                line,
            }),
        }
    }
}

/// Reads a Rust string literal at `p` — plain `"…"` (with escapes) or raw `r#"…"#`.
/// Returns the literal's contents and the index just past it.
fn read_string_literal(b: &[char], p: usize) -> Option<(String, usize)> {
    if p >= b.len() {
        return None;
    }
    if b[p] == 'r' {
        let mut q = p + 1;
        let mut hashes = 0usize;
        while q < b.len() && b[q] == '#' {
            hashes += 1;
            q += 1;
        }
        if q >= b.len() || b[q] != '"' {
            return None;
        }
        q += 1;
        let start = q;
        let closing: String = std::iter::once('"').chain(std::iter::repeat('#').take(hashes)).collect();
        let end = find_from(b, q, &closing)?;
        return Some((b[start..end].iter().collect(), end + closing.len()));
    }
    if b[p] != '"' {
        return None;
    }
    let mut q = p + 1;
    let mut out = String::new();
    while q < b.len() {
        match b[q] {
            '\\' => {
                q += 1;
                match b.get(q) {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('\n') => {
                        // Line continuation: `\` at EOL eats the following indentation.
                        q += 1;
                        while q < b.len() && b[q].is_whitespace() {
                            q += 1;
                        }
                        continue;
                    }
                    Some(c) => out.push(*c),
                    None => return None,
                }
                q += 1;
            }
            '"' => return Some((out, q + 1)),
            c => {
                out.push(c);
                q += 1;
            }
        }
    }
    None
}

/// Counts `.bind(…)` calls between the SQL literal and the executor. `None` means the chain was not
/// a straight line (a loop, a reassignment, an unrecognised method) — the bind comparison is then
/// unsafe to make, so it is skipped rather than guessed at.
fn count_binds(b: &[char], mut p: usize) -> Option<usize> {
    let mut binds = 0usize;
    loop {
        p = skip_trivia(b, p);
        if p >= b.len() || b[p] != '.' {
            return None;
        }
        p = skip_trivia(b, p + 1);
        let start = p;
        while p < b.len() && (b[p].is_alphanumeric() || b[p] == '_') {
            p += 1;
        }
        let ident: String = b[start..p].iter().collect();
        if CHAIN_TERMINATORS.contains(&ident.as_str()) {
            return Some(binds);
        }
        let passthrough = CHAIN_PASSTHROUGH.contains(&ident.as_str());
        if ident != "bind" && !passthrough {
            return None;
        }
        if ident == "bind" {
            binds += 1;
        }
        p = skip_trivia(b, p);
        if p >= b.len() || b[p] != '(' {
            return None;
        }
        p = skip_balanced(b, p)?;
    }
}

/// Skips a balanced `(…)` starting at `p`, stepping over string literals inside it so a `)` in a
/// bound string cannot close the call early.
fn skip_balanced(b: &[char], mut p: usize) -> Option<usize> {
    let mut depth = 0usize;
    while p < b.len() {
        match b[p] {
            '(' => {
                depth += 1;
                p += 1;
            }
            ')' => {
                depth -= 1;
                p += 1;
                if depth == 0 {
                    return Some(p);
                }
            }
            '"' | 'r' if b[p] == '"' || starts_with(b, p, "r\"") || starts_with(b, p, "r#\"") => {
                let (_, after) = read_string_literal(b, p)?;
                p = after;
            }
            '\'' => {
                // A char literal or a lifetime — either way one token, and `'"'` must not open a
                // string. Step past `'x'` / `'\n'`, else past the tick.
                if b.get(p + 1) == Some(&'\\') {
                    p += 4;
                } else if b.get(p + 2) == Some(&'\'') {
                    p += 3;
                } else {
                    p += 1;
                }
            }
            _ => p += 1,
        }
    }
    None
}

fn skip_trivia(b: &[char], mut p: usize) -> usize {
    loop {
        while p < b.len() && b[p].is_whitespace() {
            p += 1;
        }
        if starts_with(b, p, "//") {
            while p < b.len() && b[p] != '\n' {
                p += 1;
            }
            continue;
        }
        if starts_with(b, p, "/*") {
            p += 2;
            while p < b.len() && !starts_with(b, p, "*/") {
                p += 1;
            }
            p = (p + 2).min(b.len());
            continue;
        }
        return p;
    }
}

fn starts_with(b: &[char], p: usize, pat: &str) -> bool {
    pat.chars().enumerate().all(|(k, c)| b.get(p + k) == Some(&c))
}

fn find_from(b: &[char], from: usize, pat: &str) -> Option<usize> {
    let n = pat.chars().count();
    if n == 0 || b.len() < n {
        return None;
    }
    (from..=b.len() - n).find(|&i| starts_with(b, i, pat))
}

fn one_line(s: &str) -> String {
    s.lines().collect::<Vec<_>>().join(" ")
}

fn snippet(sql: &str) -> String {
    let flat = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 140 {
        format!("{}…", flat.chars().take(140).collect::<String>())
    } else {
        flat
    }
}

// ---------------------------------------------------------------------------------------------
// Card #160 Phase 4 — a statement that touches a tenant-scoped table must NAME the tenant.
//
// `every_sqlx_statement_prepares` above proves each statement is valid against the schema and that
// its binds line up. Neither says anything about isolation: `SELECT * FROM devices` is, to that
// check, a perfectly healthy query. It is also every tenant's cameras.
//
// Phase 3 of the plan carries the tenant to the database per connection, which is the real fix and
// converts 330 call sites. This is the cheap half that lands first and covers everything Phase 3
// has not reached: if a statement touches a table that has a `tenant_id` column, the statement must
// mention `tenant_id` — or be listed in ALLOWED_CROSS_TENANT with a reason someone wrote down.
//
// WHAT THIS DOES NOT DO. It checks that the column is MENTIONED, not that it is used as a
// predicate: a query naming `tenant_id` only in its SELECT list passes. Tightening that means
// parsing SQL properly, and a crude gate running today beats a correct one that never lands. The
// failure it does catch — a tenant table queried with no notion of a tenant anywhere — is the one
// that leaks data.

/// The RATCHET, not an allow-list.
///
/// 60 statements touch a tenant-scoped table without naming `tenant_id`. Demanding zero today would
/// mean writing 60 justifications in one sitting, which is rubber-stamping rather than review — and
/// a file full of unread reasons is worse than an honest number.
///
/// So this records where the debt is, per file, and the test enforces two directions:
///
///   * a file may not EXCEED its number — a new unscoped statement fails CI;
///   * a file may not fall BELOW it without the number being lowered — an improvement that is not
///     recorded can silently be undone later.
///
/// Per FILE rather than per line, because line numbers churn on every edit and a baseline nobody
/// can keep current gets deleted.
///
/// Most of these are not leaks. The dominant shape is `WHERE id = $1` — a row addressed by its own
/// unguessable UUID, already resolved by a tenant-scoped query upstream. That is safe by
/// unguessability, not safe by construction, and closing exactly that gap is what RLS (Phase 2) is
/// for. A few are legitimately cross-tenant: the queue consumers, the master console's
/// `COUNT(*) FROM telegram_config`, and invite acceptance by token (the person has no tenant yet).
///
/// The number goes DOWN as Phase 3 converts call sites. It should never go up.
const TENANT_PREDICATE_BASELINE: &[(&str, usize)] = &[
    ("crates/app-core/src/billing/invoices.rs", 1),
    ("crates/app-core/src/billing/payments/mod.rs", 2),
    ("crates/app-core/src/edge_commands.rs", 8),
    ("crates/app-core/src/event_queue.rs", 3),
    ("crates/app-core/src/invites.rs", 2),
    ("crates/app-core/src/notification/queue.rs", 6),
    ("crates/app-core/src/notification/repo.rs", 1),
    ("crates/app-core/src/work_queue.rs", 10),
    ("crates/app-core/src/workflows/device_gate.rs", 3),
    ("crates/app-core/src/workflows/repo.rs", 2),
    ("sentinel-control/src/box_signing.rs", 4),
    ("sentinel-control/src/handlers.rs", 1),
    ("sentinel-control/src/password_reset.rs", 3),
    ("sentinel-control/src/store.rs", 14),
];

#[test]
fn every_tenant_scoped_statement_names_the_tenant() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("tenant_predicate: SKIPPED — set TEST_DATABASE_URL to run it.");
        return;
    };

    let root = repo_root();
    let mut files = Vec::new();
    for dir in SCAN_DIRS {
        collect_rs(&root.join(dir), &mut files);
    }
    let mut sites = Vec::new();
    let mut dynamic = Vec::new();
    for path in &files {
        let src = std::fs::read_to_string(path).expect("read source");
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string();
        scan_file(&rel, &src, &mut sites, &mut dynamic);
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let tenant_tables = rt.block_on(async { tenant_scoped_tables(&url).await });
    assert!(
        tenant_tables.len() > 20,
        "tenant_predicate: only {} tenant-scoped tables found — the schema query is wrong, not the \
         codebase",
        tenant_tables.len()
    );

    let mut findings: Vec<(String, String)> = Vec::new();
    for site in &sites {
        let touched = tables_touched(&site.sql);
        let scoped: Vec<&String> = touched.iter().filter(|t| tenant_tables.contains(*t)).collect();
        if scoped.is_empty() {
            continue;
        }
        if site.sql.to_lowercase().contains("tenant_id") {
            continue;
        }
        findings.push((
            site.file.clone(),
            format!(
                "  {}:{} — touches {} without naming tenant_id\n      {}",
                site.file,
                site.line,
                scoped
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                snippet(&site.sql)
            ),
        ));
    }

    let mut per_file: std::collections::BTreeMap<&str, (usize, Vec<&str>)> =
        std::collections::BTreeMap::new();
    for (file, detail) in &findings {
        let e = per_file.entry(file.as_str()).or_insert((0, Vec::new()));
        e.0 += 1;
        e.1.push(detail.as_str());
    }

    eprintln!(
        "tenant_predicate: {} statements scanned, {} tenant-scoped tables, {} finding(s) across {} \
         file(s)",
        sites.len(),
        tenant_tables.len(),
        findings.len(),
        per_file.len()
    );

    let baseline: std::collections::BTreeMap<&str, usize> =
        TENANT_PREDICATE_BASELINE.iter().copied().collect();

    let mut regressions = Vec::new();
    for (file, (count, details)) in &per_file {
        let allowed = baseline.get(file).copied().unwrap_or(0);
        if *count > allowed {
            regressions.push(format!(
                "{file}: {count} unscoped statement(s), baseline {allowed}\n{}",
                details.join("\n")
            ));
        }
    }
    if !regressions.is_empty() {
        panic!(
            "tenant_predicate: a statement touching a tenant-scoped table does not name \
             tenant_id.\n\nAdd the predicate. If the query is genuinely cross-tenant, raise the \
             file's number in TENANT_PREDICATE_BASELINE and say why in the commit — the ratchet \
             only means something if going up costs an explanation.\n\n{}",
            regressions.join("\n\n")
        );
    }

    // The other direction: an improvement that is not recorded can be silently undone later.
    let mut improved = Vec::new();
    for (file, allowed) in &baseline {
        let now = per_file.get(file).map(|(c, _)| *c).unwrap_or(0);
        if now < *allowed {
            improved.push(format!("  {file}: {allowed} -> {now}"));
        }
    }
    assert!(
        improved.is_empty(),
        "tenant_predicate: fewer unscoped statements than the baseline — good. Lower these numbers \
         in TENANT_PREDICATE_BASELINE so the ratchet holds:\n{}",
        improved.join("\n")
    );
}

/// Every public table carrying a `tenant_id` column, asked of the live schema rather than listed
/// here — a hand-maintained list would drift the first time someone adds a table.
async fn tenant_scoped_tables(url: &str) -> std::collections::HashSet<String> {
    use sqlx::{Connection, PgConnection, Row};
    let mut conn = PgConnection::connect(url).await.expect("connect");
    sqlx::migrate!("src/../migrations")
        .run(&mut conn)
        .await
        .expect("migrate");
    let rows = sqlx::query(
        "SELECT table_name FROM information_schema.columns
          WHERE table_schema = 'public' AND column_name = 'tenant_id'",
    )
    .fetch_all(&mut conn)
    .await
    .expect("query tenant-scoped tables");
    rows.into_iter()
        .map(|r| r.get::<String, _>("table_name"))
        .collect()
}

/// Table names a statement references. Deliberately shallow: the identifier following FROM, JOIN,
/// UPDATE, INTO. Subqueries and CTEs are covered because those keywords appear inside them too.
fn tables_touched(sql: &str) -> Vec<String> {
    const LEAD: &[&str] = &["from", "join", "update", "into"];
    let lower = sql.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .collect();
    let mut out = Vec::new();
    for (i, w) in words.iter().enumerate() {
        if !LEAD.contains(w) {
            continue;
        }
        if let Some(next) = words.get(i + 1) {
            // `DELETE FROM only` / `INSERT INTO` guards, plus CTE self-references are harmless:
            // a name that is not a real table simply never matches the tenant-table set.
            if !next.is_empty() && !out.contains(&next.to_string()) {
                out.push(next.to_string());
            }
        }
    }
    out
}
