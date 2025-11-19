use cggmp21::{
    supported_curves::Secp256k1,
    security_level::SecurityLevel128,
    KeyShare,
    trusted_dealer,
};
use generic_ec::{NonZero, SecretScalar, Point, Scalar};
use rand::rngs::OsRng;
use anyhow::{Result, Context, anyhow};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::io::Write;
use age::Encryptor;

/// Configuration for key share generation
#[derive(Debug, Clone)]
pub struct KeyGenConfig {
    /// Number of parties
    pub n_parties: u16,
    /// Threshold for signing
    pub threshold: u16,
    /// Account identifier
    pub account_id: String,
    /// Child key bytes (32 bytes)
    pub child_key: [u8; 32],
    /// Output file prefix
    pub output_prefix: String,
    /// Age public keys for encrypting each file (optional)
    pub pubkeys: Option<Vec<String>>,
}

/// Key share dealer for MPC HD Wallet
pub struct KeyShareDealer {
    config: KeyGenConfig,
    key_shares: Option<Vec<KeyShare<Secp256k1, SecurityLevel128>>>,
}

impl KeyShareDealer {
    /// Create a new key share dealer with the given configuration
    pub fn new(config: KeyGenConfig) -> Result<Self> {
        // Validate configuration
        Self::validate_config(&config)?;

        Ok(Self {
            config,
            key_shares: None,
        })
    }

    /// Validate the configuration
    fn validate_config(config: &KeyGenConfig) -> Result<()> {
        if config.n_parties < 2 {
            return Err(anyhow!("Number of parties must be at least 2"));
        }

        if config.threshold < 2 {
            return Err(anyhow!("Threshold must be at least 2"));
        }

        if config.threshold > config.n_parties {
            return Err(anyhow!("Threshold cannot exceed number of parties"));
        }

        Ok(())
    }

    /// Generate key shares from the child key
    pub fn generate_shares(&mut self) -> Result<()> {
        println!("\n🔐 Generating {}-of-{} MPC key shares...",
                 self.config.threshold, self.config.n_parties);

        // Convert key bytes to scalar
        let secret_scalar = self.create_scalar_from_bytes(&self.config.child_key)?;

        // Generate key shares using trusted dealer
        let key_shares = trusted_dealer::builder::<Secp256k1, SecurityLevel128>(self.config.n_parties)
            .set_threshold(Some(self.config.threshold))
            .set_shared_secret_key(secret_scalar)
            .hd_wallet(true)  // Enable HD wallet support
            .generate_shares(&mut OsRng)?;

        println!("   ✓ Generated {} key shares", key_shares.len());

        self.key_shares = Some(key_shares);
        Ok(())
    }

    /// Verify that the generated key shares match the expected public key
    pub fn verify_public_key(&self) -> Result<()> {
        let key_shares = self.key_shares.as_ref()
            .ok_or_else(|| anyhow!("Key shares not generated yet. Call generate_shares() first"))?;

        println!("\n🔍 Public Key Verification:");

        // Compute expected public key from the input child key
        let scalar_for_pubkey = Scalar::<Secp256k1>::from_be_bytes_mod_order(&self.config.child_key);
        let expected_pubkey: Point<Secp256k1> = Point::generator() * &scalar_for_pubkey;
        let expected_pubkey_hex = hex::encode(expected_pubkey.to_bytes(true));

        println!("   Expected public key (from input):  {}", expected_pubkey_hex);

        // Display shared public key
        let shared_pubkey = &key_shares[0].core.shared_public_key;
        let shared_pubkey_hex = hex::encode(shared_pubkey.to_bytes(true));
        println!("   MPC shared public key (generated): {}", shared_pubkey_hex);

        // Verify they match
        if expected_pubkey_hex == shared_pubkey_hex {
            println!("   ✅ MATCH: MPC key shares generated correctly!");
            Ok(())
        } else {
            Err(anyhow!("❌ MISMATCH: Public keys don't match! Key generation may have failed."))
        }
    }

    /// Save key shares to separate files, supporting append mode and optional encryption
    pub fn save_to_files(&self) -> Result<()> {
        let key_shares = self.key_shares.as_ref()
            .ok_or_else(|| anyhow!("Key shares not generated yet. Call generate_shares() first"))?;

        println!("\n💾 Saving key shares to files...");

        for (i, key_share) in key_shares.iter().enumerate() {
            let base_filename = format!("{}_{}.json", self.config.output_prefix, i + 1);
            let encrypted_filename = format!("{}.age", base_filename);

            // Determine which filename to use for existing data
            let (existing_filename, existing_encrypted) = if Path::new(&encrypted_filename).exists() {
                (&encrypted_filename, true)
            } else if Path::new(&base_filename).exists() {
                (&base_filename, false)
            } else {
                // No existing file
                if self.config.pubkeys.is_some() {
                    println!("   • Creating new encrypted file: {}", encrypted_filename);
                } else {
                    println!("   • Creating new file: {}", base_filename);
                }
                (&base_filename, false) // dummy, won't be used
            };

            // Load existing data if file exists
            let mut all_accounts: HashMap<String, serde_json::Value> = if Path::new(existing_filename).exists() {
                println!("   • Loading existing file: {}", existing_filename);
                let content = if existing_encrypted {
                    // Decrypt existing file
                    return Err(anyhow!("Cannot append to encrypted file. Decryption for appending is not yet supported. Please decrypt manually first."));
                } else {
                    fs::read_to_string(existing_filename)
                        .with_context(|| format!("Failed to read existing file: {}", existing_filename))?
                };

                serde_json::from_str(&content)
                    .with_context(|| format!("Failed to parse existing file: {}", existing_filename))?
            } else {
                HashMap::new()
            };

            // Check if account_id already exists
            if all_accounts.contains_key(&self.config.account_id) {
                println!("   ⚠️  Account '{}' already exists, will overwrite",
                         self.config.account_id);
            }

            // Serialize the key share for this account
            let key_share_value = serde_json::to_value(key_share)
                .context("Failed to serialize key share")?;

            // Insert/update the account
            all_accounts.insert(self.config.account_id.clone(), key_share_value);

            // Serialize to JSON
            let json = serde_json::to_string_pretty(&all_accounts)
                .context("Failed to serialize accounts map")?;

            // Write to file (encrypted or plain)
            if let Some(ref pubkeys) = self.config.pubkeys {
                // Encrypt and write
                let pubkey_str = &pubkeys[i];
                let output_filename = encrypted_filename;

                self.write_encrypted_file(&json, pubkey_str, &output_filename)?;
                println!("   ✓ Saved to {} (encrypted): {} account(s)", output_filename, all_accounts.len());
            } else {
                // Write plain JSON
                fs::write(&base_filename, json)
                    .with_context(|| format!("Failed to write file: {}", base_filename))?;
                println!("   ✓ Saved to {}: {} account(s)", base_filename, all_accounts.len());
            }
        }

        Ok(())
    }

    /// Write encrypted file using age
    fn write_encrypted_file(&self, content: &str, pubkey_str: &str, output_path: &str) -> Result<()> {
        // Parse the recipient public key
        let recipient = pubkey_str.parse::<age::x25519::Recipient>()
            .map_err(|e| anyhow!("Invalid age public key '{}': {}", pubkey_str, e))?;

        // Create encryptor
        let encryptor = Encryptor::with_recipients(vec![Box::new(recipient)])
            .ok_or_else(|| anyhow!("Failed to create encryptor"))?;

        // Open output file
        let output_file = fs::File::create(output_path)
            .with_context(|| format!("Failed to create output file: {}", output_path))?;

        // Create encrypted writer
        let mut encrypted_writer = encryptor
            .wrap_output(output_file)
            .context("Failed to create encrypted writer")?;

        // Write content
        encrypted_writer.write_all(content.as_bytes())
            .context("Failed to write encrypted content")?;

        // Finalize (important!)
        encrypted_writer.finish()
            .and_then(|_| Ok(()))
            .context("Failed to finalize encrypted file")?;

        Ok(())
    }

    /// Check if encryption is enabled
    pub fn is_encrypted(&self) -> bool {
        self.config.pubkeys.is_some()
    }

    /// Get the number of parties
    pub fn n_parties(&self) -> u16 {
        self.config.n_parties
    }

    /// Get the output prefix
    pub fn output_prefix(&self) -> &str {
        &self.config.output_prefix
    }

    /// Create a valid scalar from bytes
    fn create_scalar_from_bytes(&self, bytes: &[u8; 32]) -> Result<NonZero<SecretScalar<Secp256k1>>> {
        let scalar = SecretScalar::<Secp256k1>::from_be_bytes(bytes)
            .map_err(|_| anyhow!("Invalid private key"))?;

        NonZero::from_secret_scalar(scalar)
            .ok_or_else(|| anyhow!("Private key cannot be zero"))
    }
}

/// Helper function to parse hex string to 32-byte array
pub fn parse_child_key_hex(hex_str: &str) -> Result<[u8; 32]> {
    if hex_str.len() != 64 {
        return Err(anyhow!("Child key must be 64 hex characters (32 bytes)"));
    }

    let bytes = hex::decode(hex_str)
        .context("Failed to decode child key hex")?;

    if bytes.len() != 32 {
        return Err(anyhow!("Child key must be 32 bytes (64 hex characters)"));
    }

    let mut child_key = [0u8; 32];
    child_key.copy_from_slice(&bytes);

    Ok(child_key)
}

