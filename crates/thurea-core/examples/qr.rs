//! cargo run -p thurea-core --example qr -- TEXT OUT.pgm   (used to check codes with zbarimg)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let q = thurea_core::qr::QrCode::encode(a[1].as_bytes(), thurea_core::qr::Ecc::Medium).expect("too long");
    std::fs::write(&a[2], q.to_pgm(4)).unwrap();
    println!("{}", q.version);
}
