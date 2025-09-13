use anyhow::{anyhow, Result};
use clap::Parser;
use starr_core::{StarrProfile, StarrSession};
use std::io::{self, Read};
use std::thread;
use std::time::Duration;

/// Minimaler Plink-Klon (WinSCP-kompatibel genug fürs Daily-Use)
/// Beispiele:
///   starr-plink -ssh -P 22 -l user host -pw geheim
///   starr-plink user@host -i C:\Keys\id_ed25519 -pass meinePassphrase
#[derive(Parser, Debug)]
#[command(
    author, version, about = "starr-plink (MVP)",
    // Lass „komische“ Args durch (plink-kompat)
    trailing_var_arg = true,
    allow_hyphen_values = true,
    allow_external_subcommands = true,
    // Keine aggressiven Fehlermeldungen, damit WinSCP nicht stolpert
    disable_help_flag = false
)]
struct Args {
    /// host oder [user@]host (kann fehlen, wenn WinSCP uns den Host in EXTRAS reinwirft)
    host: Option<String>,

    /// -P <port>
    #[arg(short = 'P', long = "port", default_value_t = 22)]
    port: u16,

    /// -l <user>
    #[arg(short = 'l', long = "user")]
    user: Option<String>,

    /// -i <keyfile> (OpenSSH)
    #[arg(short = 'i', long = "identity")]
    identity: Option<String>,

    /// -pw <password>
    #[arg(long = "pw")]
    password: Option<String>,

    /// -pass <passphrase> für verschlüsselte Keys
    #[arg(long = "pass")]
    passphrase: Option<String>,

    /// akzeptiere, aber ignoriere plink-kompat Flags:
    #[arg(long = "ssh", help = "ignored (plink compat)")]
    _ssh: bool,

    #[arg(long = "batch", help = "ignored (plink compat)")]
    _batch: bool,

    // Sammel alle unbekannten/zusätzlichen Tokens (wir ignorieren die später)
    #[arg(hide = true)]
    extras: Vec<String>,
}

fn main() -> Result<()> {
    let a = Args::parse();

    // 1) Host/User ermitteln (user@host oder getrennt)
    let user = a.user.unwrap_or_else(whoami::username);
    let mut host_opt = a.host;

    // WinSCP schmeißt den Host manchmal in "extras". Pick ihn da raus, falls nötig.
    if host_opt.is_none() {
        host_opt = a.extras.iter().rev().find(|s| !s.starts_with('-')).cloned();
    }

    let host_raw = host_opt.ok_or_else(|| anyhow!("Kein Host übergeben"))?;
    let (user_final, host) = if let Some((u, h)) = host_raw.split_once('@') {
        (u.to_string(), h.to_string())
    } else {
        (user, host_raw)
    };

    // 2) Profil bauen
    let prof = StarrProfile {
        host,
        port: a.port,
        user: user_final,
        key_path: a.identity.map(Into::into),
        password: a.password,
        key_passphrase: a.passphrase,
    };

    // 3) Verbinden
    let sess = match StarrSession::connect(&prof) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Verbindungsfehler: {e}");
            std::process::exit(1);
        }
    };

    // 4) stdin → remote
    let _writer = {
        let s = sess.weak_clone();
        thread::spawn(move || {
            let mut inb = io::stdin();
            let mut tmp = [0u8; 4096];
            loop {
                match inb.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = s.send(std::str::from_utf8(&tmp[..n]).unwrap_or_default());
                    }
                    Err(_) => break,
                }
            }
        })
    };

    // 5) remote → stdout (einfaches Polling)
    loop {
        let out = sess.read_string();
        if !out.is_empty() {
            print!("{out}");
        }
        thread::sleep(Duration::from_millis(25));
    }

    // (nie erreicht; Ctrl+C beendet)
    // Ok(())
}
