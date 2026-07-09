//! Steward CLI — the only human-facing ops path for lab kits.
//! Does NOT accept dog invites. Ever.

use anyhow::Context;
use clap::{Parser, Subcommand};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(
    name = "steward",
    about = "CanisLink steward ops (install, bond, e-stop)"
)]
struct Args {
    #[arg(long, env = "CANIS_API", default_value = "http://127.0.0.1:8080")]
    api: String,
    #[arg(
        long,
        env = "CANIS_STEWARD_SECRET",
        default_value = "canis-steward-secret"
    )]
    steward_secret: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Enroll a new terminal + dog (lab). Prints IDs and device token.
    Enroll,
    /// Create a mutual bond between two dogs so they may Call each other.
    Bond {
        #[arg(long)]
        dog_a: Uuid,
        #[arg(long)]
        dog_b: Uuid,
        #[arg(long, default_value_t = 0.7)]
        weight: f32,
    },
    /// Emergency stop a dog (ends sessions, blocks social).
    #[command(name = "estop")]
    EStop {
        #[arg(long)]
        dog: Uuid,
        /// 1/0 or true/false
        #[arg(long)]
        enabled: String,
    },
    /// Disable social without e-stop incident UX.
    #[command(name = "social-disable")]
    SocialDisable {
        #[arg(long)]
        dog: Uuid,
        #[arg(long)]
        enabled: String,
    },
    /// Health check
    Health,
}

#[derive(Deserialize)]
struct EnrollOut {
    terminal_id: Uuid,
    dog_id: Uuid,
    token: String,
}

fn parse_bool(s: &str) -> anyhow::Result<bool> {
    match s.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => anyhow::bail!("invalid bool {other:?}, use true/false"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let client = reqwest::Client::new();
    match args.cmd {
        Cmd::Health => {
            let v: serde_json::Value = client
                .get(format!("{}/healthz", args.api))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Cmd::Enroll => {
            let out: EnrollOut = client
                .post(format!("{}/v1/dev/enroll", args.api))
                .json(&serde_json::json!({}))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await
                .context("enroll")?;
            println!("terminal_id={}", out.terminal_id);
            println!("dog_id={}", out.dog_id);
            println!("token={}", out.token);
            println!();
            println!("# edge env example:");
            println!("export CANIS_API={}", args.api);
            println!("export CANIS_TERMINAL_ID={}", out.terminal_id);
            println!("export CANIS_DOG_ID={}", out.dog_id);
            println!("export CANIS_DEVICE_TOKEN={}", out.token);
        }
        Cmd::Bond {
            dog_a,
            dog_b,
            weight,
        } => {
            client
                .post(format!("{}/v1/steward/bonds", args.api))
                .header("Authorization", format!("Steward {}", args.steward_secret))
                .json(&serde_json::json!({"dog_a": dog_a, "dog_b": dog_b, "weight": weight}))
                .send()
                .await?
                .error_for_status()?;
            println!("bonded {dog_a} ↔ {dog_b} (weight={weight})");
        }
        Cmd::EStop { dog, enabled } => {
            let enabled = parse_bool(&enabled)?;
            client
                .post(format!("{}/v1/steward/estop", args.api))
                .header("Authorization", format!("Steward {}", args.steward_secret))
                .json(&serde_json::json!({"dog_id": dog, "enabled": enabled}))
                .send()
                .await?
                .error_for_status()?;
            println!("emergency_stop dog={dog} enabled={enabled}");
        }
        Cmd::SocialDisable { dog, enabled } => {
            let enabled = parse_bool(&enabled)?;
            client
                .post(format!("{}/v1/steward/social_disabled", args.api))
                .header("Authorization", format!("Steward {}", args.steward_secret))
                .json(&serde_json::json!({"dog_id": dog, "enabled": enabled}))
                .send()
                .await?
                .error_for_status()?;
            println!("social_disabled dog={dog} enabled={enabled}");
        }
    }
    Ok(())
}
