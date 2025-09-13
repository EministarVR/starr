#![cfg_attr(windows, windows_subsystem = "windows")] // keine extra Konsole

use eframe::egui;
use egui::{text::LayoutJob, Color32, FontId, Id, TextFormat};
use starr_core::{StarrProfile, StarrSession};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/* ---------- Worker-IPC ---------- */

#[derive(Debug)]
enum ToWorker {
    SendText(String),
    Resize(u32, u32),
    Close,
}

#[derive(Debug)]
enum FromWorker {
    ConnectedOk,
    ConnectedErr(String),
    Data(String),
    Closed(String),
}

/* ---------- App ---------- */

pub struct App {
    // Connect-Form
    host: String,
    port: u16,
    user: String,
    key_path: String,
    passphrase: String,
    password: String,

    // State
    connected: bool,
    connect_error: Option<String>,
    tx: Option<mpsc::Sender<ToWorker>>,
    rx: Option<mpsc::Receiver<FromWorker>>,

    // Terminal
    view_buf: String,      // echter Output-Buffer (nur Worker schreibt)
    display_buf: String,   // Anzeige-Puffer fürs Widget (wir ändern den nur, wenn view_buf sich ändert)
    term_id: Id,

    // ANSI-Cache + Drosselung
    ansi_job: LayoutJob,
    ansi_dirty: bool,
    last_ansi_build: Instant,

    // Fokus & Layout
    want_focus: bool,
    autoscroll: bool,
    last_cols: u32,
    last_rows: u32,

    // Input
    input_buf: String, 
    local_echo: bool, 
}

impl Default for App {
    fn default() -> Self {
        Self {
            host: "localhost".into(),
            port: 22,
            user: whoami::username(),
            key_path: String::new(),
            passphrase: String::new(),
            password: String::new(),

            connected: false,
            connect_error: None,
            tx: None,
            rx: None,

            view_buf: String::new(),
            display_buf: String::new(),
            term_id: Id::new("starr-terminal"),

            ansi_job: LayoutJob::default(),
            ansi_dirty: true,
            last_ansi_build: Instant::now(),

            want_focus: false,
            autoscroll: true,
            last_cols: 0,
            last_rows: 0,
            input_buf: String::new(),
            local_echo: true,  
        }
    }
}

fn main() {
    let mut native_options = eframe::NativeOptions::default();
    native_options.viewport = egui::ViewportBuilder::default()
        .with_inner_size([980.0, 640.0])
        .with_title("Starr");
    eframe::run_native(
        "Starr",
        native_options,
        Box::new(|_cc| Ok(Box::new(App::default()))),
    )
    .ok();
}

/* ---------- GUI ---------- */

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_visuals(egui::Visuals::dark());

        poll_worker(self);

        // Header
        egui::TopBottomPanel::top("bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Starr");
                ui.separator();
                ui.label(if self.connected { "Verbunden" } else { "Getrennt" });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(&mut self.autoscroll, "Autoscroll");
                });
            });
            if let Some(e) = &self.connect_error {
                ui.colored_label(Color32::RED, format!("⚠ {e}"));
            }
        });

        if !self.connected && self.tx.is_none() {
            connect_card(self, ctx);
        } else {
            terminal_view(self, ctx);
        }

        // 50 ms → deutlich weniger GPU als 16 ms
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(ToWorker::Close);
        }
    }
}

/* ---------- Panels ---------- */

fn connect_card(app: &mut App, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(ui.available_height() * 0.1);
        ui.vertical_centered(|ui| {
            ui.set_min_width(420.0);
            ui.heading("Verbinden");
            ui.separator();
            ui.label("Host");
            let host_resp = ui.text_edit_singleline(&mut app.host);
            ui.label("Port");
            ui.add(egui::DragValue::new(&mut app.port).range(1..=65535));
            ui.label("Benutzer");
            ui.text_edit_singleline(&mut app.user);
            ui.label("Key (optional)");
            ui.text_edit_singleline(&mut app.key_path);
            ui.label("Passphrase");
            ui.text_edit_singleline(&mut app.passphrase);
            ui.label("oder Passwort");
            ui.add(egui::TextEdit::singleline(&mut app.password).password(true));
            ui.add_space(10.0);

            let go = ui.button("Verbinden").clicked()
                || (host_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));

            if go {
                start_worker(app);
            }
        });
    });
}

fn terminal_view(app: &mut App, ctx: &egui::Context) {
    // display_buf aktualisieren, wenn neuer Output kam
    if app.display_buf != app.view_buf {
        app.display_buf = app.view_buf.clone();
        app.ansi_dirty = true;
    }
    // ANSI-Layout nur bei Bedarf/throttled neu bauen
    if app.ansi_dirty && app.last_ansi_build.elapsed() >= Duration::from_millis(50) {
        app.ansi_job = ansi_to_layout_job(&app.display_buf);
        app.last_ansi_build = Instant::now();
        app.ansi_dirty = false;
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(Color32::from_rgb(10, 10, 14)))
        .show(ctx, |ui| {
            // 1) Reines Anzeige-Widget: NICHT interaktiv, damit es nicht gegen den Output puffert
            let mut text = app.display_buf.as_str();
            let te = egui::TextEdit::multiline(&mut text)
                .id(app.term_id)
                .font(egui::TextStyle::Monospace)
                .code_editor()
                .interactive(false)        // <- read-only Anzeige
                .cursor_at_end(true)
                .desired_width(f32::INFINITY)
                .desired_rows(30)
                .layouter(&mut |ui, _t, _| ui.fonts(|f| f.layout_job(app.ansi_job.clone())))
                .show(ui);

            // 2) Fokus aufs Terminal, damit globales Keyboard-Capture aktiv ist
            if app.want_focus {
                te.response.request_focus();
                app.want_focus = false;
            }

            // 3) Tastatur/Paste global abgreifen und an Worker senden
            handle_input_and_send(app, ctx);

            // 4) Auswahl → Auto-Copy (wie PuTTY)
            if let Some(cr) = te.cursor_range {
                if ui.input(|i| i.pointer.any_released()) {
                    let c = cr.as_ccursor_range();
                    if c.primary.index != c.secondary.index {
                        let start = c.primary.index.min(c.secondary.index);
                        let end = c.primary.index.max(c.secondary.index);
                        if let Some(slice) = safe_slice(&app.display_buf, start, end) {
                            copy_to_clipboard(slice);
                        }
                    }
                }
            }

            // 5) Rechtsklick / Middle-Click = Paste+Send
            te.response.context_menu(|ui| {
               if ui.button("Einfügen & Senden").clicked() {
    if let Some(txt) = paste_from_clipboard() {
        let do_echo = app.local_echo;
        if do_echo { append_local_echo(app, &txt); }
        if let Some(tx) = app.tx.as_ref().cloned() {
            let _ = tx.send(ToWorker::SendText(txt));
        }
    }
    ui.close_menu();
}
                if ui.button("Alles kopieren").clicked() {
                    copy_to_clipboard(&app.display_buf);
                    ui.close_menu();
                }
                ui.separator();
                ui.checkbox(&mut app.local_echo, "Lokales Echo");
            });
            if te.response.middle_clicked() {
    if let Some(txt) = paste_from_clipboard() {
        let do_echo = app.local_echo;
        if do_echo { append_local_echo(app, &txt); }
        if let Some(tx) = app.tx.as_ref().cloned() {
            let _ = tx.send(ToWorker::SendText(txt));
        }
    }
}

            // 6) Ctrl+Shift+C = alles kopieren (Ctrl+C NICHT abfangen!)
            let (ctrl, shift) = ctx.input(|i| (i.modifiers.ctrl || i.modifiers.command, i.modifiers.shift));
            if ctrl && shift && ctx.input(|i| i.key_pressed(egui::Key::C)) {
                copy_to_clipboard(&app.display_buf);
            }

            // 7) Resize → Worker
            if let Some(tx) = &app.tx {
                let rect = te.response.rect;
                let char_w = ui.fonts(|f| f.glyph_width(&FontId::monospace(15.0), 'W')).max(8.0);
                let char_h = ui.text_style_height(&egui::TextStyle::Monospace).max(12.0);
                let cols = ((rect.width() - 8.0) / char_w).max(20.0) as u32;
                let rows = ((rect.height() - 8.0) / char_h).max(5.0) as u32;
                if cols != app.last_cols || rows != app.last_rows {
                    let _ = tx.send(ToWorker::Resize(cols, rows));
                    app.last_cols = cols;
                    app.last_rows = rows;
                }
            }
        });
}

fn handle_input_and_send(app: &mut App, ctx: &egui::Context) {
    let Some(tx) = app.tx.as_ref().cloned() else { return; };

    // Eingabe-Events einsammeln
    let mut to_send = String::new();
    for ev in ctx.input(|i| i.events.clone()) {
        use egui::Event::*;
        match ev {
            Text(t) => {
                if !t.is_empty() { to_send.push_str(&t); }
            }
            Key { key, pressed, modifiers, .. } if pressed => {
                if let Some(seq) = map_key(key, modifiers) {
                    to_send.push_str(&seq);
                }
            }
            _ => {}
        }
    }

    if to_send.is_empty() { return; }

    // Optional: lokales Echo, damit du Tippen SOFORT siehst
    if app.local_echo {
        append_local_echo(app, &to_send);
    }

 let _ = tx.send(ToWorker::SendText(to_send));
}

// Hängt lokal an den View-Buffer + markiert ANSI dirty
fn append_local_echo(app: &mut App, s: &str) {
    append_and_limit(&mut app.view_buf, s, 200_000); // 200KB Limit
    app.ansi_dirty = true;
}

/* ---------- Worker ---------- */

fn start_worker(app: &mut App) {
    app.connect_error = None;

    if app.host.trim().is_empty() {
        app.connect_error = Some("Host darf nicht leer sein.".into());
        return;
    }
    if app.user.trim().is_empty() {
        app.connect_error = Some("Benutzer darf nicht leer sein.".into());
        return;
    }

    let profile = StarrProfile {
        host: app.host.clone(),
        port: app.port,
        user: app.user.clone(),
        key_path: if app.key_path.is_empty() { None } else { Some(app.key_path.clone().into()) },
        password: if app.password.is_empty() { None } else { Some(app.password.clone()) },
        key_passphrase: if app.passphrase.is_empty() { None } else { Some(app.passphrase.clone()) },
    };

    let (tx_cmd, rx_cmd) = mpsc::channel::<ToWorker>();
    let (tx_evt, rx_evt) = mpsc::channel::<FromWorker>();

    thread::spawn(move || {
        let sess = match StarrSession::connect(&profile) {
            Ok(s) => { let _ = tx_evt.send(FromWorker::ConnectedOk); s }
            Err(e) => { let _ = tx_evt.send(FromWorker::ConnectedErr(e.to_string())); return; }
        };

        let _ = sess.resize(120, 34);
        let mut last = Instant::now();

        loop {
            // Commands
            while let Ok(cmd) = rx_cmd.try_recv() {
                match cmd {
                    ToWorker::SendText(t) => { let _ = sess.send(&t); }
                    ToWorker::Resize(c, r) => { let _ = sess.resize(c, r); }
                    ToWorker::Close => { let _ = tx_evt.send(FromWorker::Closed("geschlossen".into())); return; }
                }
            }

            // Output poll
            let data = sess.read_string();
            if !data.is_empty() {
                let _ = tx_evt.send(FromWorker::Data(data));
                last = Instant::now();
            } else {
                thread::sleep(Duration::from_millis(10));
                if last.elapsed() > Duration::from_secs(3600) {
                    let _ = tx_evt.send(FromWorker::Closed("timeout".into()));
                    return;
                }
            }
        }
    });

    app.tx = Some(tx_cmd);
    app.rx = Some(rx_evt);
    app.want_focus = true;
}

/* ---------- Utils ---------- */

fn poll_worker(app: &mut App) {
    let mut drop_rx = false;
    if let Some(rx) = app.rx.as_ref() {
        loop {
            match rx.try_recv() {
                Ok(FromWorker::ConnectedOk) => {
                    app.connected = true;
                    app.connect_error = None;
                    app.view_buf.clear();
                    app.display_buf.clear();
                    app.ansi_job = LayoutJob::default();
                    app.ansi_dirty = true;
                    app.last_ansi_build = Instant::now();
                    app.want_focus = true;
                }
                Ok(FromWorker::ConnectedErr(e)) => {
                    app.connected = false;
                    app.connect_error = Some(e);
                    app.tx = None;
                    drop_rx = true;
                    break;
                }
                Ok(FromWorker::Data(chunk)) => {
                    // 200 KB Limit → deutlich weniger GPU
                    append_and_limit(&mut app.view_buf, &chunk, 200_000);
                    app.ansi_dirty = true;
                }
                Ok(FromWorker::Closed(msg)) => {
                    app.connected = false;
                    app.connect_error = Some(format!("Verbindung beendet: {msg}"));
                    app.tx = None;
                    drop_rx = true;
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    app.connected = false;
                    app.tx = None;
                    drop_rx = true;
                    break;
                }
            }
        }
    }
    if drop_rx {
        app.rx = None;
    }
}

/// Hängt `chunk` an und kappt am Anfang, wenn `max_len` überschritten.
fn append_and_limit(buf: &mut String, chunk: &str, max_len: usize) {
    buf.push_str(chunk);
    if buf.len() > max_len {
        let cut = buf.len() - max_len;
        // an char-Grenze schneiden:
        let mut cut_b = cut;
        for (i, _) in buf.char_indices() {
            if i >= cut { cut_b = i; break; }
        }
        buf.drain(..cut_b);
    }
}

/// Sichere UTF-8 Scheibe aus char-Indizes.
fn safe_slice(s: &str, start_char: usize, end_char: usize) -> Option<&str> {
    let to_byte = |s: &str, cidx: usize| {
        if cidx == 0 { return 0; }
        let mut count = 0usize;
        for (i, _) in s.char_indices() {
            if count == cidx { return i; }
            count += 1;
        }
        s.len()
    };
    let b0 = to_byte(s, start_char);
    let b1 = to_byte(s, end_char);
    if b0 <= b1 && b1 <= s.len() { Some(&s[b0..b1]) } else { None }
}

/// ANSI → LayoutJob (SGR 0, 30–37, 90–97)
fn ansi_to_layout_job(s: &str) -> LayoutJob {
    use ansi_parser::{AnsiParser, AnsiSequence, Output};
    let mut job = LayoutJob::default();
    let mut color = Color32::from_rgb(230, 230, 230);
    let font = FontId::monospace(15.0);
    let mut fmt = TextFormat { font_id: font.clone(), color, ..Default::default() };

    for item in s.ansi_parse() {
        match item {
            Output::TextBlock(txt) => job.append(&txt, 0.0, fmt.clone()),
            Output::Escape(AnsiSequence::SetGraphicsMode(params)) => {
                for p in params {
                    match p as u8 {
                        0  => { color = Color32::from_rgb(230,230,230); fmt.color = color; }
                        30 => { color = Color32::from_rgb(0,0,0);      fmt.color = color; }
                        31 => { color = Color32::from_rgb(205,49,49);  fmt.color = color; }
                        32 => { color = Color32::from_rgb(13,188,121); fmt.color = color; }
                        33 => { color = Color32::from_rgb(229,229,16); fmt.color = color; }
                        34 => { color = Color32::from_rgb(36,114,200); fmt.color = color; }
                        35 => { color = Color32::from_rgb(188,63,188); fmt.color = color; }
                        36 => { color = Color32::from_rgb(17,168,205); fmt.color = color; }
                        37 => { color = Color32::from_rgb(229,229,229);fmt.color = color; }
                        90 => { color = Color32::from_rgb(102,102,102);fmt.color = color; }
                        91 => { color = Color32::from_rgb(241,76,76);  fmt.color = color; }
                        92 => { color = Color32::from_rgb(35,209,139); fmt.color = color; }
                        93 => { color = Color32::from_rgb(245,245,67); fmt.color = color; }
                        94 => { color = Color32::from_rgb(59,142,234); fmt.color = color; }
                        95 => { color = Color32::from_rgb(214,112,214);fmt.color = color; }
                        96 => { color = Color32::from_rgb(41,184,219); fmt.color = color; }
                        97 => { color = Color32::from_rgb(255,255,255);fmt.color = color; }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    job
}

/// Keyboard → xterm-Sequenzen (Ctrl+C/D/Z NICHT abfangen)
fn map_key(k: egui::Key, m: egui::Modifiers) -> Option<String> {
    use egui::Key::*;
    if m.ctrl || m.command {
        return match k {
            V => paste_from_clipboard(),
            // C/D/Z NICHT abfangen -> None
            _ => None,
        };
    }
    match k {
        Enter => Some("\r".into()),
        Tab => Some("\t".into()),
        Backspace => Some("\x7f".into()),
        Delete => Some("\x1b[3~".into()),
        ArrowUp => Some("\x1b[A".into()),
        ArrowDown => Some("\x1b[B".into()),
        ArrowRight => Some("\x1b[C".into()),
        ArrowLeft => Some("\x1b[D".into()),
        Home => Some("\x1b[H".into()),
        End => Some("\x1b[F".into()),
        PageUp => Some("\x1b[5~".into()),
        PageDown => Some("\x1b[6~".into()),
        _ => None,
    }
}

fn copy_to_clipboard(text: &str) {
    #[cfg(windows)]
    let _ = clipboard_win::set_clipboard_string(text);
}

fn paste_from_clipboard() -> Option<String> {
    #[cfg(windows)]
    { clipboard_win::get_clipboard_string().ok() }
    #[cfg(not(windows))]
    { None }
}
