use redb::{Database, Error, TableDefinition};
use std::path::Path;

// Define table names for different value types
const INT_TABLE: TableDefinition<&str, i64> = TableDefinition::new("integers");
const FLOAT_TABLE: TableDefinition<&str, f64> = TableDefinition::new("floats");
const STRING_TABLE: TableDefinition<&str, &str> = TableDefinition::new("strings");

pub struct KV {
    db: Database,
}

// redb's error type is 160 bytes, but an error only appears when an I/O operation fails and
// is mapped to a bool/null at the C ABI right away, so boxing it would only add allocations.
#[allow(clippy::result_large_err)]
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
}
