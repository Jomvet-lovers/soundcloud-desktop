use dpi_desync::{Desync, Strategy};

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let strat = match args.get(1).map(|s| s.as_str()).unwrap_or("multisplit") {
        "none" => Strategy::None,
        "tlsrec" => Strategy::TlsRec,
        "split" => Strategy::Split,
        "tlsrecsplit" => Strategy::TlsRecSplit,
        _ => Strategy::MultiSplit,
    };
    let d = Desync::spawn(true).await?;
    d.set_strategy(strat);
    println!("{}", d.proxy_url());
    eprintln!("listening on {} strategy={:?}", d.addr(), strat);
    std::future::pending::<()>().await;
    Ok(())
}
