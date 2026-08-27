//! Mint the release signing keypair — step 1 of `docs/RELEASE.md`.
//!
//! ```text
//! cargo run -p mb-license --example keygen
//! ```

#![allow(
    clippy::expect_used,
    clippy::print_stdout,
    reason = "a developer tool that must fail loudly and whose whole job is to print"
)]

use base64::Engine as _;
use ring::signature::{Ed25519KeyPair, KeyPair as _};

fn main() {
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).expect("the OS has a random source");
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("what we just generated parses");

    println!("Magic Bill release signing key");
    println!("==============================\n");

    println!("PRIVATE — 1Password (Magic Bill / Release signing key) AND the CI");
    println!("secret MB_RELEASE_KEY. This is printed once and is not saved anywhere.\n");
    println!(
        "{}\n",
        base64::engine::general_purpose::STANDARD.encode(pkcs8.as_ref())
    );

    println!("PUBLIC — paste into `snapshot::PRODUCTION_PUBLIC_KEY`:\n");
    print!("pub const PRODUCTION_PUBLIC_KEY: Option<&[u8]> = Some(&[");
    for (index, byte) in pair.public_key().as_ref().iter().enumerate() {
        if index % 8 == 0 {
            print!("\n    ");
        }
        print!("0x{byte:02x}, ");
    }
    println!("\n]);\n");

    println!("Then delete DEVELOPMENT_SEED and the test that points at it.");
    println!("See MB-pos/docs/RELEASE.md section 5.");
}
