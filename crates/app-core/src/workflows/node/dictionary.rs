//! Dictionary nodes (#113) — read and mutate a named typed-column table mid-run.
//!
//! The `table` input resolves to a `PortValue::Table(Dictionary)` — the run-start snapshot (from a
//! dropdown-picked id or a wired Table variable). **Read** nodes work off that snapshot. **Mutate**
//! nodes ignore the snapshot's rows and act on the live DB row through `dictionaries::mutate`, which
//! takes a `SELECT … FOR UPDATE` lock — so concurrent runs mutating the same dictionary serialize,
//! and each mutation sees the others' writes.
use super::{PortValue, PortValues};
use crate::agent::tools::ToolContext;
use crate::dictionaries::{self, Cell, Dictionary, Mutation};

/// The resolved `table` input (a dictionary snapshot). Errors clearly when unset/unwired.
fn table<'a>(inputs: &'a PortValues, node: &str) -> Result<&'a Dictionary, String> {
    match inputs.get("table") {
        Some(PortValue::Table(d)) => Ok(d),
        _ => Err(format!("{node}: no dictionary selected (wire or pick a 'table')")),
    }
}

fn text_in(inputs: &PortValues, key: &str) -> String {
    inputs.get(key).and_then(PortValue::as_text).unwrap_or("").trim().to_string()
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
        .map(|(i, col)| (col.name.clone(), row.get(i).map(Cell::as_text).unwrap_or_default()))
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
    match usize::try_from(ri).ok().and_then(|r| dict.rows.get(r)).and_then(|row| row.get(ci)) {
        Some(cell) => Ok(out(vec![
            ("value", PortValue::Text(cell.as_text())),
            ("found", PortValue::Bool(true)),
        ])),
        None => Ok(out(vec![("found", PortValue::Bool(false))])),
    }
}

// ---- Mutate nodes (atomic; hit the live DB row under a lock) ----------------------------------

fn values_in(inputs: &PortValues) -> Vec<(String, String)> {
    match inputs.get("values") {
        Some(PortValue::Dict(pairs)) => pairs.clone(),
        _ => Vec::new(),
    }
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
    let row = usize::try_from(num_in(inputs, "row").unwrap_or(0)).map_err(|_| "row out of range")?;
    let value = Cell::Text(text_in(inputs, "value"));
    dictionaries::mutate(&sys.db, sys.tenant_id, dict.id, Mutation::SetCell { row, col, value })?;
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
        Mutation::UpdateRowByKey { key, col: Some(col), row },
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
        Mutation::DeleteRowByKey { key, col: Some(col) },
    )?;
    Ok(PortValues::new())
}

pub(super) fn run_clear(inputs: &PortValues, sys: &ToolContext) -> Result<PortValues, String> {
    let dict = table(inputs, "Dictionary Clear")?;
    dictionaries::mutate(&sys.db, sys.tenant_id, dict.id, Mutation::Clear)?;
    Ok(PortValues::new())
}
