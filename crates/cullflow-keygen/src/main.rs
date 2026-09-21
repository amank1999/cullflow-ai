use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use cullflow_core::license::{
    machine_fingerprint, sign_license, verify_license_with_key, LicensePayload, LicenseTier,
};
use ed25519_dalek::SigningKey;
use std::env;

/// CullFlow AI license tooling. Seller-side only - never ships inside the
/// desktop app. See this crate's section of the repo README for how the
/// signing key is meant to be stored and used.
#[derive(Parser)]
#[command(name = "cullflow-keygen")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generates a new Ed25519 signing keypair. Print the private key ONCE,
    /// store it somewhere safe (password manager / secrets vault) and never
    /// commit it - then paste the public key into
    /// `cullflow-core::license::LICENSE_PUBLIC_KEY_B64` so the app can
    /// verify keys issued with the matching private key.
    Genkey,

    /// Issues a signed license key. Reads the seller's private key from the
    /// CULLFLOW_SIGNING_KEY env var (base64, as printed by `genkey`) so it
    /// never has to be typed on the command line or stored in shell history.
    Issue {
        /// Customer's email, embedded in the license for support/lookup purposes.
        #[arg(long)]
        email: String,

        /// Pricing tier this key unlocks.
        #[arg(long, value_enum)]
        tier: Tier,

        /// Optional expiry in days from now. Omit for a perpetual license
        /// (the Founding Pass's one-time-purchase model).
        #[arg(long)]
        expires_in_days: Option<i64>,
    },

    /// Verifies a license key against a given public key (for testing the
    /// pipeline end to end without touching the app's embedded key).
    Verify {
        key: String,
        #[arg(long)]
        public_key: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Tier {
    Founding,
    Pro,
    Studio,
}

impl From<Tier> for LicenseTier {
    fn from(t: Tier) -> Self {
        match t {
            Tier::Founding => LicenseTier::Founding,
            Tier::Pro => LicenseTier::ProFreelancer,
            Tier::Studio => LicenseTier::StudioSuite,
        }
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Genkey => {
            let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
            let private_b64 = URL_SAFE_NO_PAD.encode(signing_key.to_bytes());
            let public_b64 = URL_SAFE_NO_PAD.encode(signing_key.verifying_key().to_bytes());

            println!("Private key (KEEP SECRET - store safely, do not commit):");
            println!("  {private_b64}");
            println!();
            println!("Public key (paste into cullflow-core::license::LICENSE_PUBLIC_KEY_B64):");
            println!("  {public_b64}");
            println!();
            println!("To issue keys with this keypair:");
            println!("  export CULLFLOW_SIGNING_KEY={private_b64}");
        }

        Command::Issue {
            email,
            tier,
            expires_in_days,
        } => {
            let signing_key = match load_signing_key() {
                Ok(k) => k,
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            };

            let payload = LicensePayload {
                license_id: format!("lic_{}", uuid_v4_like()),
                customer_email: email,
                tier: tier.into(),
                issued_at: Utc::now(),
                expires_at: expires_in_days.map(|d| Utc::now() + Duration::days(d)),
            };

            let key = sign_license(&payload, &signing_key);
            println!("{key}");
        }

        Command::Verify { key, public_key } => match verify_license_with_key(&key, &public_key) {
            Ok(payload) => {
                println!("valid: {payload:#?}");
                println!("this machine's fingerprint: {}", machine_fingerprint());
            }
            Err(e) => {
                eprintln!("invalid: {e}");
                std::process::exit(1);
            }
        },
    }
}

fn load_signing_key() -> Result<SigningKey, String> {
    let b64 = env::var("CULLFLOW_SIGNING_KEY")
        .map_err(|_| "CULLFLOW_SIGNING_KEY env var not set - run `genkey` first".to_string())?;
    let bytes = URL_SAFE_NO_PAD
        .decode(&b64)
        .map_err(|_| "CULLFLOW_SIGNING_KEY is not valid base64url".to_string())?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "CULLFLOW_SIGNING_KEY is the wrong length for an Ed25519 key".to_string())?;
    Ok(SigningKey::from_bytes(&bytes))
}

/// A dependency-free, good-enough-for-a-label unique id (not used for any
/// security property - the license's authenticity comes from the
/// signature, not from this id being unguessable).
fn uuid_v4_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{nanos:x}")
}
