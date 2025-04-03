use redb::{Database, Error, TableDefinition};
use std::path::Path;

// Define table names for different value types
const INT_TABLE: TableDefinition<&str, i64> = TableDefinition::new("integers");
const FLOAT_TABLE: TableDefinition<&str, f64> = TableDefinition::new("floats");
const STRING_TABLE: TableDefinition<&str, &str> = TableDefinition::new("strings");

pub struct KV {
    db: Database,
}

impl KV {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let db = Database::create(path)?;
        Ok(Self { db })
    }

    pub fn write_int(&self, key: &str, value: i64) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(INT_TABLE)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn read_int(&self, key: &str) -> Result<Option<i64>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(INT_TABLE)?;
        Ok(table.get(key)?.map(|x| x.value()))
    }

    pub fn write_float(&self, key: &str, value: f64) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(FLOAT_TABLE)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn read_float(&self, key: &str) -> Result<Option<f64>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(FLOAT_TABLE)?;
        Ok(table.get(key)?.map(|x| x.value()))
    }

    pub fn write_string(&self, key: &str, value: &str) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(STRING_TABLE)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn read_string(&self, key: &str) -> Result<Option<String>, Error> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(STRING_TABLE)?;
        Ok(table.get(key)?.map(|x| x.value().to_string()))
    }
}