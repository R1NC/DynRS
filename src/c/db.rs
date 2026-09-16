use crate::c::util::{box_into_raw_new, cstr_to_rust, ngenrs_free_cstr, rust_to_cstr};
use crate::core::db::{DB, QueryResult};
use std::ffi::{c_char, c_void};
use std::ptr;

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_open(path: *const c_char) -> *mut c_void {
    if path.is_null() {
        return ptr::null_mut();
    }

    let path_str = match cstr_to_rust(path) {
        Some(s) => s,
        None => return ptr::null_mut(),
    };

    match DB::open(path_str) {
        Ok(db) => box_into_raw_new(db) as *mut c_void,
        Err(_) => ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_exec(db: *mut c_void, sql: *const c_char) -> bool {
    if db.is_null() || sql.is_null() {
        return false;
    }

    let db = unsafe { &*(db as *mut DB) };
    let sql_str = match cstr_to_rust(sql) {
        Some(s) => s,
        None => return false,
    };

    db.exec(sql_str).is_ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_query(db: *mut c_void, sql: *const c_char) -> *mut c_void {
    if db.is_null() || sql.is_null() {
        return ptr::null_mut();
    }

    let db = unsafe { &mut *(db as *mut DB) };
    let sql_str = match cstr_to_rust(sql) {
        Some(s) => s,
        None => return ptr::null_mut(),
    };

    match db.query(sql_str) {
        Ok(result) => box_into_raw_new(result) as *mut c_void,
        Err(_) => ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_next_row(result: *mut c_void) -> bool {
    if result.is_null() {
        return false;
    }

    let result = unsafe { &mut *(result as *mut QueryResult) };
    result.next_row()
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_get_string(result: *mut c_void, column: *const c_char) -> *mut c_char {
    if result.is_null() || column.is_null() {
        return ptr::null_mut();
    }

    let result = unsafe { &mut *(result as *mut QueryResult) };
    let column_str = match cstr_to_rust(column) {
        Some(s) => s,
        None => return ptr::null_mut(),
    };

    match result
        .current_row()
        .and_then(|row| row.get_string(column_str))
    {
        Some(s) => rust_to_cstr(s),
        None => ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_get_i64(result: *mut c_void, column: *const c_char) -> i64 {
    if result.is_null() || column.is_null() {
        return 0;
    }

    let result = unsafe { &mut *(result as *mut QueryResult) };
    let column_str = match cstr_to_rust(column) {
        Some(s) => s,
        None => return 0,
    };

    result
        .current_row()
        .and_then(|row| row.get_i64(column_str))
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_get_f64(result: *mut c_void, column: *const c_char) -> f64 {
    if result.is_null() || column.is_null() {
        return 0.0;
    }

    let result = unsafe { &mut *(result as *mut QueryResult) };
    let column_str = match cstr_to_rust(column) {
        Some(s) => s,
        None => return 0.0,
    };

    result
        .current_row()
        .and_then(|row| row.get_f64(column_str))
        .unwrap_or(0.0)
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_free_string(s: *mut c_char) {
    ngenrs_free_cstr(s)
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_free_database(db: *mut c_void) {
    if !db.is_null() {
        unsafe {
            let _ = Box::from_raw(db as *mut DB);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ngenrs_db_free_result(result: *mut c_void) {
    if !result.is_null() {
        unsafe {
            let _ = Box::from_raw(result as *mut QueryResult);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};

    fn cstr(value: &str) -> CString {
        CString::new(value).unwrap()
    }

    /// An in-memory database, so the tests leave no file behind.
    fn open_memory_db() -> *mut c_void {
        let path = cstr(":memory:");
        let db = ngenrs_db_open(path.as_ptr());
        assert!(!db.is_null(), "in-memory database opens");
        db
    }

    fn read_cstr(ptr: *mut c_char) -> String {
        assert!(!ptr.is_null(), "a C string is returned");
        unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string()
    }

    #[test]
    fn null_arguments_are_rejected() {
        assert!(ngenrs_db_open(std::ptr::null()).is_null());
        assert!(!ngenrs_db_exec(std::ptr::null_mut(), std::ptr::null()));
        assert!(ngenrs_db_query(std::ptr::null_mut(), std::ptr::null()).is_null());
        assert!(!ngenrs_db_next_row(std::ptr::null_mut()));
        assert!(ngenrs_db_get_string(std::ptr::null_mut(), std::ptr::null()).is_null());
        assert_eq!(ngenrs_db_get_i64(std::ptr::null_mut(), std::ptr::null()), 0);
        assert_eq!(
            ngenrs_db_get_f64(std::ptr::null_mut(), std::ptr::null()),
            0.0
        );

        // The release entry points tolerate a null handle.
        ngenrs_db_free_string(std::ptr::null_mut());
        ngenrs_db_free_database(std::ptr::null_mut());
        ngenrs_db_free_result(std::ptr::null_mut());
    }

    #[test]
    fn open_rejects_an_unusable_path() {
        let path = cstr("no-such-directory/db.sqlite");
        assert!(ngenrs_db_open(path.as_ptr()).is_null());
    }

    #[test]
    fn exec_reports_failure_for_empty_and_broken_sql() {
        let db = open_memory_db();

        let create = cstr("CREATE TABLE t (id INTEGER, name TEXT)");
        assert!(ngenrs_db_exec(db, create.as_ptr()));

        let empty = cstr("");
        assert!(!ngenrs_db_exec(db, empty.as_ptr()));
        let broken = cstr("CREATE TABLE t (");
        assert!(!ngenrs_db_exec(db, broken.as_ptr()));

        // Several statements in one call, like `sqlite3_exec`.
        let batch = cstr("INSERT INTO t VALUES (1, 'one'); INSERT INTO t VALUES (2, 'two');");
        assert!(ngenrs_db_exec(db, batch.as_ptr()));

        ngenrs_db_free_database(db);
    }

    #[test]
    fn rows_are_iterated_and_columns_read_from_the_current_row() {
        let db = open_memory_db();
        let setup = cstr(
            "CREATE TABLE t (id INTEGER, name TEXT); \
             INSERT INTO t VALUES (1, 'one'), (2, 'two');",
        );
        assert!(ngenrs_db_exec(db, setup.as_ptr()));

        let sql = cstr("SELECT id, name FROM t ORDER BY id");
        let result = ngenrs_db_query(db, sql.as_ptr());
        assert!(!result.is_null());

        let id = cstr("id");
        let name = cstr("name");

        // Nothing is readable before the first `ngenrs_db_next_row`.
        assert_eq!(ngenrs_db_get_i64(result, id.as_ptr()), 0);

        assert!(ngenrs_db_next_row(result), "the first row is available");
        assert_eq!(ngenrs_db_get_i64(result, id.as_ptr()), 1);
        let first = ngenrs_db_get_string(result, name.as_ptr());
        assert_eq!(read_cstr(first), "one");
        ngenrs_db_free_string(first);

        // The second call moves on instead of repeating the first row.
        assert!(ngenrs_db_next_row(result), "the second row is available");
        assert_eq!(ngenrs_db_get_i64(result, id.as_ptr()), 2);
        let second = ngenrs_db_get_string(result, name.as_ptr());
        assert_eq!(read_cstr(second), "two");
        ngenrs_db_free_string(second);

        assert!(
            !ngenrs_db_next_row(result),
            "iteration stops after the last row"
        );
        assert_eq!(ngenrs_db_get_i64(result, id.as_ptr()), 0);
        assert!(ngenrs_db_get_string(result, name.as_ptr()).is_null());

        ngenrs_db_free_result(result);
        ngenrs_db_free_database(db);
    }

    #[test]
    fn values_of_another_type_missing_columns_and_nulls_read_as_empty() {
        let db = open_memory_db();
        let setup = cstr(
            "CREATE TABLE t (id INTEGER, name TEXT, note TEXT); INSERT INTO t VALUES (1, 'one', NULL);",
        );
        assert!(ngenrs_db_exec(db, setup.as_ptr()));

        let sql = cstr("SELECT id, name, note FROM t");
        let result = ngenrs_db_query(db, sql.as_ptr());
        assert!(!result.is_null());
        assert!(ngenrs_db_next_row(result));

        let id = cstr("id");
        let name = cstr("name");
        let note = cstr("note");
        let missing = cstr("not-a-column");

        // A text read of an integer column yields nothing, like DynXX's `readColumn`.
        assert!(ngenrs_db_get_string(result, id.as_ptr()).is_null());
        assert_eq!(ngenrs_db_get_i64(result, name.as_ptr()), 0);
        assert_eq!(ngenrs_db_get_f64(result, id.as_ptr()), 0.0);
        assert!(
            ngenrs_db_get_string(result, note.as_ptr()).is_null(),
            "NULL has no value"
        );
        assert!(ngenrs_db_get_string(result, missing.as_ptr()).is_null());
        assert_eq!(ngenrs_db_get_i64(result, missing.as_ptr()), 0);

        ngenrs_db_free_result(result);
        ngenrs_db_free_database(db);
    }

    #[test]
    fn broken_query_sql_yields_no_result() {
        let db = open_memory_db();
        let result = ngenrs_db_query(db, cstr("SELECT bad syntax FROM").as_ptr());
        assert!(result.is_null());
        ngenrs_db_free_database(db);
    }
}
