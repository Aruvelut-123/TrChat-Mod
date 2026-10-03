use std::num::NonZero;
use std::sync::Arc;

fn main() {
    let path = std::env::var("PROBE_DB").unwrap_or_else(|_| "probe-file.db".into());
    let io = Arc::new(turso_core::io::PlatformIO::new().expect("platform io"));
    let opts = || turso_core::OpenOptions::new(Arc::new(turso_core::SqliteDialect {}));
    let db = turso_core::Database::open(io.clone(), &path, opts()).expect("open file db");
    let conn = db.connect().expect("connect");

    conn.execute(
        "CREATE TABLE IF NOT EXISTS t (id INTEGER PRIMARY KEY, name TEXT, flag INTEGER NOT NULL DEFAULT 0)",
    )
    .expect("create");

    // 1) changes() 语义函数：UPDATE 影响行数
    conn.execute("DELETE FROM t").expect("clear");
    conn.execute("INSERT INTO t (name) VALUES ('a'), ('b'), ('c')")
        .expect("insert 3");
    let mut stmt = conn.query("SELECT changes()").expect("query").expect("stmt");
    let mut changed: i64 = -1;
    loop {
        match stmt.step().expect("step") {
            turso_core::StepResult::Row => {
                changed = stmt.row().expect("row").get::<i64>(0).unwrap_or(-1);
            }
            _ => break,
        }
    }
    println!("after-insert changes()={changed} (expect 3)");
    assert_eq!(changed, 3);

    // UPDATE 命中 3 行
    conn.execute("UPDATE t SET flag=1").expect("update 3");
    let mut stmt = conn.query("SELECT changes()").expect("query").expect("stmt");
    let mut changed: i64 = -1;
    loop {
        match stmt.step().expect("step") {
            turso_core::StepResult::Row => {
                changed = stmt.row().expect("row").get::<i64>(0).unwrap_or(-1);
            }
            _ => break,
        }
    }
    println!("after-update changes()={changed} (expect 3)");
    assert_eq!(changed, 3);

    // UPDATE 不命中（0 行）→ save 双语句应走 INSERT
    conn.execute("UPDATE t SET flag=0 WHERE id=999").expect("update 0");
    let mut stmt = conn.query("SELECT changes()").expect("query").expect("stmt");
    let mut changed: i64 = -1;
    loop {
        match stmt.step().expect("step") {
            turso_core::StepResult::Row => {
                changed = stmt.row().expect("row").get::<i64>(0).unwrap_or(-1);
            }
            _ => break,
        }
    }
    println!("after-update-0 changes()={changed} (expect 0)");
    assert_eq!(changed, 0);

    // 2) 参数绑定：bind_at + Value::from_text / from_i64
    let mut stmt = conn
        .prepare("INSERT INTO t (name, flag) VALUES (?, ?)")
        .expect("prepare insert");
    stmt.bind_at(NonZero::new(1).unwrap(), turso_core::Value::from_text("bound"))
        .expect("bind 1");
    stmt.bind_at(NonZero::new(2).unwrap(), turso_core::Value::from_i64(7))
        .expect("bind 2");
    stmt.step().expect("run insert");
    let mut q = conn
        .query("SELECT name, flag FROM t WHERE name=?")
        .expect("query")
        .expect("stmt");
    q.bind_at(NonZero::new(1).unwrap(), turso_core::Value::from_text("bound"))
        .expect("bind q");
    let mut got = ("".to_string(), -1i64);
    loop {
        match q.step().expect("step") {
            turso_core::StepResult::Row => {
                let row = q.row().expect("row");
                got = (
                    row.get::<String>(0).unwrap_or_default(),
                    row.get::<i64>(1).unwrap_or(-1),
                );
            }
            _ => break,
        }
    }
    println!("bound row = ({:?}, {}) (expect (\"bound\", 7))", got.0, got.1);
    assert_eq!(got, ("bound".to_string(), 7));

    // 3) 事务：BEGIN / COMMIT / ROLLBACK 文本是否被接受
    conn.execute("BEGIN").expect("begin");
    conn.execute("DELETE FROM t WHERE name='bound'").expect("del in txn");
    conn.execute("ROLLBACK").expect("rollback");
    let mut q = conn.query("SELECT count(*) FROM t WHERE name='bound'").expect("query").expect("stmt");
    let mut n: i64 = -1;
    loop {
        match q.step().expect("step") {
            turso_core::StepResult::Row => {
                n = q.row().expect("row").get::<i64>(0).unwrap_or(-1);
            }
            _ => break,
        }
    }
    println!("after-rollback bound count={n} (expect 1)");
    assert_eq!(n, 1);

    // 4) 多语句 execute：BEGIN; ...; COMMIT; 一段传
    conn.execute("BEGIN; DELETE FROM t WHERE name='bound'; COMMIT;").expect("multi txn");
    let mut q = conn.query("SELECT count(*) FROM t WHERE name='bound'").expect("query").expect("stmt");
    let mut n: i64 = -1;
    loop {
        match q.step().expect("step") {
            turso_core::StepResult::Row => {
                n = q.row().expect("row").get::<i64>(0).unwrap_or(-1);
            }
            _ => break,
        }
    }
    println!("after-multi-txn bound count={n} (expect 0)");
    assert_eq!(n, 0);

    drop(conn);
    drop(db);
    println!("probe-changes OK");
}
