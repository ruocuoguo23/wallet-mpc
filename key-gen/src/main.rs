mod dealer;

use dealer::{KeyShareDealer, KeyGenConfig, parse_child_key_hex};
use anyhow::{Result};
use clap::Parser;
use chrono::Local;

fn timestamp() -> String {
    Local::now().format("[%Y-%m-%d %H:%M:%S%.3f]").to_string()
}

/// Key Generation Tool for MPC HD Wallet
///
/// This tool generates MPC key shares from a derived child key.
/// Note: The input key should be a pre-derived child key from your HD wallet,
/// as MPC HD wallet derivation differs from traditional BIP-32.
#[derive(Parser, Debug)]
#[command(name = "key-gen")]
#[command(about = "Generate MPC key shares for HD wallet child accounts", long_about = None)]
struct Args {
    /// Child key in hex format (64 characters, 32 bytes)
    /// This should be a pre-derived key from your HD wallet
    #[arg(short = 'k', long)]
    child_key: String,

    /// Account ID for this key (e.g., "m/44/60/0/0/0" or "account_1")
    /// Used as the identifier in the key share files
    #[arg(short, long)]
    account_id: String,

    /// Number of participants (default: 2)
    #[arg(short = 'n', long, default_value = "2")]
    n_parties: u16,

    /// Threshold for signing (default: 2)
    #[arg(short = 't', long, default_value = "2")]
    threshold: u16,

    /// Output file prefix (default: "key_shares")
    /// Will generate {prefix}_1.json, {prefix}_2.json
    #[arg(short, long, default_value = "key_shares")]
    output: String,

    /// Age public keys for encrypting each key share file (comma-separated)
    /// Format: "pubkey1,pubkey2,pubkey3" where pubkey1 encrypts {prefix}_1.json, etc.
    /// If not provided, files will not be encrypted.
    /// Example: age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p,age1...
    #[arg(short = 'p', long)]
    pubkeys: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("{} === MPC HD Wallet Key Share Generator ===\n", timestamp());

    // Parse child key
    let child_key = parse_child_key_hex(&args.child_key)?;

    println!("{} 🔑 Account ID: {}", timestamp(), args.account_id);
    println!("{}    Child Key (hex): {}", timestamp(), hex::encode(&child_key));

    // Parse public keys if provided
    let pubkeys = if let Some(ref pubkeys_str) = args.pubkeys {
        let keys: Vec<String> = pubkeys_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        
        if keys.len() != args.n_parties as usize {
            eprintln!("{} ⚠️  Warning: Number of public keys ({}) doesn't match number of parties ({})",
                     timestamp(), keys.len(), args.n_parties);
            eprintln!("{}    Files will not be encrypted.", timestamp());
            None
        } else {
            println!("{} 🔐 Encryption: Enabled ({} public keys provided)", timestamp(), keys.len());
            Some(keys)
        }
    } else {
        println!("{} 🔓 Encryption: Disabled (no public keys provided)", timestamp());
        None
    };

    // Create key generation configuration
    let config = KeyGenConfig {
        n_parties: args.n_parties,
        threshold: args.threshold,
        account_id: args.account_id,
        child_key,
        output_prefix: args.output,
        pubkeys,
    };

    // Create dealer and generate key shares
    println!("{} 📋 Creating key share dealer...", timestamp());
    let mut dealer = KeyShareDealer::new(config)?;
    dealer.generate_shares()?;

    // Verify the public key matches
    dealer.verify_public_key()?;

    // Save key shares to files
    dealer.save_to_files()?;

    println!("\n{} ✅ Key share generation complete!", timestamp());
    println!("\n📁 Output files:");
    for i in 1..=dealer.n_parties() {
        let filename = if dealer.is_encrypted() {
            format!("{}_{}.json.age", dealer.output_prefix(), i)
        } else {
            format!("{}_{}.json", dealer.output_prefix(), i)
        };
        println!("   • {}", filename);
    }

    println!("\n💡 Usage:");
    println!("   Each file contains key shares for one or more account IDs.");
    println!("   You can run this tool multiple times to add more accounts to the same files.");
    println!("   The files support multiple account_ids with the account_id as the key.");
    if dealer.is_encrypted() {
        println!("\n🔐 Decryption:");
        println!("   Files are encrypted with age. To decrypt:");
        println!("   age --decrypt -i <identity-file> -o output.json input.json.age");
    }

    Ok(())
}
