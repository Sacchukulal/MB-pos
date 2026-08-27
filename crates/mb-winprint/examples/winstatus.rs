//! What Windows says about a printer, and our stale jobs in its queue: a check for a real box.
//!
//! `cargo run -p mb-winprint --example winstatus -- "<printer name>" [purge]`

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(printer) = args.next() else {
        eprintln!("usage: winstatus <printer name> [purge]");
        return;
    };
    println!("trouble: {:?}", mb_winprint::printer_trouble(&printer));
    if args.next().as_deref() == Some("purge") {
        println!("purged: {:?}", mb_winprint::purge_jobs(&printer, "Magic Bill"));
    }
}
