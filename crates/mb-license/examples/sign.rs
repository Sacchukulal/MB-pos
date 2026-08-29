//! Sign a release manifest — what CI runs after it has built the installer.
//!
//! ```text
//! MB_RELEASE_KEY=<base64 seed or pkcs8> cargo run -p mb-license --example sign -- manifest.json
//! cargo run -p mb-license --example sign -- --dev manifest.json      (the development key)
//! ```
//!
//! Writes `manifest.json.sig` beside the manifest: the base64 Ed25519 signature over the
//! file's exact bytes, which is what `updates::check` verifies. Prints the public key it
//! signed with, so a release can be checked against `snapshot::PRODUCTION_PUBLIC_KEY`
//! before it goes out.

#![allow(
    clippy::expect_used,
    clippy::print_stdout,
    clippy::exit,
    reason = "a developer tool that must fail loudly and whose whole job is to print"
)]

use base64::Engine as _;
use ring::signature::{Ed25519KeyPair, KeyPair as _};

fn key_from_env() -> Ed25519KeyPair {
    let text = std::env::var("MB_RELEASE_KEY").unwrap_or_default();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .expect("MB_RELEASE_KEY is base64");
    if bytes.len() == 32 {
        return Ed25519KeyPair::from_seed_unchecked(&bytes).expect("a 32-byte seed");
    }
    Ed25519KeyPair::from_pkcs8(&bytes).expect("MB_RELEASE_KEY is a 32-byte seed or a pkcs8 key")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dev = args.iter().any(|a| a == "--dev");
    let Some(path) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: sign [--dev] <manifest.json>");
        std::process::exit(2);
    };
    let key = if dev {
        mb_license::snapshot::development_keypair().expect("the development key")
    } else {
        key_from_env()
    };
    let payload = std::fs::read(path).expect("the manifest is readable");
    let signature = mb_license::snapshot::sign_detached(&payload, &key);
    let out = format!("{path}.sig");
    std::fs::write(&out, &signature).expect("the signature is writable");

    let public: Vec<String> = key.public_key().as_ref().iter().map(|b| format!("{b:02x}")).collect();
    println!("signed {path} -> {out}");
    println!("public key: {}", public.join(""));
    let production: Option<String> = mb_license::snapshot::PRODUCTION_PUBLIC_KEY
        .map(|k| k.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(""));
    match production {
        Some(p) if p == public.join("") => println!("this is the production key a release build trusts"),
        Some(_) => println!("WARNING: not the production key — a release build will refuse this manifest"),
        None => println!("no production key is set in this build"),
    }
}
