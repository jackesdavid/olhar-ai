//! Dictionary nodes (#113) — read and mutate a named typed-column table mid-run.
//!
//! The `table` input resolves to a `PortValue::Table(Dictionary)` — the run-start snapshot (from a
//! dropdown-picked id or a wired Table variable). **Read** nodes work off that snapshot. **Mutate**
//! nodes ignore the snapshot's rows and act on the live DB row through `dictionaries::mutate`, which
//! takes a `SELECT … FOR UPDATE` lock — so concurrent runs mutating the same dictionary serialize,
//! and each mutation sees the others' writes.
use serde_json::Value;
use uuid::Uuid;

use super::{PortValue, PortValues};
use crate::agent::tools::ToolContext;
use crate::dictionaries::{self, Cell, Column, Dictionary, Mutation};

/// The resolved `table` input (a dictionary snapshot). Errors clearly when unset/unwired.
fn table<'a>(inputs: &'a PortValues, node: &str) -> Result<&'a Dictionary, String> {
    match inputs.get("table") {
        Some(PortValue::Table(d)) => Ok(d),
        _ => Err(format!(
            "{node}: no dictionary selected (wire or pick a 'table')"
        )),
    }
}

fn text_in(inputs: &PortValues, key: &str) -> String {
    inputs
        .get(key)
        .and_then(PortValue::as_text)
        .unwrap_or("")
        .trim()
        .to_string()
}
fn num_in(inputs: &PortValues, key: &str) -> Option<i64> {
    inputs.get(key).and_then(PortValue::as_num)
}

/// Emit a row as a `colname → cell-text` Dictionary (consumable by Format, HTTP Post fields, …).
fn row_to_dict(dict: &Dictionary, row: &[Cell]) -> PortValue {
    let pairs = dict
        .columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            (
                col.name.clone(),
                row.get(i).map(Cell::as_text).unwrap_or_default(),
            )
        })
        .collect();
    PortValue::Dict(pairs)
}

/// Build a full row (positioned + coerced to the schema) from a `colname → value` map. Missing
/// columns get their type-zero; extra keys are ignored. Final coercion happens in `apply`.
fn cells_from_named(dict: &Dictionary, values: &[(String, String)]) -> Vec<Cell> {
    dict.columns
        .iter()
        .map(|col| {
            values
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&col.name))
                .map(|(_, v)| Cell::Text(v.clone()))
                .unwrap_or_else(|| col.ty.zero())
        })
        .collect()
}

/// A required column name → its index, or a clear error.
fn col_index(dict: &Dictionary, name: &str, node: &str) -> Result<usize, String> {
    dict.col_index(name)
        .ok_or_else(|| format!("{node}: no column named '{name}'"))
}

fn out(pairs: Vec<(&str, PortValue)>) -> PortValues {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

// ---- Read nodes (pure; work off the snapshot) ------------------------------------------------

pub(super) fn run_row_count(inputs: &PortValues) -> Result<PortValues, String> {
    let dict = table(inputs, "Dictionary Row Count")?;
    Ok(out(vec![("count", PortValue::Num(dict.rows.len() as i64))]))
}

pub(super) fn run_get_row(inputs: &PortValues) -> Result<PortValues, String> {
    let dict = table(inputs, "Dictionary Get Row")?;
    let idx = num_in(inputs, "index").unwrap_or(0);
    match usize::try_from(idx).ok().and_then(|i| dict.rows.get(i)) {
        Some(row) => Ok(out(vec![
            ("row", row_to_dict(dict, row)),
            ("found", PortValue::Bool(true)),
        ])),
        None => Ok(out(vec![("found", PortValue::Bool(false))])),
    }
}

pub(super) fn run_find_row(inputs: &PortValues) -> Result<PortValues, String> {
    let dict = table(inputs, "Dictionary Find Row")?;
    let column = text_in(inputs, "column");
    let key = text_in(inputs, "key");
    let ci = col_index(dict, &column, "Dictionary Find Row")?;
    match dictionaries::find_by_key(dict, &key, Some(ci)) {
        Some(i) => Ok(out(vec![
            ("row", row_to_dict(dict, &dict.rows[i])),
            ("index", PortValue::Num(i as i64)),
            ("found", PortValue::Bool(true)),
        ])),
        None => Ok(out(vec![("found", PortValue::Bool(false))])),
    }
}

pub(super) fn run_get_cell(inputs: &PortValues) -> Result<PortValues, String> {
    let dict = table(inputs, "Dictionary Get Cell")?;
    let column = text_in(inputs, "column");
    let ci = col_index(dict, &column, "Dictionary Get Cell")?;
    let ri = num_in(inputs, "row").unwrap_or(0);
    match usize::try_from(ri)
        .ok()
        .and_then(|r| dict.rows.get(r))
        .and_then(|row| row.get(ci))
    {
        Some(cell) => Ok(out(vec![
            ("value", PortValue::Text(cell.as_text())),
            ("found", PortValue::Bool(true)),
        ])),
        None => Ok(out(vec![("found", PortValue::Bool(false))])),
    }
}

// ---- Mutate nodes (atomic; hit the live DB row under a lock) ----------------------------------

fn dict_pairs_in(inputs: &PortValues, key: &str) -> Vec<(String, String)> {
    match inputs.get(key) {
        Some(PortValue::Dict(pairs)) => pairs.clone(),
        _ => Vec::new(),
    }
}

fn values_in(inputs: &PortValues) -> Vec<(String, String)> {
    dict_pairs_in(inputs, "values")
}

pub(super) fn run_add_row(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let dict = table(inputs, "Dictionary Add Row")?;
    let row = cells_from_named(dict, &values_in(inputs));
    dictionaries::mutate(&sys.db, sys.tenant_id, dict.id, Mutation::AddRow(row))?;
    Ok(PortValues::new())
}

pub(super) fn run_set_cell(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let dict = table(inputs, "Dictionary Set Cell")?;
    let column = text_in(inputs, "column");
    let col = col_index(dict, &column, "Dictionary Set Cell")?;
    let row =
        usize::try_from(num_in(inputs, "row").unwrap_or(0)).map_err(|_| "row out of range")?;
    let value = Cell::Text(text_in(inputs, "value"));
    dictionaries::mutate(
        &sys.db,
        sys.tenant_id,
        dict.id,
        Mutation::SetCell { row, col, value },
    )?;
    Ok(PortValues::new())
}

pub(super) fn run_update_row(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let dict = table(inputs, "Dictionary Update Row")?;
    let column = text_in(inputs, "column");
    let col = col_index(dict, &column, "Dictionary Update Row")?;
    let key = text_in(inputs, "key");
    let row = cells_from_named(dict, &values_in(inputs));
    dictionaries::mutate(
        &sys.db,
        sys.tenant_id,
        dict.id,
        Mutation::UpdateRowByKey {
            key,
            col: Some(col),
            row,
        },
    )?;
    Ok(PortValues::new())
}

pub(super) fn run_delete_row(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let dict = table(inputs, "Dictionary Delete Row")?;
    let column = text_in(inputs, "column");
    let col = col_index(dict, &column, "Dictionary Delete Row")?;
    let key = text_in(inputs, "key");
    dictionaries::mutate(
        &sys.db,
        sys.tenant_id,
        dict.id,
        Mutation::DeleteRowByKey {
            key,
            col: Some(col),
        },
    )?;
    Ok(PortValues::new())
}

pub(super) fn run_clear(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let dict = table(inputs, "Dictionary Clear")?;
    dictionaries::mutate(&sys.db, sys.tenant_id, dict.id, Mutation::Clear)?;
    Ok(PortValues::new())
}

// ---- Build nodes (pure; compose a table in-graph, no DB) --------------------------------------

/// Parse `config.columns` (`[{name, type}]`) into the schema's columns. Malformed → empty.
fn columns_from_config(config: &Value) -> Vec<Column> {
    config
        .get("columns")
        .and_then(|v| serde_json::from_value::<Vec<Column>>(v.clone()).ok())
        .unwrap_or_default()
}

/// Coerce any scalar `PortValue` (a column input) to its cell text. References carry their string
/// handle (camera id / recipient label); non-scalars stringify to empty.
fn value_to_text(v: &PortValue) -> String {
    match v {
        PortValue::Text(s) | PortValue::Recipient(s) => s.clone(),
        PortValue::Num(n) => n.to_string(),
        PortValue::Bool(b) => b.to_string(),
        PortValue::Camera(c) => c.id.clone(),
        _ => String::new(),
    }
}

/// `DictSchema` — build an inline, unsaved empty table from `config.columns` → `schema` (Table).
pub(super) fn run_schema(config: &Value) -> Result<PortValues, String> {
    let dict = Dictionary {
        id: Uuid::nil(),
        name: String::new(),
        columns: columns_from_config(config),
        rows: Vec::new(),
        created_at: String::new(),
        updated_at: String::new(),
    };
    Ok(out(vec![("schema", PortValue::Table(dict))]))
}

/// `DictMakeRow` — build a `row` (colname→value Dictionary) from one input per configured column.
/// The `schema` input is a structural dependency; the column list is baked in `config.columns`.
pub(super) fn run_make_row(config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let pairs = columns_from_config(config)
        .iter()
        .map(|col| {
            let v = inputs.get(&col.name).map(value_to_text).unwrap_or_default();
            (col.name.clone(), v)
        })
        .collect();
    Ok(out(vec![("row", PortValue::Dict(pairs))]))
}

/// `DictAppendRow` — coerce a `row` Dictionary to the `table`'s schema and append it, in memory
/// (no DB). Outputs the new `table`.
pub(super) fn run_append_row(inputs: &PortValues) -> Result<PortValues, String> {
    let dict = table(inputs, "Add Dictionary Row")?;
    let row = cells_from_named(dict, &dict_pairs_in(inputs, "row"));
    let mut next = dict.clone();
    dictionaries::apply(&mut next, Mutation::AddRow(row))
        .map_err(|e| format!("Add Dictionary Row: {e}"))?;
    Ok(out(vec![("table", PortValue::Table(next))]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionaries::CellType;
    use serde_json::json;

    fn pv(inputs: Vec<(&str, PortValue)>) -> PortValues {
        inputs
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    #[test]
    fn schema_make_append_round_trip() {
        // 1) Build an inline schema (day: Text, open: Text, h24: Bool).
        let config = json!({
            "columns": [
                { "name": "day", "type": "text" },
                { "name": "open", "type": "text" },
                { "name": "h24", "type": "bool" },
            ]
        });
        let schema_out = run_schema(&config).unwrap();
        let PortValue::Table(schema) = schema_out.get("schema").unwrap() else {
            panic!("schema is not a Table");
        };
        assert_eq!(schema.columns.len(), 3);
        assert_eq!(schema.columns[2].ty, CellType::Bool);
        assert!(schema.rows.is_empty());

        // 2) Make a row from per-column inputs (bool arrives as a real Bool value).
        let make_out = run_make_row(
            &config,
            &pv(vec![
                ("day", PortValue::Text("Mon".into())),
                ("open", PortValue::Text("09:00".into())),
                ("h24", PortValue::Bool(true)),
            ]),
        )
        .unwrap();
        let PortValue::Dict(pairs) = make_out.get("row").unwrap() else {
            panic!("row is not a Dict");
        };
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0], ("day".into(), "Mon".into()));
        assert_eq!(pairs[2], ("h24".into(), "true".into()));

        // 3) Append the row into the schema table → typed, coerced cells.
        let append_out = run_append_row(&pv(vec![
            ("table", PortValue::Table(schema.clone())),
            ("row", make_out.get("row").unwrap().clone()),
        ]))
        .unwrap();
        let PortValue::Table(table) = append_out.get("table").unwrap() else {
            panic!("table is not a Table");
        };
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][0], Cell::Text("Mon".into()));
        assert_eq!(table.rows[0][2], Cell::Bool(true)); // "true" coerced to a real Bool
    }
}
