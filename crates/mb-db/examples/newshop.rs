//! Make an empty shop database, so a session can RUN the app and look at it.
#![allow(clippy::expect_used, clippy::print_stdout, reason = "a dev tool")]

fn main() {
    let path = std::env::args().nth(1).expect("usage: newshop <db>");
    let db = mb_db::Db::open(&mb_db::DbConfig::new(std::path::PathBuf::from(&path)))
        .expect("it opens and migrates");
    println!("made {} ({} tables)", path, db.path().display());
}
