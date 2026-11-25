//! cargo run -p thurea-core --example qr -- TEXT OUT.pgm [low|high]   (used to check codes with zbarimg)
//! With `low` or `high`, also prints the module rows (as `thurea ... --tray` sends them).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let ecc = match a.get(3).map(|s| s.as_str()) {
        Some("low") => thurea_core::qr::Ecc::Low,
        Some("high") => thurea_core::qr::Ecc::High,
        _ => thurea_core::qr::Ecc::Medium,
    };
    let rows = a.len() > 3;
    let q = thurea_core::qr::QrCode::encode(a[1].as_bytes(), ecc).expect("too long");
    std::fs::write(&a[2], q.to_pgm(4)).unwrap();
    println!("{}", q.version);
    if rows {
        for row in q.to_matrix() {
            println!("{row}");
        }
    }
}
