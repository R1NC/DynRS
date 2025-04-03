use rusqlite::{Connection, Statement, types::Value};

pub struct DB {
    conn: Connection,
}

pub struct QueryResult<'a> {
    stmt: Statement<'a>,
    columns: Vec<String>,
}

pub struct QueryResultRow {
    values: Vec<Value>,
    column_indices: std::collections::HashMap<String, usize>,
}

impl DB {
    pub fn open(path: &str) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        Ok(Self { conn })
    }

    pub fn exec(&self, sql: &str) -> Result<(), rusqlite::Error> {
        self.conn.execute(sql, [])?;
        Ok(())
    }

    pub fn query(&mut self, sql: &str) -> Result<QueryResult, rusqlite::Error> {
        let stmt = self.conn.prepare(sql)?;
        let columns = stmt.column_names().iter().map(|s| s.to_string()).collect();
        Ok(QueryResult { stmt, columns })
    }
}

impl<'a> QueryResult<'a> {
    pub fn next_row(&mut self) -> Option<QueryResultRow> {
        let mut rows = self.stmt.query([]).ok()?;
        
        match rows.next() {
            Ok(Some(row)) => {
                let mut values = Vec::new();
                let mut column_indices = std::collections::HashMap::new();
                
                for (idx, name) in self.columns.iter().enumerate() {
                    column_indices.insert(name.clone(), idx);
                    if let Ok(value) = row.get::<_, Value>(idx) {
                        values.push(value);
                    }
                }
                
                Some(QueryResultRow { values, column_indices })
            },
            _ => None,
        }
    }
}

impl QueryResultRow {
    fn get_value(&self, column: &str) -> Option<&Value> {
        let idx = self.column_indices.get(column)?;
        self.values.get(*idx)
    }

    pub fn get_string(&self, column: &str) -> Option<String> {
        match self.get_value(column)? {
            Value::Text(s) => Some(s.clone()),
            _ => None,
        }
    }

    pub fn get_i64(&self, column: &str) -> Option<i64> {
        match self.get_value(column)? {
            Value::Integer(i) => Some(*i),
            _ => None,
        }
    }

    pub fn get_f64(&self, column: &str) -> Option<f64> {
        match self.get_value(column)? {
            Value::Real(f) => Some(*f),
            _ => None,
        }
    }
}