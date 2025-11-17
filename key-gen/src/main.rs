use cggmp21::{
    supported_curves::Secp256k1,
    security_level::SecurityLevel128,
    KeyShare,
    trusted_dealer,
};
use generic_ec::{NonZero, SecretScalar, Point, Scalar};
use rand::rngs::OsRng;
use anyhow::{Result, Context, anyhow};
use clap::Parser;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

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

    /// Number of participants (default: 3)
    #[arg(short = 'n', long, default_value = "3")]
    n_parties: u16,

    /// Threshold for signing (default: 2)
    #[arg(short = 't', long, default_value = "2")]
    threshold: u16,

    /// Output file prefix (default: "key_shares")
    /// Will generate {prefix}_1.json, {prefix}_2.json, {prefix}_3.json
    #[arg(short, long, default_value = "key_shares")]
    output: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("=== MPC HD Wallet Key Share Generator ===\n");

    // Validate inputs
    validate_args(&args)?;

    // Parse child key
    let child_key_bytes = hex::decode(&args.child_key)
        .context("Failed to decode child key hex")?;

    if child_key_bytes.len() != 32 {
        return Err(anyhow!("Child key must be 32 bytes (64 hex characters)"));
    }

    let mut child_key = [0u8; 32];
    child_key.copy_from_slice(&child_key_bytes);

    println!("🔑 Account ID: {}", args.account_id);
    println!("   Child Key (hex): {}", hex::encode(&child_key));

    // Convert key bytes to scalar
    let secret_scalar = create_scalar_from_bytes(&child_key)?;

    // Compute public key from the input child key (before secret_scalar is moved)
    // We need to convert the bytes to a Scalar to compute the public key
    let scalar_for_pubkey = Scalar::<Secp256k1>::from_be_bytes_mod_order(&child_key);
    let expected_pubkey: Point<Secp256k1> = Point::generator() * &scalar_for_pubkey;
    let expected_pubkey_hex = hex::encode(expected_pubkey.to_bytes(true));

    // Generate key shares using trusted dealer
    println!("\n🔐 Generating {}-of-{} MPC key shares...", args.threshold, args.n_parties);

    let key_shares = trusted_dealer::builder::<Secp256k1, SecurityLevel128>(args.n_parties)
        .set_threshold(Some(args.threshold))
        .set_shared_secret_key(secret_scalar)
        .hd_wallet(true)  // Enable HD wallet support
        .generate_shares(&mut OsRng)?;

    println!("   ✓ Generated {} key shares", key_shares.len());

    // Display and verify public keys
    println!("\n🔍 Public Key Verification:");
    println!("   Expected public key (from input):  {}", expected_pubkey_hex);

    // Display shared public key
    let shared_pubkey = &key_shares[0].core.shared_public_key;
    let shared_pubkey_hex = hex::encode(shared_pubkey.to_bytes(true));
    println!("   MPC shared public key (generated): {}", shared_pubkey_hex);

    // Verify they match
    if expected_pubkey_hex == shared_pubkey_hex {
        println!("   ✅ MATCH: MPC key shares generated correctly!");
    } else {
        return Err(anyhow!("❌ MISMATCH: Public keys don't match! Key generation may have failed."));
    }

    // Save key shares to files
    println!("\n💾 Saving key shares to files...");
    save_key_shares_to_files(&key_shares, &args.account_id, &args.output)?;

    println!("\n✅ Key share generation complete!");
    println!("\n📁 Output files:");
    for i in 1..=args.n_parties {
        println!("   • {}_{}.json", args.output, i);
    }

    println!("\n💡 Usage:");
    println!("   Each file contains key shares for one or more account IDs.");
    println!("   You can run this tool multiple times to add more accounts to the same files.");
    println!("   The files support multiple account_ids with the account_id as the key.");

    Ok(())
}

fn validate_args(args: &Args) -> Result<()> {
    // Validate n_parties
    if args.n_parties < 2 {
        return Err(anyhow!("Number of parties must be at least 2"));
    }

    // Validate threshold
    if args.threshold < 2 {
        return Err(anyhow!("Threshold must be at least 2"));
    }

    if args.threshold > args.n_parties {
        return Err(anyhow!("Threshold cannot exceed number of parties"));
    }

    // Validate child key length
    if args.child_key.len() != 64 {
        return Err(anyhow!("Child key must be 64 hex characters (32 bytes)"));
    }

    Ok(())
}

/// Create a valid scalar from bytes
fn create_scalar_from_bytes(bytes: &[u8; 32]) -> Result<NonZero<SecretScalar<Secp256k1>>> {
    let scalar = SecretScalar::<Secp256k1>::from_be_bytes(bytes)
        .map_err(|_| anyhow!("Invalid private key"))?;

    NonZero::from_secret_scalar(scalar)
        .ok_or_else(|| anyhow!("Private key cannot be zero"))
}

/// Save key shares to separate files, supporting append mode
fn save_key_shares_to_files(
    key_shares: &[KeyShare<Secp256k1, SecurityLevel128>],
    account_id: &str,
    output_prefix: &str,
) -> Result<()> {
    for (i, key_share) in key_shares.iter().enumerate() {
        let filename = format!("{}_{}.json", output_prefix, i + 1);

        // Load existing data if file exists
        let mut all_accounts: HashMap<String, serde_json::Value> = if Path::new(&filename).exists() {
            println!("   • Loading existing file: {}", filename);
            let content = fs::read_to_string(&filename)
                .with_context(|| format!("Failed to read existing file: {}", filename))?;

            serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse existing file: {}", filename))?
        } else {
            println!("   • Creating new file: {}", filename);
            HashMap::new()
        };

        // Check if account_id already exists
        if all_accounts.contains_key(account_id) {
            println!("   ⚠️  Account '{}' already exists in {}, will overwrite", account_id, filename);
        }

        // Serialize the key share for this account
        let key_share_value = serde_json::to_value(key_share)
            .context("Failed to serialize key share")?;

        // Insert/update the account
        all_accounts.insert(account_id.to_string(), key_share_value);

        // Write back to file
        let json = serde_json::to_string_pretty(&all_accounts)
            .context("Failed to serialize accounts map")?;

        fs::write(&filename, json)
            .with_context(|| format!("Failed to write file: {}", filename))?;

        println!("   ✓ Saved to {}: {} account(s)", filename, all_accounts.len());
    }

    Ok(())
}
