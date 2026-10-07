use rusqlite::{Connection, types::Value};
use std::collections::HashMap;
use std::sync::Arc;

pub struct DB {
    conn: Connection,
}

/// Rows of a query, walked with `next_row` the way DynXX walks a `sqlite3_stmt`.
///
/// `rusqlite` hands rows out through a `Rows` value that borrows its `Statement`, so a
/// cursor cannot be stored next to the statement it comes from. The rows are therefore
/// materialized up front and `next_row` moves an index over them.
pub struct QueryResult {
    rows: Vec<QueryResultRow>,
    /// Index of the row `next_row` last advanced to: `None` before the first call, when
    /// the result is empty, and once iteration ran past the end.
    current: Option<usize>,
    /// Set once iteration ran past the end, so that later calls keep reporting `false`
    /// instead of restarting at the first row.
    finished: bool,
}

pub struct QueryResultRow {
    values: Vec<Value>,
    /// Shared by every row of the same result, so a row does not clone the name table.
    column_indices: Arc<HashMap<String, usize>>,
}

impl DB {
    pub fn open(path: &str) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        Ok(Self { conn })
    }

    pub fn exec(&self, sql: &str) -> Result<(), rusqlite::Error> {
        // DynXX runs `sqlite3_exec`, which accepts several statements in one call and
        // ignores the rows of statements that return them, but treats an empty string as
        // a failure.
        if sql.trim().is_empty() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        self.conn.execute_batch(sql)
    }

    pub fn query(&mut self, sql: &str) -> Result<QueryResult, rusqlite::Error> {
        let mut stmt = self.conn.prepare(sql)?;
        let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
        let column_indices = Arc::new(
            columns
                .iter()
                .enumerate()
                .map(|(index, name)| (name.clone(), index))
                .collect::<HashMap<String, usize>>(),
        );

        let mut rows = stmt.query([])?;
        let mut materialized = Vec::new();
        while let Some(row) = rows.next()? {
            let mut values = Vec::with_capacity(columns.len());
            for index in 0..columns.len() {
                values.push(row.get::<_, Value>(index)?);
            }
            materialized.push(QueryResultRow {
                values,
                column_indices: Arc::clone(&column_indices),
            });
        }

        Ok(QueryResult {
            rows: materialized,
            current: None,
            finished: false,
        })
    }
}

impl QueryResult {
    /// Advances to the next row and reports whether one is available, mirroring DynXX's
    /// `readRow`. Column values are read from the row that was just advanced to.
    pub fn next_row(&mut self) -> bool {
        if self.finished {
            return false;
        }

        let next = self.current.map_or(0, |index| index + 1);
        if next < self.rows.len() {
            self.current = Some(next);
            true
        } else {
            self.current = None;
            self.finished = true;
            false
        }
    }

    /// The row `next_row` last advanced to, if any.
    pub fn current_row(&self) -> Option<&QueryResultRow> {
        self.rows.get(self.current?)
    }
}

/// Why a column could not be read.
///
/// The readers used to return a bare `Option`, which folded four different answers into `None`: no
/// such column, no value at that index, SQL `NULL`, and a value of another SQL type. `Ok(None)` now
/// means only the last two — "there is nothing here" — and the first is an error.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ColumnError {
    /// The query did not select a column with this name.
    NoSuchColumn,
}

impl std::fmt::Display for ColumnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColumnError::NoSuchColumn => f.write_str("the query has no such column"),
        }
    }
}

impl std::error::Error for ColumnError {}

impl QueryResultRow {
    fn get_value(&self, column: &str) -> Result<Option<&Value>, ColumnError> {
        let index = self
            .column_indices
            .get(column)
            .ok_or(ColumnError::NoSuchColumn)?;
        Ok(self.values.get(*index))
    }

    /// The text of `column`. `Ok(None)` when the cell is `NULL` or holds another type, which is how
    /// this reader has always treated a value it cannot present as text.
    pub fn get_string(&self, column: &str) -> Result<Option<String>, ColumnError> {
        match self.get_value(column)? {
            Some(Value::Text(s)) => Ok(Some(s.clone())),
            _ => Ok(None),
        }
    }

    pub fn get_i64(&self, column: &str) -> Result<Option<i64>, ColumnError> {
        match self.get_value(column)? {
            Some(Value::Integer(i)) => Ok(Some(*i)),
            _ => Ok(None),
        }
    }

    pub fn get_f64(&self, column: &str) -> Result<Option<f64>, ColumnError> {
        match self.get_value(column)? {
            Some(Value::Real(f)) => Ok(Some(*f)),
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREATE_TABLE: &str = "CREATE TABLE t (id INTEGER, name TEXT, ratio REAL, note TEXT)";

    /// An in-memory database with three rows, so no test leaves a file behind.
    fn seeded_db() -> DB {
        let db = DB::open(":memory:").expect("in-memory database opens");
        db.exec(CREATE_TABLE).expect("table is created");
        db.exec(
            "INSERT INTO t (id, name, ratio, note) VALUES \
             (1, 'one', 1.5, NULL), (2, 'two', 2.5, NULL), (3, 'three', 3.5, NULL)",
        )
        .expect("rows are inserted");
        db
    }

    #[test]
    fn open_rejects_an_unusable_path() {
        assert!(DB::open("no-such-directory/db.sqlite").is_err());
    }

    #[test]
    fn exec_rejects_empty_sql_and_reports_broken_sql() {
        let db = DB::open(":memory:").expect("in-memory database opens");
        assert!(db.exec("").is_err(), "empty SQL fails, like DynXX");
        assert!(db.exec("   ").is_err());
        assert!(db.exec("CREATE TABLE t (").is_err());
    }

    #[test]
    fn exec_accepts_several_statements_in_one_call() {
        let db = DB::open(":memory:").expect("in-memory database opens");
        db.exec("CREATE TABLE t (v INTEGER); INSERT INTO t (v) VALUES (1);")
            .expect("batches run like sqlite3_exec");
    }

    #[test]
    fn query_iterates_every_row_and_stops_at_the_end() {
        let mut db = seeded_db();
        let mut result = db
            .query("SELECT id, name, ratio FROM t ORDER BY id")
            .expect("query runs");

        // A column read before the first `next_row` has no row to read from.
        assert!(result.current_row().is_none());

        for (id, name, ratio) in [(1_i64, "one", 1.5_f64), (2, "two", 2.5), (3, "three", 3.5)] {
            assert!(result.next_row(), "{name} should be reachable");
            let row = result.current_row().expect("the advanced row is current");
            assert_eq!(row.get_i64("id").expect("the column exists"), Some(id));
            assert_eq!(
                row.get_string("name")
                    .expect("the column exists")
                    .as_deref(),
                Some(name)
            );
            assert_eq!(
                row.get_f64("ratio").expect("the column exists"),
                Some(ratio)
            );
        }

        assert!(!result.next_row(), "iteration stops after the last row");
        assert!(result.current_row().is_none());
    }

    #[test]
    fn a_row_without_matching_values_reads_as_nothing() {
        let mut db = seeded_db();
        let mut result = db
            .query("SELECT id, name, note FROM t ORDER BY id")
            .expect("query runs");
        assert!(result.next_row());

        let row = result.current_row().expect("the advanced row is current");
        // A column holding another type and a NULL both read as "no value", like DynXX's
        // `readColumn`.
        assert_eq!(row.get_string("id").expect("the column exists"), None);
        assert_eq!(row.get_i64("name").expect("the column exists"), None);
        assert_eq!(row.get_f64("id").expect("the column exists"), None);
        assert_eq!(
            row.get_string("note").expect("the column exists"),
            None,
            "NULL has no value"
        );

        // A column the query never selected is a different answer: the caller named it wrong.
        assert_eq!(
            row.get_string("not-a-column"),
            Err(ColumnError::NoSuchColumn)
        );
        assert_eq!(row.get_i64("not-a-column"), Err(ColumnError::NoSuchColumn));
        assert_eq!(row.get_f64("not-a-column"), Err(ColumnError::NoSuchColumn));
    }

    #[test]
    fn an_empty_result_has_no_rows() {
        let mut db = seeded_db();
        let mut result = db
            .query("SELECT id FROM t WHERE id < 0")
            .expect("query runs");
        assert!(!result.next_row());
        assert!(result.current_row().is_none());
    }

    #[test]
    fn broken_query_sql_is_reported() {
        let mut db = seeded_db();
        assert!(db.query("SELECT bad syntax FROM").is_err());
    }

    #[test]
    fn a_result_keeps_its_rows_after_the_connection_is_reused() {
        let mut db = seeded_db();
        let mut result = db
            .query("SELECT name FROM t ORDER BY id")
            .expect("query runs");
        db.exec("DELETE FROM t").expect("rows are deleted");

        // Rows are materialized when the query runs, so they stay readable afterwards.
        assert!(result.next_row());
        assert_eq!(
            result
                .current_row()
                .expect("the advanced row is current")
                .get_string("name")
                .expect("the column exists")
                .as_deref(),
            Some("one")
        );
    }
}
