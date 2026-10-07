use crate::DynrsStatus;
use crate::c::util::{box_into_raw_new, bytes_to_c, cstr_to_rust, ngenrs_free_cstr};
use crate::core::db::{ColumnError, DB, QueryResult};
use std::ffi::{c_char, c_void};

/// Opens a database, handing the handle back through `out`.
///
/// The status separates a rejected argument from a database that would not open, which used to be
/// the same null pointer.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_open(path: *const c_char, out: *mut *mut c_void) -> DynrsStatus {
    ffi_return! {
        if !out.is_null() {
            unsafe { *out = std::ptr::null_mut() };
        }
        let Some(path_str) = cstr_to_rust(path) else {
            return DynrsStatus::InvalidArgument;
        };
        if out.is_null() {
            return DynrsStatus::InvalidArgument;
        }

        match DB::open(path_str) {
            Ok(db) => {
                unsafe { *out = box_into_raw_new(db) as *mut c_void };
                DynrsStatus::Ok
            }
            Err(_) => DynrsStatus::Failed,
        }
    }
}

/// Runs SQL. An empty statement is a rejected argument rather than a failure of the database.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_exec(db: *mut c_void, sql: *const c_char) -> DynrsStatus {
    ffi_return! {
        let Some(db) = (unsafe { (db as *const DB).as_ref() }) else {
            return DynrsStatus::InvalidHandle;
        };
        let Some(sql_str) = cstr_to_rust(sql) else {
            return DynrsStatus::InvalidArgument;
        };
        // Checked here rather than inferred from the database's error: "there is nothing to run"
        // is a different answer from "the statement was rejected".
        if sql_str.trim().is_empty() {
            return DynrsStatus::InvalidArgument;
        }

        match db.exec(sql_str) {
            Ok(()) => DynrsStatus::Ok,
            Err(_) => DynrsStatus::Failed,
        }
    }
}

/// Runs a query, handing the result handle back through `out`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_query(
    db: *mut c_void,
    sql: *const c_char,
    out: *mut *mut c_void,
) -> DynrsStatus {
    ffi_return! {
        if !out.is_null() {
            unsafe { *out = std::ptr::null_mut() };
        }
        let Some(db) = (unsafe { (db as *mut DB).as_mut() }) else {
            return DynrsStatus::InvalidHandle;
        };
        let Some(sql_str) = cstr_to_rust(sql) else {
            return DynrsStatus::InvalidArgument;
        };
        if out.is_null() {
            return DynrsStatus::InvalidArgument;
        }

        match db.query(sql_str) {
            Ok(result) => {
                unsafe { *out = box_into_raw_new(result) as *mut c_void };
                DynrsStatus::Ok
            }
            Err(_) => DynrsStatus::Failed,
        }
    }
}

/// Advances to the next row.
///
/// [`DynrsStatus::Empty`] means the iteration is over, which used to be the same `false` a null
/// handle produced.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_next_row(result: *mut c_void) -> DynrsStatus {
    ffi_return! {
        let Some(result) = (unsafe { (result as *mut QueryResult).as_mut() }) else {
            return DynrsStatus::InvalidHandle;
        };
        if result.next_row() {
            DynrsStatus::Ok
        } else {
            DynrsStatus::Empty
        }
    }
}

/// Reads a text column as bytes, writing its length through `len_out`.
///
/// The value is data the caller produced, so it may contain a NUL byte: as a C string such a value
/// came back as a null pointer, indistinguishable from "no row is current", "there is no such
/// column" and "the value is NULL". With a length it arrives whole, and a null pointer means only
/// that there is no value to report — in which case `len_out` is zero. A value that is present but
/// empty comes back as a non-null buffer of length zero.
///
/// Maps a column lookup failure onto the status the C caller sees.
fn column_status(error: ColumnError) -> DynrsStatus {
    match error {
        // The query did not select this column: a caller that typed the name wrong wants to know
        // that, rather than reading a zero and concluding the cell was empty.
        ColumnError::NoSuchColumn => DynrsStatus::InvalidArgument,
    }
}

/// Runs a column read and writes the value through `out`.
///
/// The three readers used to spell out the same five checks — out pointer, result handle, column
/// name, current row, error mapping — so the rule for each lived in three places.
macro_rules! column_read {
    ($result:expr, $column:expr, $out:expr, |$row:ident, $column_str:ident| $body:expr) => {{
        if $out.is_null() {
            return DynrsStatus::InvalidArgument;
        }
        let Some($row) = (unsafe { ($result as *mut QueryResult).as_mut() }) else {
            return DynrsStatus::InvalidHandle;
        };
        let Some($column_str) = cstr_to_rust($column) else {
            return DynrsStatus::InvalidArgument;
        };

        // No current row: the handle cannot answer until `ngenrs_db_next_row` advances it, which is
        // a different thing from a cell that holds NULL.
        let Some($row) = $row.current_row() else {
            return DynrsStatus::InvalidHandle;
        };
        match $body {
            Ok(Some(value)) => {
                unsafe { *$out = value };
                DynrsStatus::Ok
            }
            Ok(None) => DynrsStatus::Empty,
            Err(error) => column_status(error),
        }
    }};
}

/// Reads a text column as bytes, writing the buffer through `out` and its length through `len_out`.
///
/// The status separates the answers that used to be one null pointer:
/// [`DynrsStatus::Ok`] with a buffer — a value, possibly empty; [`DynrsStatus::Empty`] — a SQL
/// `NULL`, or a cell of another type; [`DynrsStatus::InvalidArgument`] — no such column;
/// [`DynrsStatus::InvalidHandle`] — a null handle, or no current row.
///
/// Release a non-null buffer with `ngenrs_free_bytes(ptr)`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_get_string(
    result: *mut c_void,
    column: *const c_char,
    out: *mut *mut u8,
    len_out: *mut usize,
) -> DynrsStatus {
    ffi_return! {
        if !len_out.is_null() {
            unsafe { *len_out = 0 };
        }
        // The bytes conversion needs `len_out`, so it happens inside the read rather than being
        // written straight into `out` the way the numeric readers are.
        if out.is_null() {
            return DynrsStatus::InvalidArgument;
        }
        let Some(result) = (unsafe { (result as *mut QueryResult).as_mut() }) else {
            return DynrsStatus::InvalidHandle;
        };
        let Some(column_str) = cstr_to_rust(column) else {
            return DynrsStatus::InvalidArgument;
        };
        let Some(row) = result.current_row() else {
            return DynrsStatus::InvalidHandle;
        };
        match row.get_string(column_str) {
            Ok(Some(text)) => {
                unsafe { *out = bytes_to_c(text.into_bytes(), len_out) };
                DynrsStatus::Ok
            }
            Ok(None) => DynrsStatus::Empty,
            Err(error) => column_status(error),
        }
    }
}

/// Reads an integer column into `out`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_get_i64(
    result: *mut c_void,
    column: *const c_char,
    out: *mut i64,
) -> DynrsStatus {
    ffi_return! {
        column_read!(result, column, out, |row, column| row.get_i64(column))
    }
}

/// Reads a real column into `out`.
#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_get_f64(
    result: *mut c_void,
    column: *const c_char,
    out: *mut f64,
) -> DynrsStatus {
    ffi_return! {
        column_read!(result, column, out, |row, column| row.get_f64(column))
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_free_string(s: *mut c_char) {
    ffi_return! {
        ngenrs_free_cstr(s)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_free_database(db: *mut c_void) {
    ffi_return! {
        if !db.is_null() {
            unsafe {
                let _ = Box::from_raw(db as *mut DB);
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_free_result(result: *mut c_void) {
    ffi_return! {
        if !result.is_null() {
            unsafe {
                let _ = Box::from_raw(result as *mut QueryResult);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn cstr(value: &str) -> CString {
        CString::new(value).unwrap()
    }

    /// An in-memory database, so the tests leave no file behind.
    fn open_memory_db() -> *mut c_void {
        let path = cstr(":memory:");
        let mut db = std::ptr::null_mut();
        assert_eq!(ngenrs_db_open(path.as_ptr(), &mut db), DynrsStatus::Ok);
        assert!(!db.is_null(), "in-memory database opens");
        db
    }

    /// Copies a text value out through the length-bearing reader and releases it, the way a C
    /// caller does.
    fn read_cstr(result: *mut c_void, column: *const c_char) -> String {
        let mut ptr = std::ptr::null_mut();
        let mut len = 0usize;
        assert_eq!(
            ngenrs_db_get_string(result, column, &mut ptr, &mut len),
            DynrsStatus::Ok,
            "a value is returned"
        );
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
        crate::c::util::ngenrs_free_bytes(ptr);
        String::from_utf8(bytes).expect("the test data is text")
    }

    #[test]
    fn null_arguments_are_rejected() {
        let mut handle = std::ptr::null_mut();
        assert_eq!(
            ngenrs_db_open(std::ptr::null(), &mut handle),
            DynrsStatus::InvalidArgument
        );
        assert!(handle.is_null());
        assert_eq!(
            ngenrs_db_exec(std::ptr::null_mut(), std::ptr::null()),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_db_query(std::ptr::null_mut(), std::ptr::null(), &mut handle),
            DynrsStatus::InvalidHandle
        );
        assert_eq!(
            ngenrs_db_next_row(std::ptr::null_mut()),
            DynrsStatus::InvalidHandle
        );
        // The text reader checks its output pointer first, because the length has to be written
        // beside the buffer: with nowhere to put a result there is nothing else to report.
        assert_eq!(
            ngenrs_db_get_string(
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut()
            ),
            DynrsStatus::InvalidArgument
        );
        let mut text = std::ptr::null_mut();
        let mut text_len = 0usize;
        assert_eq!(
            ngenrs_db_get_string(
                std::ptr::null_mut(),
                std::ptr::null(),
                &mut text,
                &mut text_len
            ),
            DynrsStatus::InvalidHandle
        );
        let mut int = 0i64;
        assert_eq!(
            ngenrs_db_get_i64(std::ptr::null_mut(), std::ptr::null(), &mut int),
            DynrsStatus::InvalidHandle
        );
        let mut real = 0.0f64;
        assert_eq!(
            ngenrs_db_get_f64(std::ptr::null_mut(), std::ptr::null(), &mut real),
            DynrsStatus::InvalidHandle
        );

        // The release entry points tolerate a null handle.
        ngenrs_db_free_string(std::ptr::null_mut());
        ngenrs_db_free_database(std::ptr::null_mut());
        ngenrs_db_free_result(std::ptr::null_mut());
    }

    #[test]
    fn open_rejects_an_unusable_path() {
        let path = cstr("no-such-directory/db.sqlite");
        let mut db = std::ptr::null_mut();
        assert_eq!(
            ngenrs_db_open(path.as_ptr(), &mut db),
            DynrsStatus::Failed,
            "the path is fine, the database is not"
        );
        assert!(db.is_null());
    }

    #[test]
    fn exec_reports_failure_for_empty_and_broken_sql() {
        let db = open_memory_db();

        let create = cstr("CREATE TABLE t (id INTEGER, name TEXT)");
        assert_eq!(ngenrs_db_exec(db, create.as_ptr()), DynrsStatus::Ok);

        // Empty SQL is a rejected argument: nothing was wrong with the database.
        let empty = cstr("");
        assert_eq!(
            ngenrs_db_exec(db, empty.as_ptr()),
            DynrsStatus::InvalidArgument
        );
        let broken = cstr("CREATE TABLE t (");
        assert_eq!(ngenrs_db_exec(db, broken.as_ptr()), DynrsStatus::Failed);

        // Several statements in one call, like `sqlite3_exec`.
        let batch = cstr("INSERT INTO t VALUES (1, 'one'); INSERT INTO t VALUES (2, 'two');");
        assert_eq!(ngenrs_db_exec(db, batch.as_ptr()), DynrsStatus::Ok);

        ngenrs_db_free_database(db);
    }

    #[test]
    fn rows_are_iterated_and_columns_read_from_the_current_row() {
        let db = open_memory_db();
        let setup = cstr(
            "CREATE TABLE t (id INTEGER, name TEXT); \
             INSERT INTO t VALUES (1, 'one'), (2, 'two');",
        );
        assert_eq!(ngenrs_db_exec(db, setup.as_ptr()), DynrsStatus::Ok);

        let sql = cstr("SELECT id, name FROM t ORDER BY id");
        let mut result = std::ptr::null_mut();
        assert_eq!(
            ngenrs_db_query(db, sql.as_ptr(), &mut result),
            DynrsStatus::Ok
        );

        let id = cstr("id");
        let name = cstr("name");

        // Nothing is readable before the first `ngenrs_db_next_row`: the handle has no current row,
        // which is reported as a handle that cannot answer rather than as a zero that looks like
        // data.
        let mut value = 0i64;
        assert_eq!(
            ngenrs_db_get_i64(result, id.as_ptr(), &mut value),
            DynrsStatus::InvalidHandle
        );

        assert_eq!(ngenrs_db_next_row(result), DynrsStatus::Ok);
        assert_eq!(
            ngenrs_db_get_i64(result, id.as_ptr(), &mut value),
            DynrsStatus::Ok
        );
        assert_eq!(value, 1);
        assert_eq!(read_cstr(result, name.as_ptr()), "one");

        // The second call moves on instead of repeating the first row.
        assert_eq!(ngenrs_db_next_row(result), DynrsStatus::Ok);
        assert_eq!(
            ngenrs_db_get_i64(result, id.as_ptr(), &mut value),
            DynrsStatus::Ok
        );
        assert_eq!(value, 2);
        assert_eq!(read_cstr(result, name.as_ptr()), "two");

        assert_eq!(
            ngenrs_db_next_row(result),
            DynrsStatus::Empty,
            "the iteration is over, which is not an error"
        );
        assert_eq!(
            ngenrs_db_get_i64(result, id.as_ptr(), &mut value),
            DynrsStatus::InvalidHandle,
            "the iteration is over, so there is no current row to read"
        );
        let mut ptr = std::ptr::null_mut();
        let mut len = 0usize;
        assert_eq!(
            ngenrs_db_get_string(result, name.as_ptr(), &mut ptr, &mut len),
            DynrsStatus::InvalidHandle,
            "no row is current, so there is no value"
        );
        assert!(ptr.is_null());

        ngenrs_db_free_result(result);
        ngenrs_db_free_database(db);
    }

    #[test]
    fn values_of_another_type_missing_columns_and_nulls_read_as_empty() {
        let db = open_memory_db();
        let setup = cstr(
            "CREATE TABLE t (id INTEGER, name TEXT, note TEXT); INSERT INTO t VALUES (1, 'one', NULL);",
        );
        assert_eq!(ngenrs_db_exec(db, setup.as_ptr()), DynrsStatus::Ok);

        let sql = cstr("SELECT id, name, note FROM t");
        let mut result = std::ptr::null_mut();
        assert_eq!(
            ngenrs_db_query(db, sql.as_ptr(), &mut result),
            DynrsStatus::Ok
        );
        assert_eq!(ngenrs_db_next_row(result), DynrsStatus::Ok);

        let id = cstr("id");
        let name = cstr("name");
        let note = cstr("note");
        let missing = cstr("not-a-column");

        // A text read of an integer column yields nothing, like DynXX's `readColumn` — the column
        // exists, the cell simply is not text.
        let mut ptr = std::ptr::null_mut();
        let mut len = usize::MAX;
        assert_eq!(
            ngenrs_db_get_string(result, id.as_ptr(), &mut ptr, &mut len),
            DynrsStatus::Empty
        );
        assert!(ptr.is_null(), "an empty answer reports no buffer");

        let mut value = 0i64;
        assert_eq!(
            ngenrs_db_get_i64(result, name.as_ptr(), &mut value),
            DynrsStatus::Empty
        );
        let mut real = 0.0f64;
        assert_eq!(
            ngenrs_db_get_f64(result, id.as_ptr(), &mut real),
            DynrsStatus::Empty
        );
        assert_eq!(
            ngenrs_db_get_string(result, note.as_ptr(), &mut ptr, &mut len),
            DynrsStatus::Empty,
            "NULL has no value"
        );

        // A column the query never selected is an argument error: the caller typed it wrong, and
        // that is worth telling apart from a cell that happens to be empty.
        assert_eq!(
            ngenrs_db_get_string(result, missing.as_ptr(), &mut ptr, &mut len),
            DynrsStatus::InvalidArgument,
            "a column that is not there is not an empty value"
        );
        assert_eq!(
            ngenrs_db_get_i64(result, missing.as_ptr(), &mut value),
            DynrsStatus::InvalidArgument
        );

        ngenrs_db_free_result(result);
        ngenrs_db_free_database(db);
    }

    #[test]
    fn broken_query_sql_yields_no_result() {
        let db = open_memory_db();
        let mut result = std::ptr::null_mut();
        assert_eq!(
            ngenrs_db_query(db, cstr("SELECT bad syntax FROM").as_ptr(), &mut result),
            DynrsStatus::Failed
        );
        assert!(result.is_null());
        ngenrs_db_free_database(db);
    }
}
