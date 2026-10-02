fn main() {
    let dir = std::env::temp_dir().join("fjall-wasip2-probe");
    let keyspace = fjall::Config::new(&dir).open().expect("fjall open");
    let items = keyspace
        .open_partition("player", fjall::PartitionCreateOptions::default())
        .expect("partition");
    items.insert("k1", "v1").expect("insert");
    println!("fjall wasip2 probe ok, temp={}", dir.display());
}