fn main() {
    let db = redb::Database::create(":memory:").expect("redb create");
    let txn = db.begin_write().expect("txn");
    {
        let mut table = txn.open_table("player").expect("open table");
        table.insert("k1", "v1").expect("insert");
    }
    txn.commit().expect("commit");
    println!("redb wasip2 probe ok");
}