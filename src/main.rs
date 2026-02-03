use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use rayon::prelude::*;

/// Target suffixes to search for (must only contain valid base58 characters).
/// Base58 alphabet: 123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz
/// (no 0, O, I, or l)
const TARGETS: &[&str] = &[
    "poop", "shit", "fuck", "grind", "spoon", "fork", "foid",
];

struct FoundWallet {
    suffix: String,
    public_key: String,
    keypair_bytes: Vec<u8>,
}

fn main() {
    println!("=== Solana Vanity Wallet Finder ===");
    println!();

    // Validate all targets contain only valid base58 characters
    let base58_chars = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    for target in TARGETS {
        for ch in target.chars() {
            if !base58_chars.contains(ch) {
                eprintln!(
                    "ERROR: Target '{}' contains '{}' which is not a valid base58 character.",
                    target, ch
                );
                eprintln!("Base58 does not include: 0, O, I, l");
                std::process::exit(1);
            }
        }
    }

    let remaining: Arc<Mutex<HashMap<String, bool>>> = Arc::new(Mutex::new(
        TARGETS.iter().map(|t| (t.to_string(), false)).collect(),
    ));
    let results: Arc<Mutex<Vec<FoundWallet>>> = Arc::new(Mutex::new(Vec::new()));
    let total_attempts = Arc::new(AtomicUsize::new(0));
    let total_targets = TARGETS.len();

    println!("Searching for {} wallet suffixes:", total_targets);
    for target in TARGETS {
        let chars = target.len();
        let combos: u64 = 58u64.pow(chars as u32);
        println!(
            "  ...{} (~1 in {} addresses)",
            target,
            format_number(combos)
        );
    }
    println!();
    println!(
        "Using {} threads.",
        rayon::current_num_threads()
    );
    println!("4-char suffixes should be fast. 5-char suffixes will take longer.");
    println!();

    let start = Instant::now();

    // Use rayon to parallelize the brute-force search
    (0..rayon::current_num_threads())
        .into_par_iter()
        .for_each(|_| {
            let mut rng = OsRng;
            let mut local_attempts: usize = 0;

            loop {
                // Check if all targets have been found
                {
                    let rem = remaining.lock().unwrap();
                    if rem.values().all(|&found| found) {
                        break;
                    }
                }

                // Generate a random Ed25519 keypair
                let signing_key = SigningKey::generate(&mut rng);
                let verifying_key = signing_key.verifying_key();
                let pubkey_b58 = bs58::encode(verifying_key.as_bytes()).into_string();

                local_attempts += 1;

                // Check against all unfound targets
                {
                    let mut rem = remaining.lock().unwrap();
                    for target in TARGETS {
                        let key = target.to_string();
                        if let Some(&found) = rem.get(&key) {
                            if !found && pubkey_b58.ends_with(target) {
                                rem.insert(key.clone(), true);

                                // Build the 64-byte keypair (secret key + public key)
                                let mut keypair_bytes = Vec::with_capacity(64);
                                keypair_bytes.extend_from_slice(signing_key.as_bytes());
                                keypair_bytes.extend_from_slice(verifying_key.as_bytes());

                                let found_count = rem.values().filter(|&&v| v).count();

                                println!(
                                    "\n  [{}/{}] FOUND '{}': {}",
                                    found_count, total_targets, target, pubkey_b58
                                );

                                // Save wallet to disk immediately
                                let wallet = FoundWallet {
                                    suffix: target.to_string(),
                                    public_key: pubkey_b58.clone(),
                                    keypair_bytes,
                                };
                                save_wallet(&wallet);
                                results.lock().unwrap().push(wallet);
                            }
                        }
                    }
                }

                // Periodically report progress
                if local_attempts % 500_000 == 0 {
                    let total = total_attempts.fetch_add(500_000, Ordering::Relaxed) + 500_000;
                    let elapsed = start.elapsed().as_secs_f64();
                    let rate = total as f64 / elapsed;
                    let rem = remaining.lock().unwrap();
                    let found_count = rem.values().filter(|&&v| v).count();
                    eprint!(
                        "\r  Attempts: {} | Rate: {:.0}/sec | Found: {}/{}    ",
                        format_number(total as u64),
                        rate,
                        found_count,
                        total_targets
                    );
                }
            }
        });

    let elapsed = start.elapsed();
    eprintln!();
    println!();
    println!("=== Complete ===");
    println!("Finished in {:.1}s", elapsed.as_secs_f64());
    println!();

    // List all found wallets (already saved to disk during search)
    let results = results.lock().unwrap();
    for wallet in results.iter() {
        let filename = format!("wallets/{}-{}.json", wallet.suffix, wallet.public_key);
        println!("Saved: {}", filename);
        println!("  Address: {}", wallet.public_key);
        println!();
    }

    println!("All keypair files are Solana CLI compatible.");
    println!("Use with: solana-keygen pubkey wallets/<file>.json");
    println!("Or set as default: solana config set --keypair wallets/<file>.json");
}

fn save_wallet(wallet: &FoundWallet) {
    let _ = fs::create_dir_all("wallets");
    let json_bytes: Vec<serde_json::Value> = wallet
        .keypair_bytes
        .iter()
        .map(|&b| serde_json::Value::Number(b.into()))
        .collect();
    let json = serde_json::to_string(&json_bytes).unwrap();
    let filename = format!("wallets/{}-{}.json", wallet.suffix, wallet.public_key);
    fs::write(&filename, &json).unwrap();
    println!("  Saved: {}", filename);
}

fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}
