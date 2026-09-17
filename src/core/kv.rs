// redb's error type is 160 bytes, but an error only appears when an I/O operation fails and is
// mapped to a bool/null at the C ABI right away, so boxing it would only add allocations. The
// allow covers the store methods and the free helpers alike.
#![allow(clippy::result_large_err)]

use redb::{
    Database, Error, Key, ReadOnlyTable, ReadTransaction, ReadableDatabase, ReadableTable,
    StorageError, TableDefinition, TableError, Value,
};
use std::collections::BTreeSet;
use std::path::Path;

// Define table names for different value types
const INT_TABLE: TableDefinition<&str, i64> = TableDefinition::new("integers");
const FLOAT_TABLE: TableDefinition<&str, f64> = TableDefinition::new("floats");
const STRING_TABLE: TableDefinition<&str, &str> = TableDefinition::new("strings");

/// Opens a table for reading, mapping "never written to" onto `None` so that a missing table reads
/// like a missing key instead of failing.
fn open_table_for_read<K, V>(
    txn: &ReadTransaction,
    definition: TableDefinition<K, V>,
) -> Result<Option<ReadOnlyTable<K, V>>, Error>
where
    K: Key + 'static,
    V: Value + 'static,
{
    match txn.open_table(definition) {
        Ok(table) => Ok(Some(table)),
        Err(TableError::TableDoesNotExist(_)) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Reports whether a typed table holds `key`, treating a table that was never written to as empty.
macro_rules! table_contains {
    ($txn:expr, $definition:expr, $key:expr) => {
        (match open_table_for_read($txn, $definition)? {
            Some(table) => table.get($key)?.is_some(),
            None => false,
        })
    };
}

/// Rejects an empty key before the store is touched, the way DynXX does. The public signatures stay
/// `Result<_, redb::Error>`, so the rule travels as an invalid-input error from that type.
fn check_key(key: &str) -> Result<(), Error> {
    if key.is_empty() {
        return Err(StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the key must not be empty",
        ))
        .into());
    }
    Ok(())
}

pub struct KV {
    db: Database,
}

impl KV {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let db = Database::create(path)?;
        Ok(Self { db })
    }

    pub fn write_int(&self, key: &str, value: i64) -> Result<(), Error> {
        check_key(key)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(INT_TABLE)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn read_int(&self, key: &str) -> Result<Option<i64>, Error> {
        // An empty key reads as "no value", like DynXX, rather than as a failure.
        if key.is_empty() {
            return Ok(None);
        }
        let read_txn = self.db.begin_read()?;
        match open_table_for_read(&read_txn, INT_TABLE)? {
            Some(table) => Ok(table.get(key)?.map(|value| value.value())),
            None => Ok(None),
        }
    }

    pub fn write_float(&self, key: &str, value: f64) -> Result<(), Error> {
        check_key(key)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(FLOAT_TABLE)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn read_float(&self, key: &str) -> Result<Option<f64>, Error> {
        if key.is_empty() {
            return Ok(None);
        }
        let read_txn = self.db.begin_read()?;
        match open_table_for_read(&read_txn, FLOAT_TABLE)? {
            Some(table) => Ok(table.get(key)?.map(|value| value.value())),
            None => Ok(None),
        }
    }

    pub fn write_string(&self, key: &str, value: &str) -> Result<(), Error> {
        check_key(key)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(STRING_TABLE)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn read_string(&self, key: &str) -> Result<Option<String>, Error> {
        if key.is_empty() {
            return Ok(None);
        }
        let read_txn = self.db.begin_read()?;
        match open_table_for_read(&read_txn, STRING_TABLE)? {
            Some(table) => Ok(table.get(key)?.map(|value| value.value().to_string())),
            None => Ok(None),
        }
    }

    /// Mirrors `dynxxKVContains`: the key exists when any of the typed tables holds it.
    pub fn contains(&self, key: &str) -> Result<bool, Error> {
        if key.is_empty() {
            return Ok(false);
        }

        let read_txn = self.db.begin_read()?;
        // One store key can sit in more than one table, so every table has to be checked.
        Ok(table_contains!(&read_txn, INT_TABLE, key)
            || table_contains!(&read_txn, FLOAT_TABLE, key)
            || table_contains!(&read_txn, STRING_TABLE, key))
    }

    /// Mirrors `dynxxKVRemove`: drops the key from every typed table and reports whether it was
    /// there at all.
    pub fn remove(&self, key: &str) -> Result<bool, Error> {
        if key.is_empty() {
            return Ok(false);
        }

        let write_txn = self.db.begin_write()?;
        let mut removed = false;
        {
            let mut table = write_txn.open_table(INT_TABLE)?;
            removed |= table.remove(key)?.is_some();
        }
        {
            let mut table = write_txn.open_table(FLOAT_TABLE)?;
            removed |= table.remove(key)?.is_some();
        }
        {
            let mut table = write_txn.open_table(STRING_TABLE)?;
            removed |= table.remove(key)?.is_some();
        }
        write_txn.commit()?;
        Ok(removed)
    }

    /// Mirrors `dynxxKVAllKeys`: every key name once, sorted so that the order is deterministic
    /// (MMKV's own order is not).
    pub fn all_keys(&self) -> Result<Vec<String>, Error> {
        let read_txn = self.db.begin_read()?;
        let mut keys = BTreeSet::new();

        if let Some(table) = open_table_for_read(&read_txn, INT_TABLE)? {
            for entry in table.iter()? {
                let (key, _) = entry?;
                keys.insert(key.value().to_string());
            }
        }
        if let Some(table) = open_table_for_read(&read_txn, FLOAT_TABLE)? {
            for entry in table.iter()? {
                let (key, _) = entry?;
                keys.insert(key.value().to_string());
            }
        }
        if let Some(table) = open_table_for_read(&read_txn, STRING_TABLE)? {
            for entry in table.iter()? {
                let (key, _) = entry?;
                keys.insert(key.value().to_string());
            }
        }

        Ok(keys.into_iter().collect())
    }

    /// Mirrors `dynxxKVClear`: empties every typed table.
    pub fn clear(&self) -> Result<(), Error> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(INT_TABLE)?;
            table.retain(|_, _| false)?;
        }
        {
            let mut table = write_txn.open_table(FLOAT_TABLE)?;
            table.retain(|_, _| false)?;
        }
        {
            let mut table = write_txn.open_table(STRING_TABLE)?;
            table.retain(|_, _| false)?;
        }
        write_txn.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A redb file under the system temp directory that removes itself, and any leftover of
    /// an earlier run, on drop. A test must declare it before the store so that the store is
    /// closed first, which Windows needs before the file can be removed again.
    struct TempDbFile {
        path: PathBuf,
    }

    impl TempDbFile {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("dynrs_kv_{name}_{}.redb", std::process::id()));
            let _ = std::fs::remove_file(&path);
            Self { path }
        }
    }

    impl Drop for TempDbFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[test]
    fn open_rejects_an_empty_path() {
        assert!(KV::open("").is_err());
    }

    #[test]
    fn int_float_and_string_round_trip() {
        let file = TempDbFile::new("round_trip");
        let kv = KV::open(&file.path).expect("store opens");

        kv.write_int("i", -42).expect("int is written");
        kv.write_float("f", 3.5).expect("float is written");
        kv.write_string("s", "value").expect("string is written");

        assert_eq!(kv.read_int("i").expect("int is read"), Some(-42));
        assert_eq!(kv.read_float("f").expect("float is read"), Some(3.5));
        assert_eq!(
            kv.read_string("s").expect("string is read").as_deref(),
            Some("value")
        );

        // A key that was never written reads as no value.
        assert_eq!(kv.read_int("missing").expect("missing int is read"), None);
        assert_eq!(
            kv.read_float("missing").expect("missing float is read"),
            None
        );
        assert_eq!(
            kv.read_string("missing").expect("missing string is read"),
            None
        );
    }

    #[test]
    fn a_value_type_does_not_see_another_ones_key() {
        let file = TempDbFile::new("per_type");
        let kv = KV::open(&file.path).expect("store opens");

        kv.write_int("k", 1).expect("int is written");
        kv.write_float("k", 1.5).expect("float is written");
        kv.write_string("k", "one").expect("string is written");

        assert_eq!(kv.read_int("k").expect("int is read"), Some(1));
        assert_eq!(kv.read_float("k").expect("float is read"), Some(1.5));
        assert_eq!(
            kv.read_string("k").expect("string is read").as_deref(),
            Some("one")
        );
    }

    #[test]
    fn writing_a_key_again_replaces_its_value() {
        let file = TempDbFile::new("overwrite");
        let kv = KV::open(&file.path).expect("store opens");

        kv.write_string("k", "first")
            .expect("first value is written");
        kv.write_string("k", "second")
            .expect("second value is written");

        assert_eq!(
            kv.read_string("k").expect("string is read").as_deref(),
            Some("second")
        );
    }

    #[test]
    fn values_survive_a_reopen() {
        let file = TempDbFile::new("reopen");

        {
            let kv = KV::open(&file.path).expect("store opens");
            kv.write_int("i", 7).expect("int is written");
            kv.write_float("f", 1.25).expect("float is written");
            kv.write_string("s", "kept").expect("string is written");
        }

        let kv = KV::open(&file.path).expect("store reopens");
        assert_eq!(kv.read_int("i").expect("int is read"), Some(7));
        assert_eq!(kv.read_float("f").expect("float is read"), Some(1.25));
        assert_eq!(
            kv.read_string("s").expect("string is read").as_deref(),
            Some("kept")
        );
    }

    #[test]
    fn empty_unicode_large_and_extreme_values_round_trip() {
        let file = TempDbFile::new("payloads");
        let kv = KV::open(&file.path).expect("store opens");

        kv.write_string("empty", "")
            .expect("empty string is written");
        assert_eq!(
            kv.read_string("empty")
                .expect("empty string is read")
                .as_deref(),
            Some("")
        );

        kv.write_string("unicode", "键-值 🔑")
            .expect("unicode is written");
        assert_eq!(
            kv.read_string("unicode")
                .expect("unicode is read")
                .as_deref(),
            Some("键-值 🔑")
        );

        let large = "x".repeat(64 * 1024);
        kv.write_string("large", &large)
            .expect("large value is written");
        assert_eq!(
            kv.read_string("large")
                .expect("large value is read")
                .as_deref(),
            Some(large.as_str())
        );

        kv.write_int("min", i64::MIN).expect("i64::MIN is written");
        kv.write_int("max", i64::MAX).expect("i64::MAX is written");
        assert_eq!(
            kv.read_int("min").expect("i64::MIN is read"),
            Some(i64::MIN)
        );
        assert_eq!(
            kv.read_int("max").expect("i64::MAX is read"),
            Some(i64::MAX)
        );
    }

    #[test]
    fn keys_stay_independent_of_each_other() {
        let file = TempDbFile::new("keys");
        let kv = KV::open(&file.path).expect("store opens");

        kv.write_int("a", 1).expect("a is written");
        kv.write_int("b", 2).expect("b is written");

        assert_eq!(kv.read_int("a").expect("a is read"), Some(1));
        assert_eq!(kv.read_int("b").expect("b is read"), Some(2));
        assert_eq!(kv.read_int("c").expect("c is read"), None);
    }

    #[test]
    fn a_fresh_store_reads_as_empty() {
        let file = TempDbFile::new("fresh");
        let kv = KV::open(&file.path).expect("store opens");

        // Reading a table that was never written to is "no value", not a failure.
        assert_eq!(kv.read_int("nope").expect("int is read"), None);
        assert_eq!(kv.read_float("nope").expect("float is read"), None);
        assert_eq!(kv.read_string("nope").expect("string is read"), None);
        assert!(!kv.contains("nope").expect("contains answers"));
        assert!(kv.all_keys().expect("keys are listed").is_empty());
    }

    #[test]
    fn contains_remove_and_all_keys_work_per_key() {
        let file = TempDbFile::new("contains_remove");
        let kv = KV::open(&file.path).expect("store opens");

        kv.write_int("b", 2).expect("b is written");
        kv.write_string("a", "value").expect("a is written");

        // Key names come back sorted, each name once.
        assert_eq!(kv.all_keys().expect("keys are listed"), ["a", "b"]);
        assert!(kv.contains("a").expect("a is contained"));
        assert!(kv.contains("b").expect("b is contained"));
        assert!(!kv.contains("c").expect("contains answers"));

        assert!(kv.remove("a").expect("a is removed"));
        assert!(!kv.contains("a").expect("a is gone"));
        // Removing it again reports that there was nothing left to remove.
        assert!(!kv.remove("a").expect("removing twice answers"));
        assert_eq!(kv.all_keys().expect("keys are listed"), ["b"]);
        assert_eq!(kv.read_string("a").expect("a is read"), None);
    }

    #[test]
    fn one_key_can_hold_every_type_and_remove_drops_them_all() {
        let file = TempDbFile::new("multi_type");
        let kv = KV::open(&file.path).expect("store opens");

        kv.write_int("k", 1).expect("int is written");
        kv.write_float("k", 1.5).expect("float is written");
        kv.write_string("k", "one").expect("string is written");

        // The name is reported once even though three tables hold it.
        assert_eq!(kv.all_keys().expect("keys are listed"), ["k"]);

        assert!(kv.remove("k").expect("k is removed"));
        assert_eq!(kv.read_int("k").expect("int is read"), None);
        assert_eq!(kv.read_float("k").expect("float is read"), None);
        assert_eq!(kv.read_string("k").expect("string is read"), None);
    }

    #[test]
    fn clear_empties_every_type() {
        let file = TempDbFile::new("clear");
        let kv = KV::open(&file.path).expect("store opens");

        kv.write_int("i", 1).expect("int is written");
        kv.write_float("f", 1.5).expect("float is written");
        kv.write_string("s", "one").expect("string is written");

        kv.clear().expect("store is cleared");

        assert!(kv.all_keys().expect("keys are listed").is_empty());
        assert!(!kv.contains("i").expect("contains answers"));
        assert_eq!(kv.read_int("i").expect("int is read"), None);
        assert_eq!(kv.read_float("f").expect("float is read"), None);
        assert_eq!(kv.read_string("s").expect("string is read"), None);

        // The store stays usable afterwards.
        kv.write_string("s", "two").expect("s is written again");
        assert_eq!(
            kv.read_string("s").expect("s is read").as_deref(),
            Some("two")
        );
    }

    #[test]
    fn an_empty_key_is_refused_like_dynxx() {
        let file = TempDbFile::new("empty_key");
        let kv = KV::open(&file.path).expect("store opens");

        // Writes fail, reads answer "no value", and the key is never present.
        assert!(kv.write_int("", 1).is_err());
        assert!(kv.write_float("", 1.5).is_err());
        assert!(kv.write_string("", "v").is_err());
        assert_eq!(kv.read_int("").expect("int is read"), None);
        assert_eq!(kv.read_float("").expect("float is read"), None);
        assert_eq!(kv.read_string("").expect("string is read"), None);
        assert!(!kv.contains("").expect("contains answers"));
        assert!(!kv.remove("").expect("remove answers"));
        assert!(kv.all_keys().expect("keys are listed").is_empty());
    }
}
