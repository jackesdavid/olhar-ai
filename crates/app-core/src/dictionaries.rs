//! Dictionaries (#113) — named, tenant-scoped, **typed-column tables**. The structured cousin of
//! the flat string→string dictionary (HTTP Post headers/fields): a dictionary has a schema
//! (`columns`: ordered `{name, ty}` where `ty ∈ Text|Num|Bool`) and `rows` (each an ordered list
//! of cells, positionally typed by the schema).
//!
//! First-class + reusable: a workflow variable of `types::TABLE` references a dictionary by id,
//! so an edit here reaches every workflow reading it. Nodes read and **mutate** rows mid-run; the
//! mutators are made atomic + thread-safe across concurrent runs/pods by taking a
//! `SELECT … FOR UPDATE` lock on the dictionary's single row before rewriting `rows`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::Db;
use crate::timeutil::now_rfc3339;

/// A cell's scalar type. Deliberately small for v1 (Text/Num/Bool) — richer cell types
/// A column's cell type. Text/Num/Bool are plain scalars; `Camera`/`Recipient` are
/// entity references (a camera device id, a recipient label) — string-backed on the wire, but
/// authored via a dropdown. Add new reference types here + in `Cell` + the frontend editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellType {
    Text,
    Num,
    Bool,
    Camera,
    Recipient,
}

impl CellType {
    /// The zero value for this column type — used to backfill new columns onto existing rows.
    pub fn zero(self) -> Cell {
        match self {
            CellType::Text => Cell::Text(String::new()),
            CellType::Num => Cell::Num(0),
            CellType::Bool => Cell::Bool(false),
            CellType::Camera => Cell::Camera(String::new()),
            CellType::Recipient => Cell::Recipient(String::new()),
        }
    }
    /// Coerce an arbitrary cell to this column's type (used when a column is retyped).
    pub fn coerce(self, c: &Cell) -> Cell {
        match self {
            CellType::Text => Cell::Text(c.as_text()),
            CellType::Num => Cell::Num(c.as_num().unwrap_or(0)),
            CellType::Bool => Cell::Bool(c.as_bool()),
            CellType::Camera => Cell::Camera(c.as_text()),
            CellType::Recipient => Cell::Recipient(c.as_text()),
        }
    }
}

/// One column of the schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: CellType,
}

/// One typed cell value. Serialized tagged (`{"t":"num","v":3}`) so a row survives a column
/// retype/reorder round-trip without the schema; the schema stays the source of truth for display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum Cell {
    Text(String),
    Num(i64),
    Bool(bool),
    /// A camera device id (the same stable id `PortValue::Camera` carries).
    Camera(String),
    /// A recipient label (the same handle `PortValue::Recipient` carries).
    Recipient(String),
}

impl Cell {
    pub fn as_text(&self) -> String {
        match self {
            Cell::Text(s) | Cell::Camera(s) | Cell::Recipient(s) => s.clone(),
            Cell::Num(n) => n.to_string(),
            Cell::Bool(b) => b.to_string(),
        }
    }
    pub fn as_num(&self) -> Option<i64> {
        match self {
            Cell::Num(n) => Some(*n),
            Cell::Text(s) | Cell::Camera(s) | Cell::Recipient(s) => s.trim().parse().ok(),
            Cell::Bool(b) => Some(*b as i64),
        }
    }
    pub fn as_bool(&self) -> bool {
        match self {
            Cell::Bool(b) => *b,
            Cell::Text(s) | Cell::Camera(s) | Cell::Recipient(s) => {
                matches!(s.trim(), "true" | "1" | "yes" | "on")
            }
            Cell::Num(n) => *n != 0,
        }
    }
    /// Compare a cell against a raw string key for `find`/`update`/`delete by key` — string-wise,
    /// which matches on any column type without the caller knowing the type.
    pub fn matches_key(&self, key: &str) -> bool {
        self.as_text() == key
    }
}

/// A full dictionary as stored + returned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dictionary {
    pub id: Uuid,
    pub name: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Cell>>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

impl Dictionary {
    /// Index of a column by name (case-insensitive), for `find`/`get cell` by column name.
    pub fn col_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }
    /// Normalize a row to the schema width, coercing/zero-filling per column type.
    fn conform_row(&self, mut row: Vec<Cell>) -> Vec<Cell> {
        row.resize_with(self.columns.len(), || Cell::Text(String::new()));
        row.iter()
            .zip(&self.columns)
            .map(|(cell, col)| col.ty.coerce(cell))
            .collect()
    }
}

// ---- Persistence -----------------------------------------------------------------------------

type Row = (
    Uuid,
    String,
    serde_json::Value,
    serde_json::Value,
    String,
    String,
);

fn from_row(r: Row) -> Dictionary {
    Dictionary {
        id: r.0,
        name: r.1,
        columns: serde_json::from_value(r.2).unwrap_or_default(),
        rows: serde_json::from_value(r.3).unwrap_or_default(),
        created_at: r.4,
        updated_at: r.5,
    }
}

const SELECT: &str = "SELECT id, name, columns, rows, created_at, updated_at FROM dictionaries";

/// All dictionaries for a tenant (schema + rows), newest first.
pub fn list(db: &Db, tenant_id: Uuid) -> Vec<Dictionary> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, Row>(&format!(
            "{SELECT} WHERE tenant_id = $1 ORDER BY updated_at DESC"
        ))
        .bind(tenant_id)
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(from_row)
        .collect()
    })
}

/// One dictionary by id (tenant-scoped).
pub fn get(db: &Db, tenant_id: Uuid, id: Uuid) -> Option<Dictionary> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query_as::<_, Row>(&format!("{SELECT} WHERE tenant_id = $1 AND id = $2"))
            .bind(tenant_id)
            .bind(id)
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten()
            .map(from_row)
    })
}

/// Create a dictionary with an initial schema + rows (rows conformed to the columns). Returns the
/// new id, or an error string (e.g. duplicate name).
pub fn create(
    db: &Db,
    tenant_id: Uuid,
    name: &str,
    columns: &[Column],
    rows: &[Vec<Cell>],
) -> Result<Uuid, String> {
    let pool = db.pool();
    let id = Uuid::new_v4();
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Name is required".into());
    }
    let scaffold = Dictionary {
        id,
        name: name.clone(),
        columns: columns.to_vec(),
        rows: rows.to_vec(),
        created_at: String::new(),
        updated_at: String::new(),
    };
    let conformed: Vec<Vec<Cell>> = scaffold
        .rows
        .iter()
        .cloned()
        .map(|r| scaffold.conform_row(r))
        .collect();
    let cols = serde_json::to_value(columns).unwrap_or_else(|_| serde_json::json!([]));
    let rows_json = serde_json::to_value(&conformed).unwrap_or_else(|_| serde_json::json!([]));
    let now = now_rfc3339();
    db.block(async move {
        sqlx::query(
            "INSERT INTO dictionaries (id, tenant_id, name, columns, rows, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $6)",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(&name)
        .bind(cols)
        .bind(rows_json)
        .bind(&now)
        .execute(&pool)
        .await
        .map(|_| id)
        .map_err(|e| {
            if e.to_string().contains("dictionaries_tenant_name_uidx") {
                format!("A dictionary named “{name}” already exists")
            } else {
                e.to_string()
            }
        })
    })
}

/// Replace a dictionary's full content — name, schema, and rows (rows conformed to the new
/// columns: coerce/zero-fill/truncate). This is the table editor's save. Atomic under a row lock
/// so a concurrent node mutation can't interleave with the rewrite.
pub fn update(
    db: &Db,
    tenant_id: Uuid,
    id: Uuid,
    name: &str,
    columns: &[Column],
    rows: &[Vec<Cell>],
) -> Result<Dictionary, String> {
    let pool = db.pool();
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Name is required".into());
    }
    let columns = columns.to_vec();
    let rows = rows.to_vec();
    db.block(async move {
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        let mut dict = lock_for_update(&mut tx, tenant_id, id).await?;
        dict.name = name;
        dict.columns = columns;
        let conformed: Vec<Vec<Cell>> = rows.iter().cloned().map(|r| dict.conform_row(r)).collect();
        dict.rows = conformed;
        write_back(&mut tx, tenant_id, &dict).await.map_err(|e| {
            if e.contains("dictionaries_tenant_name_uidx") {
                format!("A dictionary named “{}” already exists", dict.name)
            } else {
                e
            }
        })?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(dict)
    })
}

/// Delete a dictionary. The in-use guard lives in the caller (control), which knows the workflows.
pub fn delete(db: &Db, tenant_id: Uuid, id: Uuid) -> Result<(), String> {
    let pool = db.pool();
    db.block(async move {
        sqlx::query("DELETE FROM dictionaries WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id)
            .bind(id)
            .execute(&pool)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}

// ---- Atomic row mutators (used by nodes; each locks the row) ---------------------------------

/// A mutation over a locked dictionary. Returned to the node so it can react (e.g. the new count).
pub enum Mutation {
    AddRow(Vec<Cell>),
    /// (match key, key column index or None→first column, new full row)
    UpdateRowByKey {
        key: String,
        col: Option<usize>,
        row: Vec<Cell>,
    },
    DeleteRowByKey {
        key: String,
        col: Option<usize>,
    },
    DeleteRowAt(usize),
    SetCell {
        row: usize,
        col: usize,
        value: Cell,
    },
    Clear,
}

/// Apply a mutation atomically: lock the dictionary's row, mutate `rows`, write back. Concurrent
/// mutators to the same dictionary serialize on the `FOR UPDATE` lock. Returns the fresh dictionary.
pub fn mutate(db: &Db, tenant_id: Uuid, id: Uuid, m: Mutation) -> Result<Dictionary, String> {
    let pool = db.pool();
    db.block(async move {
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        let mut dict = lock_for_update(&mut tx, tenant_id, id).await?;
        apply(&mut dict, m)?;
        write_back(&mut tx, tenant_id, &dict).await?;
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(dict)
    })
}

/// Pure, testable core of `mutate` — apply one mutation to an in-memory dictionary.
pub fn apply(dict: &mut Dictionary, m: Mutation) -> Result<(), String> {
    match m {
        Mutation::AddRow(row) => {
            let row = dict.conform_row(row);
            dict.rows.push(row);
        }
        Mutation::UpdateRowByKey { key, col, row } => {
            let idx = find_by_key(dict, &key, col).ok_or("No row matched the key")?;
            dict.rows[idx] = dict.conform_row(row);
        }
        Mutation::DeleteRowByKey { key, col } => {
            let idx = find_by_key(dict, &key, col).ok_or("No row matched the key")?;
            dict.rows.remove(idx);
        }
        Mutation::DeleteRowAt(idx) => {
            if idx >= dict.rows.len() {
                return Err("Row index out of range".into());
            }
            dict.rows.remove(idx);
        }
        Mutation::SetCell { row, col, value } => {
            let cell = dict
                .rows
                .get_mut(row)
                .and_then(|r| r.get_mut(col))
                .ok_or("Cell out of range")?;
            let ty = dict
                .columns
                .get(col)
                .map(|c| c.ty)
                .unwrap_or(CellType::Text);
            *cell = ty.coerce(&value);
        }
        Mutation::Clear => dict.rows.clear(),
    }
    Ok(())
}

/// First row index whose key-column cell matches `key` (string-wise). `col=None` → first column.
pub fn find_by_key(dict: &Dictionary, key: &str, col: Option<usize>) -> Option<usize> {
    let c = col.unwrap_or(0);
    dict.rows
        .iter()
        .position(|r| r.get(c).is_some_and(|cell| cell.matches_key(key)))
}

async fn lock_for_update(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Dictionary, String> {
    sqlx::query_as::<_, Row>(&format!(
        "{SELECT} WHERE tenant_id = $1 AND id = $2 FOR UPDATE"
    ))
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| e.to_string())?
    .map(from_row)
    .ok_or_else(|| "Dictionary not found".to_string())
}

async fn write_back(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    dict: &Dictionary,
) -> Result<(), String> {
    let cols = serde_json::to_value(&dict.columns).unwrap_or_else(|_| serde_json::json!([]));
    let rows = serde_json::to_value(&dict.rows).unwrap_or_else(|_| serde_json::json!([]));
    sqlx::query(
        "UPDATE dictionaries SET name = $3, columns = $4, rows = $5, updated_at = $6 \
         WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id)
    .bind(dict.id)
    .bind(&dict.name)
    .bind(cols)
    .bind(rows)
    .bind(now_rfc3339())
    .execute(&mut **tx)
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Dictionary {
        Dictionary {
            id: Uuid::nil(),
            name: "hours".into(),
            columns: vec![
                Column {
                    name: "day".into(),
                    ty: CellType::Text,
                },
                Column {
                    name: "open".into(),
                    ty: CellType::Text,
                },
                Column {
                    name: "h24".into(),
                    ty: CellType::Bool,
                },
            ],
            rows: vec![
                vec![
                    Cell::Text("Mon".into()),
                    Cell::Text("09:00".into()),
                    Cell::Bool(false),
                ],
                vec![
                    Cell::Text("Sat".into()),
                    Cell::Text("10:00".into()),
                    Cell::Bool(false),
                ],
            ],
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn add_row_conforms_to_schema() {
        let mut d = sample();
        // short + wrongly-typed row → zero-filled + coerced
        apply(
            &mut d,
            Mutation::AddRow(vec![Cell::Text("Sun".into()), Cell::Num(1)]),
        )
        .unwrap();
        assert_eq!(d.rows.len(), 3);
        assert_eq!(d.rows[2].len(), 3);
        assert_eq!(d.rows[2][2], Cell::Bool(false)); // zero-filled bool
    }

    #[test]
    fn update_and_delete_by_key() {
        let mut d = sample();
        apply(
            &mut d,
            Mutation::UpdateRowByKey {
                key: "Sat".into(),
                col: None,
                row: vec![
                    Cell::Text("Sat".into()),
                    Cell::Text("11:00".into()),
                    Cell::Bool(true),
                ],
            },
        )
        .unwrap();
        assert_eq!(d.rows[1][1], Cell::Text("11:00".into()));
        apply(
            &mut d,
            Mutation::DeleteRowByKey {
                key: "Mon".into(),
                col: None,
            },
        )
        .unwrap();
        assert_eq!(d.rows.len(), 1);
        assert_eq!(d.rows[0][0], Cell::Text("Sat".into()));
    }

    #[test]
    fn retype_column_coerces_cells() {
        let mut d = sample();
        d.columns[2].ty = CellType::Text;
        let conformed: Vec<Vec<Cell>> = d.rows.iter().cloned().map(|r| d.conform_row(r)).collect();
        d.rows = conformed;
        assert_eq!(d.rows[0][2], Cell::Text("false".into()));
    }
}
