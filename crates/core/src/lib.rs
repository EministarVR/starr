use anyhow::{anyhow, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write, ErrorKind};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarrProfile {
    pub host: String,
    pub port: u16,
    pub user: String,
    /// OpenSSH-Key (PPK konvertieren oder später implementieren)
    pub key_path: Option<PathBuf>,
    /// Passwort (nur wenn kein Key)
    pub password: Option<String>,
    /// Passphrase für verschlüsselte OpenSSH-Keys
    pub key_passphrase: Option<String>,
}

pub struct StarrSession {
    inner: Arc<Mutex<ssh2::Session>>,
    chan: Arc<Mutex<ssh2::Channel>>,
    /// Puffer für stdout/stderr (simpel, aber funktioniert)
    buf: Arc<Mutex<Vec<u8>>>,
    reader_join: Option<thread::JoinHandle<()>>,
}

impl Drop for StarrSession {
    fn drop(&mut self) {
        if let Ok(mut ch) = self.chan.lock() {
            let _ = ch.send_eof();
            let _ = ch.wait_close();
        }
    }
}

impl StarrSession {
    /// Öffnet SSH, PTY und Shell, startet Reader-Thread.
    pub fn connect(p: &StarrProfile) -> Result<Self> {
        let addr = format!("{}:{}", p.host, p.port);
        let tcp = TcpStream::connect(addr)?;
        tcp.set_nodelay(true)?;
        tcp.set_read_timeout(Some(Duration::from_millis(100)))?;

        // FIX 1: Session::new() -> Result, kein Option
        let mut sess = ssh2::Session::new().map_err(|e| anyhow!("Session new() failed: {e}"))?;
        sess.set_tcp_stream(tcp);
        sess.handshake()?;

        // Auth
        if let Some(ref key) = p.key_path {
            sess.userauth_pubkey_file(
                &p.user,
                None,
                key,
                p.key_passphrase.as_deref(),
            )?;
        } else if let Some(ref pw) = p.password {
            sess.userauth_password(&p.user, pw)?;
        } else {
            return Err(anyhow!("Kein Auth-Material (Key oder Passwort) angegeben"));
        }

        if !sess.authenticated() {
            return Err(anyhow!("Auth fehlgeschlagen"));
        }

        // PTY + Shell
        let mut ch = sess.channel_session()?;
        ch.request_pty("xterm", None, Some((80, 24, 0, 0)))?;
        ch.shell()?;

        let sess_arc = Arc::new(Mutex::new(sess));
        let ch_arc = Arc::new(Mutex::new(ch));
        let buf = Arc::new(Mutex::new(Vec::<u8>::new()));

        // Reader-Thread (stdout/stderr)
        let reader_buf = buf.clone();
        let ch_for_read = ch_arc.clone();
        let handle = thread::spawn(move || {
            let mut tmp = [0u8; 4096];
            loop {
                // FIX 2: Kein Pattern-Guard; normal behandeln
                let n = {
                    let mut guard = ch_for_read.lock().unwrap();
                    match guard.read(&mut tmp) {
                        Ok(0) => break,                 // Channel zu
                        Ok(n) => n,                     // Daten gelesen
                        Err(e) => {
                            if e.kind() == ErrorKind::WouldBlock {
                                0
                            } else {
                                break
                            }
                        }
                    }
                };

                if n > 0 {
                    let mut b = reader_buf.lock().unwrap();
                    b.extend_from_slice(&tmp[..n]);
                } else {
                    thread::sleep(Duration::from_millis(30));
                }
            }
        });

        Ok(Self {
            inner: sess_arc,
            chan: ch_arc,
            buf,
            reader_join: Some(handle),
        })
    }

    /// Dupliziert nur die Handles (keine zweite Reader-Loop).
    pub fn weak_clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            chan: self.chan.clone(),
            buf: self.buf.clone(),
            reader_join: None,
        }
    }

    /// Sendet eine Zeile (fügt kein \n hinzu – selbst anhängen!)
    pub fn send(&self, data: &str) -> Result<()> {
        let mut ch = self.chan.lock().unwrap();
        ch.write_all(data.as_bytes())?;
        ch.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u32, rows: u32) -> Result<()> {
        let mut ch = self.chan.lock().unwrap();
        ch.request_pty_size(cols, rows, None, None)?;
        Ok(())
    }

    /// Holt den aktuell gepufferten Output und leert den Puffer.
    pub fn read_string(&self) -> String {
        let mut b = self.buf.lock().unwrap();
        let s = String::from_utf8_lossy(&b).to_string();
        b.clear();
        s
    }

    pub fn close(mut self) -> Result<()> {
        if let Ok(mut ch) = self.chan.lock() {
            let _ = ch.send_eof();
            let _ = ch.wait_close();
        }
        if let Some(h) = self.reader_join.take() {
            let _ = h.join();
        }
        Ok(())
    }
}

/// Konfig-Pfad: %APPDATA%\Starr\config.toml
pub fn config_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("dev", "Eministar", "Starr")
        .ok_or_else(|| anyhow!("ProjectDirs not available"))?;
    let path = dirs.config_dir().to_path_buf();
    std::fs::create_dir_all(&path)?;
    Ok(path)
}
