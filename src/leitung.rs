//! Die Live-Leitung zu AI Brain (#529/#530).
//!
//! **Was sie tut.** Der Rechner baut AUSGEHEND eine WebSocket-Verbindung zu
//! Reverb auf, hört auf seinem eigenen Eingabe-Kanal und reicht ein imkr-Pane
//! durch: was das Pane zeigt, geht als gebündelte Ausgabe per POST an AI
//! Brain, was im Browser getippt wird, kommt als `geraet.eingabe` zurück.
//! Brain ruft auf dem Rechner nie etwas auf — dieselbe Vertrauensrichtung wie
//! bei den Aufträgen, und nur so geht es auch hinter NAT.
//!
//! ```text
//! imkr tap PANE --max 40000            Brain (/api/v1/geraete/…)        Reverb
//!   screen/out/gap/exit  ──▶ Bündler ──▶ POST ausgabe  ──▶ Broadcast ──▶ Browser
//!   stdin {"input"|"screen"} ◀─────────────────────────── geraet.eingabe ◀─┘
//! ```
//!
//! ## Der Schalter ist lokal, Standard AUS, und läuft ab
//!
//! Ohne gültigen Schalter wird nicht einmal der Kanal erfragt. Der Schalter
//! ist eine Datei mit einem Ablauf (`leitung.json`, `{"bis": <unix>}`); fehlt
//! sie, ist sie unlesbar, abgelaufen oder liegt ihr Ablauf weiter als die
//! Höchstdauer in der Zukunft, gilt AUS. Dieselbe Haltung wie bei den Rechten
//! in `rechner.rs`: was nicht ausdrücklich erlaubt ist, ist es nicht.
//!
//! Die Leitung prüft den Schalter laufend (bei jedem Lesetakt, 100 ms) und legt **selbst** auf, wenn er
//! fällt. Ein Ablauf, den nur die Gegenseite durchsetzt, ist keiner — fällt
//! die Gegenseite aus, bliebe das Terminal offen.
//!
//! ## Was die Leitung bewusst NICHT tut
//!
//! * **Keine fremden Rechner.** Ein Pane heisst `sitzung/nummer`. Eine Adresse
//!   mit Rechnernamen (`server:api/1`) würde imkr über diesen Rechner hinaus
//!   weiterreichen — der Besitz dieses Geräts ist aber keine Erlaubnis für
//!   jenes. Solche Adressen werden abgelehnt.
//! * **Keine Tastendrücke im Protokoll.** Auf diesem Konto werden auch
//!   Passwörter getippt; ein Mitschnitt im Log wäre einer, den niemand
//!   bestellt hat. Aus demselben Grund steht der Text eines Befehls nicht im
//!   Log, nur sein Auftrag.
//! * **Ein Pane je Gerät.** Kommt ein Ereignis für ein anderes Pane, wird das
//!   alte beendet. Zwei gleichzeitige Taps sähe der Browser durcheinander.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::config::{self, Connection};
use crate::imkr::{self, PaneZustand};
use crate::mcp;
use crate::pusher::{self, Verbindung};

/// Was eine ganze Ausgabe-Nutzlast höchstens wiegen darf (`BLOCK_GRENZE_BYTES`
/// in Brain). Darüber antwortet Brain mit 422.
pub const GRENZE_BLOCK: usize = 60_000;

/// So viele Rohbytes gehen höchstens in einen Block.
///
/// Gewählt wird je Block die kürzere Form aus `text` und `b64`, also wiegt der
/// kodierte Teil höchstens so viel wie base64: 40 000 · 4/3 ≈ 53 334 Bytes.
/// Der Rest bis 60 000 ist Platz für Pane, seq und Größe. Ein Test hält die
/// Rechnung fest.
const ROH_BLOCK: usize = 40_000;

/// Was `imkr tap` höchstens auf einmal liefert, bevor es eine Lücke meldet.
const TAP_MAX: &str = "40000";

/// Die Befehlsausgabe, die zurückgeht, höchstens.
const GRENZE_ERGEBNIS: usize = 16 * 1024;

/// Länger lässt sich die Leitung nicht auf einmal einschalten.
const HOECHSTDAUER: i64 = 8 * 3600;
const VORGABE_DAUER: i64 = 30 * 60;

const FRIST_HTTP: Duration = Duration::from_secs(15);

/// Vorgabe und Obergrenze einer Befehlsfrist, wie bei `rechner-befehl`.
const FRIST_BEFEHL_VORGABE: u64 = 60;
const FRIST_BEFEHL_HOECHST: u64 = 600;

const HILFE: &str = "\
Live-Leitung zu AI Brain: ein imkr-Pane im Browser zeigen und steuern.

Aufrufe:
  ai-brain-harness leitung an [--dauer 30m]   einschalten (hoechstens 8h; s, m oder h)
  ai-brain-harness leitung aus                ausschalten und auflegen
  ai-brain-harness leitung status             Schalter und Verbindung zeigen
  ai-brain-harness leitung laufen             die Leitung im Vordergrund halten

Standard ist AUS. Ohne gueltigen Schalter baut der Rechner keine Verbindung
auf; laeuft der Schalter ab, legt er selbst auf. Welches Pane gezeigt wird,
bestimmt AI Brain — aber nur Panes auf diesem Rechner.
";

// ---------------------------------------------------------------------------
// Der Schalter
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct Schalter {
    bis: i64,
}

/// Bis wann der Schalter gilt — oder `None`, wenn er AUS ist.
///
/// Rein, damit die Regel ohne Uhr und Dateisystem prüfbar ist. Ein Ablauf
/// jenseits der Höchstdauer gilt als AUS, nicht als „gekappt": Den hat
/// niemand über `leitung an` gesetzt, und eine Datei, die von Hand verlängert
/// wurde, ist genau der Fall, gegen den ein Ablauf schützen soll.
pub fn schalter_bis(roh: Option<&str>, jetzt: i64) -> Option<i64> {
    let schalter: Schalter = serde_json::from_str(roh?).ok()?;

    (schalter.bis > jetzt && schalter.bis <= jetzt + HOECHSTDAUER).then_some(schalter.bis)
}

fn ablage(name: &str) -> Result<PathBuf, String> {
    Ok(config::config_dir()?.join(name))
}

fn schalter_lesen() -> Option<i64> {
    let roh = ablage("leitung.json")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok());

    schalter_bis(roh.as_deref(), jetzt())
}

fn schalter_gilt() -> bool {
    schalter_lesen().is_some()
}

/// Liest eine Dauer: `90s`, `30m`, `2h`; eine blosse Zahl sind Minuten.
pub fn dauer_lesen(text: &str) -> Result<i64, String> {
    let text = text.trim();
    let (zahl, faktor) = match text.chars().last() {
        Some('s') => (&text[..text.len() - 1], 1),
        Some('m') => (&text[..text.len() - 1], 60),
        Some('h') => (&text[..text.len() - 1], 3600),
        _ => (text, 60),
    };

    let sekunden = zahl
        .parse::<i64>()
        .ok()
        .filter(|z| *z > 0)
        .map(|z| z * faktor)
        .ok_or_else(|| format!("„{text}“ ist keine Dauer (z. B. 30m, 2h, 90s)."))?;

    if sekunden > HOECHSTDAUER {
        return Err(format!(
            "Höchstens {} Stunden am Stück — danach neu einschalten.",
            HOECHSTDAUER / 3600
        ));
    }

    Ok(sekunden)
}

// ---------------------------------------------------------------------------
// Unterbefehle
// ---------------------------------------------------------------------------

pub fn verwalten(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("an") => an(&args[1..]),
        Some("aus") => aus(),
        Some("status") | None => status(),
        Some("laufen") => laufen(),
        Some("--help") | Some("-h") | Some("help") => {
            print!("{HILFE}");
            Ok(())
        }
        Some(anders) => Err(format!("Unbekannt: leitung {anders}\n\n{HILFE}")),
    }
}

fn an(args: &[String]) -> Result<(), String> {
    let mut dauer = VORGABE_DAUER;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--dauer" => {
                let wert = args
                    .get(i + 1)
                    .ok_or("--dauer braucht einen Wert, z. B. 30m.")?;
                dauer = dauer_lesen(wert)?;
                i += 2;
            }
            anders => return Err(format!("Unbekannt: {anders}\n\n{HILFE}")),
        }
    }

    // Vor dem Schalter: ohne Verbindung gäbe es nur einen Schalter, der an ist
    // und nichts tut.
    config::load()?;

    let bis = jetzt() + dauer;
    let pfad = ablage("leitung.json")?;

    if let Some(ordner) = pfad.parent() {
        std::fs::create_dir_all(ordner)
            .map_err(|e| format!("{} ist nicht anlegbar: {e}", ordner.display()))?;
    }

    std::fs::write(
        &pfad,
        serde_json::to_string(&Schalter { bis }).unwrap_or_default(),
    )
    .map_err(|e| format!("{} ist nicht schreibbar: {e}", pfad.display()))?;

    if let Some(pid) = laufende_pid() {
        println!(
            "Leitung läuft schon (Prozess {pid}), jetzt an bis {} UTC.",
            uhrzeit(bis)
        );
        return Ok(());
    }

    let pid = abgekoppelt_starten()?;

    println!("Leitung an bis {} UTC (Prozess {pid}).", uhrzeit(bis));
    println!("Protokoll: {}", ablage("leitung.log")?.display());

    Ok(())
}

fn aus() -> Result<(), String> {
    let pfad = ablage("leitung.json")?;

    match std::fs::remove_file(&pfad) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{} lässt sich nicht löschen: {e}", pfad.display())),
    }

    let Some(pid) = laufende_pid() else {
        println!("Leitung ist aus.");
        return Ok(());
    };

    // Erst die Leitung selbst auflegen lassen: sie sieht den Schalter binnen
    // einer Sekunde fallen, beendet den Tap und schickt den Endemarker. Ein
    // Kill schnitte das ab, und der Browser sähe ein Terminal, das einfriert.
    let beginn = Instant::now();

    while beginn.elapsed() < Duration::from_secs(8) {
        if !prozess_lebt(pid) {
            println!("Leitung ist aus, Verbindung beendet.");
            return Ok(());
        }

        std::thread::sleep(Duration::from_millis(200));
    }

    signal_beenden(pid);
    let _ = std::fs::remove_file(ablage("leitung.pid")?);

    println!("Leitung ist aus (Prozess {pid} reagierte nicht und wurde beendet).");

    Ok(())
}

fn status() -> Result<(), String> {
    match schalter_lesen() {
        Some(bis) => println!("Schalter : an bis {} UTC", uhrzeit(bis)),
        None => println!("Schalter : aus"),
    }

    match laufende_pid() {
        Some(pid) => println!("Prozess  : läuft ({pid})"),
        None if schalter_gilt() => {
            println!("Prozess  : läuft nicht — Schalter an, aber aufgelegt; Grund im Protokoll")
        }
        None => println!("Prozess  : läuft nicht"),
    }

    println!("Protokoll: {}", ablage("leitung.log")?.display());

    Ok(())
}

// ---------------------------------------------------------------------------
// Der Prozess
// ---------------------------------------------------------------------------

/// Startet `leitung laufen` abgekoppelt vom Terminal.
///
/// Eine eigene Prozessgruppe statt `setsid`: das gibt es auf macOS nicht als
/// Befehl, `process_group(0)` aber überall unter Unix. Ctrl+C im Terminal, aus
/// dem `leitung an` kam, trifft die Leitung damit nicht.
fn abgekoppelt_starten() -> Result<u32, String> {
    let programm = std::env::current_exe().map_err(|e| format!("Eigener Pfad unbekannt: {e}"))?;
    let protokoll = ablage("leitung.log")?;

    let datei = std::fs::File::create(&protokoll)
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", protokoll.display()))?;
    let fehler = datei
        .try_clone()
        .map_err(|e| format!("Protokoll lässt sich nicht doppeln: {e}"))?;

    let mut befehl = Command::new(programm);
    befehl
        .args(["leitung", "laufen"])
        .stdin(Stdio::null())
        .stdout(Stdio::from(datei))
        .stderr(Stdio::from(fehler));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        befehl.process_group(0);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        befehl.creation_flags(DETACHED_PROCESS);
    }

    let kind = befehl
        .spawn()
        .map_err(|e| format!("Die Leitung liess sich nicht starten: {e}"))?;

    Ok(kind.id())
}

/// Die Nummer der laufenden Leitung, falls eine läuft.
///
/// Der Merker schreibt `laufen` selbst — so zählt auch eine Leitung, die
/// jemand von Hand im Vordergrund hält.
fn laufende_pid() -> Option<u32> {
    let text = std::fs::read_to_string(ablage("leitung.pid").ok()?).ok()?;
    let pid: u32 = text.trim().parse().ok()?;

    prozess_lebt(pid).then_some(pid)
}

fn prozess_lebt(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
}

/// SIGTERM, nicht SIGKILL: `Child::kill` schickt unter Unix KILL, und dann
/// räumt imkr den Tap nicht auf.
fn signal_beenden(pid: u32) {
    #[cfg(unix)]
    let _ = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    #[cfg(not(unix))]
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Räumt den Merker auf, wenn `laufen` endet — auch bei einem Fehler.
struct Merker(PathBuf);

impl Drop for Merker {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

enum Abbruch {
    /// Neu verbinden lohnt nicht (403: Besitz oder Recht fehlt).
    Endgueltig(String),
    /// Neu verbinden, nach einer Pause.
    Wieder(String),
}

fn laufen() -> Result<(), String> {
    if !schalter_gilt() {
        return Err(
            "Die Leitung ist ausgeschaltet. Einschalten: ai-brain-harness leitung an [--dauer 30m]"
                .to_string(),
        );
    }

    if let Some(pid) = laufende_pid().filter(|p| *p != std::process::id()) {
        return Err(format!(
            "Eine Leitung läuft schon (Prozess {pid}). Eine je Gerät."
        ));
    }

    let merker_pfad = ablage("leitung.pid")?;
    std::fs::write(&merker_pfad, format!("{}\n", std::process::id()))
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", merker_pfad.display()))?;
    let _merker = Merker(merker_pfad);

    let connection = config::load()?;
    let mut steuerung = Steuerung::neu(connection.clone());
    let mut pause = 1;

    let ergebnis = loop {
        if !schalter_gilt() {
            melden("Schalter aus oder abgelaufen — lege auf.");
            break Ok(());
        }

        match sitzung(&connection, &mut steuerung, &mut pause) {
            Ok(()) => {
                melden("Schalter aus oder abgelaufen — lege auf.");
                break Ok(());
            }
            Err(Abbruch::Endgueltig(grund)) => {
                melden(&grund);
                break Err(grund);
            }
            Err(Abbruch::Wieder(grund)) => {
                melden(&format!("{grund} — neuer Versuch in {pause} s."));

                // In Sekundenschritten, damit ein Ausschalten auch während der
                // Pause sofort greift.
                for _ in 0..pause {
                    if !schalter_gilt() {
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }

                pause = (pause * 2).min(30);
            }
        }
    };

    steuerung.alles_beenden();

    ergebnis
}

/// Eine Verbindung von Aufbau bis Abbruch. `Ok` heisst: der Schalter fiel.
fn sitzung(c: &Connection, st: &mut Steuerung, pause: &mut u64) -> Result<(), Abbruch> {
    let roh = anfrage(c, "GET", "geraete/kanal", None).map_err(HttpFehler::abbruch)?;
    let kanal: KanalInfo = serde_json::from_str(&roh)
        .map_err(|e| Abbruch::Wieder(format!("Kanal-Auskunft unlesbar: {e}")))?;

    let mut verbindung = Verbindung::oeffnen(&pusher::url(
        &kanal.scheme,
        &kanal.host,
        kanal.port,
        &kanal.app_key,
    ))
    .map_err(Abbruch::Wieder)?;

    let signatur = anfrage(
        c,
        "POST",
        "geraete/kanal/auth",
        Some(
            &serde_json::json!({
                "socket_id": verbindung.socket_id,
                "channel_name": kanal.kanal_eingabe,
            })
            .to_string(),
        ),
    )
    .map_err(HttpFehler::abbruch)?;

    let auth = serde_json::from_str::<serde_json::Value>(&signatur)
        .ok()
        .and_then(|v| v.get("auth").and_then(|a| a.as_str()).map(str::to_string))
        .ok_or_else(|| Abbruch::Wieder("Signatur-Antwort ohne auth.".to_string()))?;

    verbindung
        .abonnieren(&kanal.kanal_eingabe, &auth)
        .map_err(Abbruch::Wieder)?;

    melden(&format!("verbunden, höre auf {}", kanal.kanal_eingabe));
    *pause = 1;

    {
        let c = c.clone();
        let e = st.endgueltig.clone();
        std::thread::spawn(move || seite_wenn_neu(&c, &e));
    }

    // Je Verbindung neu: nach einem Neuaufbau zählt die Gegenseite womöglich
    // anders weiter, und ein alter Puffer hielte dann Ereignisse fest.
    let mut ordner = Ordner::neu();

    loop {
        if !schalter_gilt() {
            st.alles_beenden();
            verbindung.schliessen();
            return Ok(());
        }

        st.aufraeumen();

        if let Some(grund) = st.endgueltig.lock().ok().and_then(|mut g| g.take()) {
            st.alles_beenden();
            verbindung.schliessen();
            return Err(Abbruch::Endgueltig(grund));
        }

        for daten in ordner.takt(Instant::now()) {
            st.verarbeiten(&daten);
        }

        match verbindung.naechster() {
            Ok(Some(rahmen))
                if rahmen.ereignis == "geraet.eingabe"
                    && rahmen.kanal.as_deref() == Some(kanal.kanal_eingabe.as_str()) =>
            {
                for daten in ordner.eingang(rahmen.daten, Instant::now()) {
                    st.verarbeiten(&daten);
                }
            }
            // Nur Name und Kanal, nie die Nutzlast: ein Ereignis, das hier
            // ankommt und still verpufft, sähe sonst aus wie eines, das nie kam.
            Ok(Some(rahmen)) => melden(&format!(
                "unerwartetes Ereignis „{}“ auf {} — ignoriert",
                rahmen.ereignis,
                rahmen.kanal.as_deref().unwrap_or("(ohne Kanal)")
            )),
            Ok(None) => {}
            Err(grund) => return Err(Abbruch::Wieder(grund)),
        }
    }
}

#[derive(Deserialize)]
struct KanalInfo {
    app_key: String,
    host: String,
    port: u16,
    scheme: String,
    kanal_eingabe: String,
    #[allow(dead_code)]
    kanal_ausgabe: String,
}

// ---------------------------------------------------------------------------
// HTTP zu Brain
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum HttpFehler {
    /// 403: Besitz oder Recht fehlt. Nicht im Kreis neu versuchen.
    Verboten(String),
    /// 409: Brain hat den Tap beendet — neu beginnen, mit Vollbild.
    Vorbei,
    /// 404: unbekannt, abgelaufen, fremd oder schon erledigt.
    Unbekannt(String),
    /// 422: Block zu gross (dann mit `grenze` und `gewicht`) — oder eine
    /// abgelehnte Nutzlast (Laravel-Validierung, ohne die beiden).
    ZuGross {
        grund: String,
        grenze: Option<u64>,
        gewicht: Option<u64>,
    },
    Sonst(String),
}

impl HttpFehler {
    fn abbruch(self) -> Abbruch {
        match self {
            HttpFehler::Verboten(grund) => Abbruch::Endgueltig(format!(
                "AI Brain verweigert die Leitung: {grund} — lege auf, kein neuer Versuch."
            )),
            HttpFehler::Vorbei => Abbruch::Wieder("AI Brain meldet 409.".to_string()),
            HttpFehler::ZuGross { grund, .. }
            | HttpFehler::Unbekannt(grund)
            | HttpFehler::Sonst(grund) => Abbruch::Wieder(grund),
        }
    }
}

fn api_url(c: &Connection, pfad: &str) -> String {
    format!("{}/api/v1/{pfad}", c.base_url.trim_end_matches('/'))
}

/// Was die Instanz als Grund nennt — `message` (Laravel) oder `fehler`.
fn grund(antwort: ureq::Response) -> String {
    let text = antwort.into_string().unwrap_or_default();

    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("message")
                .or_else(|| v.get("fehler"))
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .unwrap_or(text)
}

/// Eine Anfrage an `/api/v1/…`, mit einem frischen Token bei 401.
///
/// Der Rumpf geht als fertige Zeichenkette: genau die, deren Gewicht der
/// Bündler gemessen hat. `send_json` serialisierte neu — heute gleich, aber
/// die Grenze hängt an diesem einen Byte-Zähler.
fn anfrage(
    c: &Connection,
    methode: &str,
    pfad: &str,
    rumpf: Option<&str>,
) -> Result<String, HttpFehler> {
    for versuch in 0..2 {
        let token = mcp::access_token(c).map_err(HttpFehler::Sonst)?;

        let anfrage = ureq::request(methode, &api_url(c, pfad))
            .timeout(FRIST_HTTP)
            .set("Accept", "application/json")
            .set("Authorization", &format!("Bearer {token}"));

        let antwort = match rumpf {
            Some(r) => anfrage
                .set("Content-Type", "application/json")
                .send_string(r),
            None => anfrage.call(),
        };

        return match antwort {
            Ok(a) => Ok(a.into_string().unwrap_or_default()),
            Err(ureq::Error::Status(401, _)) if versuch == 0 => {
                mcp::token_verwerfen();
                continue;
            }
            Err(ureq::Error::Status(403, a)) => Err(HttpFehler::Verboten(grund(a))),
            Err(ureq::Error::Status(409, _)) => Err(HttpFehler::Vorbei),
            Err(ureq::Error::Status(404, a)) => {
                Err(HttpFehler::Unbekannt(format!("{pfad}: 404 {}", grund(a))))
            }
            Err(ureq::Error::Status(422, a)) => {
                let text = a.into_string().unwrap_or_default();
                let wert = serde_json::from_str::<serde_json::Value>(&text).ok();
                let zahl = |feld: &str| wert.as_ref()?.get(feld)?.as_u64();

                Err(HttpFehler::ZuGross {
                    grund: format!("{pfad}: 422 {text}"),
                    grenze: zahl("grenze"),
                    gewicht: zahl("gewicht"),
                })
            }
            Err(ureq::Error::Status(code, a)) => {
                Err(HttpFehler::Sonst(format!("{pfad}: {code} {}", grund(a))))
            }
            Err(e) => Err(HttpFehler::Sonst(format!("{pfad}: {e}"))),
        };
    }

    Err(HttpFehler::Sonst(format!(
        "{pfad}: 401 auch mit frischem Token."
    )))
}

// ---------------------------------------------------------------------------
// Eingabe aus Brain
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub enum Eingabe {
    /// Tastendrücke, base64 — so wie `imkr tap` sie nimmt.
    Tasten {
        pane: String,
        b64: String,
    },
    /// Größenwunsch des Browsers. Stufe 1: ignoriert, das Pane behält seine.
    Groesse,
    Erstbild {
        pane: String,
    },
    Befehl(Befehl),
    Ende {
        pane: String,
    },
    /// Herzschlag der Vollbild-Seite: solange er kommt, wird der Zustand geschoben.
    Zuschauer,
    /// Brain möchte die Seite von `imkr web page`.
    Seite,
    /// In ein Pane tippen, über `imkr web send`.
    Senden(Senden),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Senden {
    /// Brains UUID, unverändert zurückzugeben; ohne sie kein Ergebnis.
    pub auftrag: Option<serde_json::Value>,
    /// Brain schickt keinen; ein fremder Name wäre ein Fehler dort. Sonst
    /// `null` oder ein Name DIESES Geräts. Brain leitet nach Gerät weiter und
    /// setzt hier den Gerätenamen aus „Meine Geräte“, nicht den von imkr.
    pub host: Option<String>,
    pub session: String,
    pub pane: u64,
    pub text: String,
    pub enter: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Befehl {
    pub pane: String,
    pub befehl: String,
    pub frist: u64,
    /// Geht unverändert zurück — Zahl oder Zeichenkette, wie Brain sie schickt.
    pub auftrag: serde_json::Value,
}

/// Ist das eine Pane-Adresse auf DIESEM Rechner (`sitzung/nummer`)?
pub fn pane_gueltig(pane: &str) -> bool {
    let Some((sitzung, nummer)) = pane.split_once('/') else {
        return false;
    };

    !sitzung.is_empty()
        && sitzung
            .chars()
            .all(|z| z.is_ascii_alphanumeric() || matches!(z, '-' | '_' | '.'))
        && !nummer.is_empty()
        && nummer.chars().all(|z| z.is_ascii_digit())
}

/// Liest die Nutzlast eines `geraet.eingabe`.
pub fn eingabe_lesen(daten: &str) -> Result<Eingabe, String> {
    let wert: serde_json::Value =
        serde_json::from_str(daten).map_err(|e| format!("Eingabe unlesbar: {e}"))?;

    let art = wert
        .get("art")
        .and_then(|a| a.as_str())
        .ok_or("Eingabe ohne art.")?;

    match art {
        "groesse" => return Ok(Eingabe::Groesse),
        "zuschauer" => return Ok(Eingabe::Zuschauer),
        "seite" => return Ok(Eingabe::Seite),
        "senden" => return senden_lesen(&wert).map(Eingabe::Senden),
        _ => {}
    }

    let pane = wert
        .get("pane")
        .and_then(|p| p.as_str())
        .ok_or_else(|| format!("Eingabe „{art}“ ohne pane."))?;

    if !pane_gueltig(pane) {
        return Err(format!(
            "Pane „{pane}“ abgelehnt: nur sitzung/nummer auf diesem Rechner."
        ));
    }

    let pane = pane.to_string();

    match art {
        "tasten" => {
            let b64 = wert
                .get("b64")
                .or_else(|| wert.get("tasten"))
                .and_then(|t| t.as_str())
                .ok_or("Tasten ohne b64.")?;

            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|_| "Tasten sind kein gültiges base64.".to_string())?;

            Ok(Eingabe::Tasten {
                pane,
                b64: b64.to_string(),
            })
        }
        "erstbild" => Ok(Eingabe::Erstbild { pane }),
        "ende" => Ok(Eingabe::Ende { pane }),
        "befehl" => {
            // Vertrag: `befehl{befehl,frist,auftrag}`. Flach geht auch, damit
            // eine kleine Abweichung der Gegenseite nicht still verpufft.
            let innen = wert
                .get("befehl")
                .filter(|b| b.is_object())
                .unwrap_or(&wert);

            let befehl = innen
                .get("befehl")
                .and_then(|b| b.as_str())
                .filter(|b| !b.trim().is_empty())
                .ok_or("Befehl ohne Text.")?
                .to_string();

            let frist = innen
                .get("frist")
                .and_then(|f| f.as_u64())
                .unwrap_or(FRIST_BEFEHL_VORGABE)
                .clamp(1, FRIST_BEFEHL_HOECHST);

            let auftrag = innen
                .get("auftrag")
                .or_else(|| wert.get("auftrag"))
                .filter(|a| !a.is_null())
                .cloned()
                .ok_or("Befehl ohne auftrag — das Ergebnis hätte keinen Empfänger.")?;

            Ok(Eingabe::Befehl(Befehl {
                pane,
                befehl,
                frist,
                auftrag,
            }))
        }
        anders => Err(format!("Unbekannte Eingabe-Art „{anders}“.")),
    }
}

/// Der `auftrag` eines `senden`-Ereignisses, auch wenn der Rest nicht taugt.
fn senden_auftrag(daten: &str) -> Option<serde_json::Value> {
    let wert: serde_json::Value = serde_json::from_str(daten).ok()?;
    if wert.get("art")?.as_str()? != "senden" {
        return None;
    }
    wert.get("auftrag").filter(|a| !a.is_null()).cloned()
}

/// Liest ein `senden`. Das Pane kommt hier als Nummer plus Sitzung, wie
/// `imkr web send` es nimmt — nicht als `sitzung/nummer`.
fn senden_lesen(wert: &serde_json::Value) -> Result<Senden, String> {
    let auftrag = wert.get("auftrag").filter(|a| !a.is_null()).cloned();

    let session = wert
        .get("session")
        .and_then(|s| s.as_str())
        .ok_or("Senden ohne session.")?
        .to_string();

    let pane = match wert.get("pane") {
        Some(serde_json::Value::Number(n)) => n.as_u64(),
        Some(serde_json::Value::String(t)) => t.parse().ok(),
        _ => None,
    }
    .ok_or("Senden ohne gültige pane-Nummer.")?;

    if !pane_gueltig(&format!("{session}/{pane}")) {
        return Err(format!("Sitzung „{session}“ abgelehnt."));
    }

    let host = match wert.get("host") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(h)) => Some(h.clone()),
        Some(_) => return Err("host ist weder null noch ein Name.".to_string()),
    };

    let text = wert
        .get("text")
        .and_then(|t| t.as_str())
        .ok_or("Senden ohne text.")?
        .to_string();

    let enter = wert.get("enter").and_then(|e| e.as_bool()).unwrap_or(false);

    Ok(Senden {
        auftrag,
        host,
        session,
        pane,
        text,
        enter,
    })
}

// ---------------------------------------------------------------------------
// Die Web-Oberfläche von imkr (`imkr web page|state|send`)
// ---------------------------------------------------------------------------

// Vertrag der AI-Brain-Seite vom 26.10.2026 (#530).
const PFAD_ZUSTAND: &str = "geraete/zustand";
const PFAD_SEITE: &str = "geraete/seite";
/// `{auftrag, ok, error?}`; Brain zeigt `error` wortgleich an, also nur
/// Sätze, die ein Mensch lesen kann. Gemeldet wird nur mit `auftrag`.
const PFAD_SENDEN: &str = "geraete/senden-ergebnis";

/// Was die Seite wiegen darf; Brain antwortet darüber mit 422.
const GRENZE_SEITE: usize = 256_000;

/// So oft wird der Zustand gelesen; geschoben wird bei Änderung …
const ZUSTAND_PRUEFEN: Duration = Duration::from_secs(1);
/// … und spätestens nach dieser Zeit auch ohne. Brain zählt ein Gerät als
/// live, solange sein Zustand höchstens 15 s alt ist — 3 s lassen vier
/// verpasste Läufe Luft.
const ZUSTAND_TAKT: Duration = Duration::from_secs(3);
/// Brain schlägt alle 10 s; nach 30 s Stille ist die Seite zu.
const ZUSCHAUER_STILLE: Duration = Duration::from_secs(30);

/// Ruft `imkr web …` und gibt stdout zurück.
fn imkr_web(was: &str, eingabe: Option<&str>) -> Result<String, String> {
    let mut kind = Command::new("imkr")
        .args(["web", was])
        .stdin(if eingabe.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("`imkr web {was}` liess sich nicht starten: {e}"))?;

    if let (Some(text), Some(mut stdin)) = (eingabe, kind.stdin.take()) {
        let _ = stdin.write_all(text.as_bytes());
    }

    let ausgang = kind
        .wait_with_output()
        .map_err(|e| format!("`imkr web {was}`: {e}"))?;

    let aus = String::from_utf8_lossy(&ausgang.stdout).to_string();

    if !ausgang.status.success() && aus.trim().is_empty() {
        return Err(format!(
            "`imkr web {was}` scheiterte: {}",
            String::from_utf8_lossy(&ausgang.stderr).trim()
        ));
    }

    Ok(aus)
}

/// Der Zustand, wie Brain ihn bekommt: ohne fremde Rechner.
///
/// `imkr web state` zeigt auch die Sitzungen der Rechner, die imkr von hier
/// per SSH erreicht. Wer DIESES Gerät besitzt, hat damit aber keinen Zugriff
/// auf jene — also verlassen sie den Rechner gar nicht erst (Stufe 1, mit
/// dem Orchestrator und Bastian so abgestimmt). Zurück kommt dazu der Name,
/// unter dem imkr diesen Rechner kennt.
pub fn zustand_filtern(roh: &str) -> Result<(String, String), String> {
    let mut wert: serde_json::Value =
        serde_json::from_str(roh).map_err(|e| format!("imkr-Zustand unlesbar: {e}"))?;

    let rechner = wert
        .get("machine")
        .and_then(|m| m.as_str())
        .ok_or("imkr-Zustand ohne machine.")?
        .to_string();

    if let Some(obj) = wert.as_object_mut() {
        obj.insert("hosts".to_string(), serde_json::json!([]));
    }

    Ok((rechner, wert.to_string()))
}

/// Darf ein `senden` mit diesem host hier ausgeführt werden?
///
/// Erlaubt sind nur Namen DIESES Geräts: der, unter dem imkr den Rechner
/// kennt, und der Gerätename aus „Meine Geräte“ — bei Gleichnamigkeit hängt
/// Brain die ersten Zeichen der Client-ID an. Brain stellt ohnehin nur
/// Eigenes zu; geprüft wird trotzdem, denn hier endet die Leitung.
pub fn host_erlaubt(host: Option<&str>, rechner: &str, geraet: &str, client_id: &str) -> bool {
    let Some(h) = host else {
        return true;
    };

    if h == rechner || h == geraet {
        return true;
    }

    // `geraet` plus Kennung: die Zeichen danach (ohne Trenner) müssen der
    // Anfang der Client-ID sein, mindestens vier.
    let Some(rest) = h.strip_prefix(geraet) else {
        return false;
    };

    let kennung: String = rest.chars().filter(|z| z.is_ascii_alphanumeric()).collect();
    let id: String = client_id
        .chars()
        .filter(|z| z.is_ascii_alphanumeric())
        .collect();

    kennung.len() >= 4 && id.starts_with(&kennung)
}

/// `imkr 0.21.68` -> `0.21.68`.
fn imkr_version() -> Option<String> {
    let aus = Command::new("imkr").arg("version").output().ok()?;
    let text = String::from_utf8_lossy(&aus.stdout);

    text.split_whitespace().last().map(str::to_string)
}

/// Die Seite an Brain — und merken, für welche imkr-Version sie dort liegt.
fn seite_schieben(c: &Connection, endgueltig: &Arc<Mutex<Option<String>>>) {
    let Some(version) = imkr_version() else {
        melden("imkr-Version unbekannt, Seite nicht geschickt.");
        return;
    };

    let html = match imkr_web("page", None) {
        Ok(h) => h,
        Err(grund) => {
            melden(&grund);
            return;
        }
    };

    let rumpf = serde_json::json!({ "html": html, "version": version }).to_string();

    if rumpf.len() > GRENZE_SEITE {
        melden(&format!(
            "FEHLER: Seite wiegt {} Bytes, nicht gesendet.",
            rumpf.len()
        ));
        return;
    }

    match anfrage(c, "POST", PFAD_SEITE, Some(&rumpf)) {
        Ok(_) => {
            melden(&format!("Seite für imkr {version} geschickt."));
            if let Ok(pfad) = ablage("leitung-seite.txt") {
                let _ = std::fs::write(pfad, &version);
            }
        }
        Err(HttpFehler::Verboten(grund)) => verbieten(endgueltig, &grund),
        Err(e) => melden(&format!("Seite nicht geschickt: {e:?}")),
    }
}

/// Beim Verbinden: die Seite nur dann, wenn Brain diese imkr-Version noch nicht hat.
fn seite_wenn_neu(c: &Connection, endgueltig: &Arc<Mutex<Option<String>>>) {
    let gemerkt = ablage("leitung-seite.txt")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok());

    if imkr_version().is_some_and(|v| gemerkt.as_deref() != Some(v.as_str())) {
        seite_schieben(c, endgueltig);
    }
}

fn verbieten(endgueltig: &Arc<Mutex<Option<String>>>, grund: &str) {
    if let Ok(mut g) = endgueltig.lock() {
        *g = Some(format!(
            "AI Brain verweigert: {grund} — lege auf, kein neuer Versuch."
        ));
    }
}

/// Der Faden, der den Zustand schiebt, solange Herzschläge kommen.
struct Zuschauer {
    herzschlag: Arc<Mutex<Instant>>,
    halt: Arc<std::sync::atomic::AtomicBool>,
    faden: JoinHandle<()>,
}

impl Zuschauer {
    fn starten(c: &Connection, endgueltig: Arc<Mutex<Option<String>>>) -> Zuschauer {
        let herzschlag = Arc::new(Mutex::new(Instant::now()));
        let halt = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let c = c.clone();
        let h = herzschlag.clone();
        let stopp = halt.clone();

        let faden = std::thread::spawn(move || {
            let mut zuletzt: Option<(Instant, String)> = None;

            loop {
                let still = h.lock().map(|z| z.elapsed()).unwrap_or(ZUSCHAUER_STILLE);

                if stopp.load(std::sync::atomic::Ordering::Relaxed)
                    || still >= ZUSCHAUER_STILLE
                    || !schalter_gilt()
                {
                    break;
                }

                match imkr_web("state", None).and_then(|roh| zustand_filtern(&roh)) {
                    Ok((_, zustand)) => {
                        let faellig = zuletzt.as_ref().is_none_or(|(wann, alt)| {
                            *alt != zustand || wann.elapsed() >= ZUSTAND_TAKT
                        });

                        if faellig {
                            let rumpf = serde_json::json!({ "zustand": serde_json::from_str::<serde_json::Value>(&zustand).unwrap_or_default() }).to_string();

                            match anfrage(&c, "POST", PFAD_ZUSTAND, Some(&rumpf)) {
                                Ok(_) => zuletzt = Some((Instant::now(), zustand)),
                                Err(HttpFehler::Verboten(grund)) => {
                                    verbieten(&endgueltig, &grund);
                                    break;
                                }
                                Err(e) => melden(&format!("Zustand nicht geschickt: {e:?}")),
                            }
                        }
                    }
                    Err(grund) => melden(&grund),
                }

                std::thread::sleep(ZUSTAND_PRUEFEN);
            }

            melden("Zuschauer weg — schiebe keinen Zustand mehr.");
        });

        melden("Zuschauer da — schiebe den Zustand.");

        Zuschauer {
            herzschlag,
            halt,
            faden,
        }
    }

    fn schlag(&self) {
        if let Ok(mut z) = self.herzschlag.lock() {
            *z = Instant::now();
        }
    }

    fn lebt(&self) -> bool {
        !self.faden.is_finished()
    }

    fn beenden(self) {
        self.halt.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = self.faden.join();
    }
}

/// Tippt per `imkr web send` und meldet das Ergebnis an Brain.
fn senden_melden(c: &Connection, s: &Senden) {
    let ergebnis = (|| -> Result<(), String> {
        let (eigener, _) = zustand_filtern(&imkr_web("state", None)?)?;

        if !host_erlaubt(s.host.as_deref(), &eigener, &c.device_name, &c.client_id) {
            return Err(format!(
                "Fremder Rechner „{}“: diese Leitung zeigt nur „{}“.",
                s.host.as_deref().unwrap_or_default(),
                c.device_name
            ));
        }

        // host immer null: der eigene Rechner, ohne dass imkr ihn als
        // Fernrechner per SSH anspricht.
        let auftrag = serde_json::json!({
            "host": null, "session": s.session, "pane": s.pane,
            "text": s.text, "enter": s.enter,
        })
        .to_string();

        let antwort = imkr_web("send", Some(&auftrag))?;
        let wert: serde_json::Value = serde_json::from_str(antwort.trim())
            .map_err(|_| format!("`imkr web send` antwortete unlesbar: {}", antwort.trim()))?;

        match wert.get("error").and_then(|e| e.as_str()) {
            Some(fehler) => Err(fehler.to_string()),
            None => Ok(()),
        }
    })();

    senden_ergebnis(c, s.auftrag.as_ref(), &ergebnis);
}

/// Meldet Brain, ob ein `senden` ankam. Ohne `auftrag` fragt niemand danach.
fn senden_ergebnis(
    c: &Connection,
    auftrag: Option<&serde_json::Value>,
    ergebnis: &Result<(), String>,
) {
    let Some(auftrag) = auftrag else {
        if let Err(fehler) = ergebnis {
            melden(&format!("Senden scheiterte: {fehler}"));
        }
        return;
    };

    let rumpf = match ergebnis {
        Ok(()) => serde_json::json!({ "auftrag": auftrag, "ok": true }),
        Err(fehler) => serde_json::json!({ "auftrag": auftrag, "ok": false, "error": fehler }),
    }
    .to_string();

    // Der Text steht nie im Protokoll — nur, ob es ging.
    match anfrage(c, "POST", PFAD_SENDEN, Some(&rumpf)) {
        Ok(_) => melden(&format!(
            "Senden für Auftrag {auftrag} gemeldet ({}).",
            if ergebnis.is_ok() { "ok" } else { "Fehler" }
        )),
        Err(e) => melden(&format!(
            "Senden-Ergebnis für Auftrag {auftrag} nicht gemeldet: {e:?}"
        )),
    }
}

// ---------------------------------------------------------------------------
// Die Reihenfolge der Eingaben
// ---------------------------------------------------------------------------

/// Das Feld, in dem Brain jedes Eingabe-Ereignis nummeriert.
const EINGABE_SEQ: &str = "seq";

/// So lange wird auf ein fehlendes Ereignis gewartet, bevor es übersprungen wird.
const LUECKE_WARTEN: Duration = Duration::from_millis(300);

/// Ordnet Eingabe-Ereignisse nach ihrer `seq`.
///
/// **Warum.** Im ersten echten Test (26.10.2026) kam „echo hallo-äöü-e2e“ als
/// „ech ohllao-äöüe-2e“ an: Brain broadcastete jede Taste in einer eigenen
/// Anfrage, und parallele Anfragen überholen sich. Die Leitung selbst
/// vertauscht nichts — aber sie ist die letzte Stelle, die es richten kann.
///
/// Fehlt eine Nummer, wird [`LUECKE_WARTEN`] gewartet und dann weitergemacht:
/// ein verlorenes Ereignis darf das Terminal nicht anhalten. Was danach noch
/// eintrifft, geht sofort durch statt verloren — eine Taste zu spät ist
/// besser als eine verschluckte. Ereignisse ohne `seq` gehen ungeordnet
/// durch, wie bisher.
pub struct Ordner {
    naechste: Option<u64>,
    puffer: BTreeMap<u64, String>,
    /// Seit wann der Puffer auf eine fehlende Nummer wartet.
    seit: Option<Instant>,
}

impl Ordner {
    pub fn neu() -> Self {
        Ordner {
            naechste: None,
            puffer: BTreeMap::new(),
            seit: None,
        }
    }

    /// Ein Ereignis kommt an; zurück kommt, was jetzt verarbeitet werden kann.
    pub fn eingang(&mut self, daten: String, jetzt: Instant) -> Vec<String> {
        let seq = serde_json::from_str::<serde_json::Value>(&daten)
            .ok()
            .and_then(|v| v.get(EINGABE_SEQ)?.as_u64());

        let Some(seq) = seq else {
            return vec![daten];
        };

        let naechste = *self.naechste.get_or_insert(seq);

        if seq < naechste {
            // Eine Zählung von vorn (neu geöffnete Ansicht): erst alles
            // Gepufferte hinaus, dann ab hier weiter.
            if seq == 0 || naechste - seq > 100 {
                let mut aus = self.alles();
                aus.push(daten);
                self.naechste = Some(seq + 1);
                return aus;
            }

            // Ein Nachzügler nach übersprungener Lücke.
            return vec![daten];
        }

        self.puffer.insert(seq, daten);
        let aus = self.reife();
        self.uhr(jetzt, !aus.is_empty());

        aus
    }

    /// Nach Ablauf der Wartezeit die Lücke überspringen.
    pub fn takt(&mut self, jetzt: Instant) -> Vec<String> {
        let Some(seit) = self.seit else {
            return Vec::new();
        };

        if jetzt.duration_since(seit) < LUECKE_WARTEN {
            return Vec::new();
        }

        self.naechste = self.puffer.keys().next().copied();
        let aus = self.reife();
        self.uhr(jetzt, true);

        aus
    }

    fn reife(&mut self) -> Vec<String> {
        let mut aus = Vec::new();

        while let Some(naechste) = self.naechste {
            let Some(daten) = self.puffer.remove(&naechste) else {
                break;
            };

            aus.push(daten);
            self.naechste = Some(naechste + 1);
        }

        aus
    }

    /// Die Wartezeit läuft ab der ersten Lücke — und neu, sobald es voranging.
    fn uhr(&mut self, jetzt: Instant, voran: bool) {
        if self.puffer.is_empty() {
            self.seit = None;
        } else if voran || self.seit.is_none() {
            self.seit = Some(jetzt);
        }
    }

    fn alles(&mut self) -> Vec<String> {
        self.seit = None;
        std::mem::take(&mut self.puffer).into_values().collect()
    }
}

// ---------------------------------------------------------------------------
// Die Steuerung: welches Pane, welcher Tap
// ---------------------------------------------------------------------------

struct Steuerung {
    c: Connection,
    tap: Option<Tap>,
    zuschauer: Option<Zuschauer>,
    /// Setzt ein Bündler bei 403 — die Hauptschleife legt dann auf.
    endgueltig: Arc<Mutex<Option<String>>>,
}

impl Steuerung {
    fn neu(c: Connection) -> Self {
        Steuerung {
            c,
            tap: None,
            zuschauer: None,
            endgueltig: Arc::new(Mutex::new(None)),
        }
    }

    fn verarbeiten(&mut self, daten: &str) {
        let eingabe = match eingabe_lesen(daten) {
            Ok(e) => e,
            Err(grund) => {
                melden(&grund);
                // Ein unlesbares `senden` mit Auftrag bekommt trotzdem eine
                // Antwort — sonst wartet der Browser vergeblich.
                if let Some(auftrag) = senden_auftrag(daten) {
                    let c = self.c.clone();
                    std::thread::spawn(move || senden_ergebnis(&c, Some(&auftrag), &Err(grund)));
                }
                return;
            }
        };

        match eingabe {
            Eingabe::Groesse => {}
            Eingabe::Zuschauer => match &self.zuschauer {
                Some(z) if z.lebt() => z.schlag(),
                _ => {
                    if let Some(alt) = self.zuschauer.take() {
                        alt.beenden();
                    }
                    self.zuschauer = Some(Zuschauer::starten(&self.c, self.endgueltig.clone()));
                }
            },
            Eingabe::Seite => {
                let c = self.c.clone();
                let e = self.endgueltig.clone();
                std::thread::spawn(move || seite_schieben(&c, &e));
            }
            Eingabe::Senden(senden) => {
                let c = self.c.clone();
                std::thread::spawn(move || senden_melden(&c, &senden));
            }
            Eingabe::Tasten { pane, b64 } => {
                if let Some((tap, _)) = self.tap_fuer(&pane) {
                    tap.schreiben(&serde_json::json!({ "input": b64 }));
                }
            }
            Eingabe::Erstbild { pane } => {
                // Ein frischer Tap beginnt ohnehin mit einem Vollbild; ein
                // zweites gleich dahinter wäre doppelte Last.
                if let Some((tap, false)) = self.tap_fuer(&pane) {
                    tap.schreiben(&serde_json::json!({ "screen": true }));
                }
            }
            Eingabe::Ende { pane } => {
                if self.tap.as_ref().is_some_and(|t| t.pane == pane) {
                    self.tap_beenden();
                }
            }
            Eingabe::Befehl(befehl) => {
                melden(&format!(
                    "Befehl für Auftrag {} in {} (Frist {} s)",
                    befehl.auftrag, befehl.pane, befehl.frist
                ));
                let c = self.c.clone();
                std::thread::spawn(move || befehl_melden(&c, &befehl));
            }
        }
    }

    /// Der Tap für dieses Pane — gestartet, wenn es noch keinen gibt. Das
    /// `bool` sagt, ob er gerade neu ist.
    fn tap_fuer(&mut self, pane: &str) -> Option<(&mut Tap, bool)> {
        if self.tap.as_ref().is_some_and(|t| t.pane != pane) {
            self.tap_beenden();
        }

        let neu = self.tap.is_none();

        if neu {
            match Tap::starten(&self.c, pane, self.endgueltig.clone()) {
                Ok(tap) => {
                    melden(&format!("zeige {pane}"));
                    self.tap = Some(tap);
                }
                Err(grund) => {
                    melden(&grund);
                    return None;
                }
            }
        }

        self.tap.as_mut().map(|t| (t, neu))
    }

    /// Beendet den Tap und wartet, bis sein Endemarker bei Brain ist.
    ///
    /// **Das Warten ist der Punkt.** Brain führt die Live-Sitzung je GERÄT,
    /// nicht je Pane, und erzwingt „ein Pane je Gerät“ nicht selbst. Liefe
    /// das `ende` des alten Taps erst nach dem ersten Vollbild des neuen ein,
    /// schlösse es dessen Sitzung — und `geraet_live_begonnen` des neuen fiele
    /// ins Leere, weil Brain die Sitzung zu dem Zeitpunkt noch offen sah.
    /// Ein Pane-Wechsel kostet damit höchstens einen POST, und das ist es wert.
    fn tap_beenden(&mut self) {
        if let Some(tap) = self.tap.take() {
            melden(&format!("beende {}", tap.pane));

            if let Some(buendler) = tap.beenden() {
                let _ = buendler.join();
            }
        }
    }

    /// Ein Tap, dessen Pane endete, wird weggeräumt.
    fn aufraeumen(&mut self) {
        if self.tap.as_mut().is_some_and(|t| !t.lebt()) {
            self.tap_beenden();
        }
    }

    fn alles_beenden(&mut self) {
        if let Some(z) = self.zuschauer.take() {
            z.beenden();
        }

        self.tap_beenden();
    }
}

struct Tap {
    pane: String,
    kind: Child,
    /// Geteilt mit dem Bündler: nach einem 409 fordert er selbst ein Vollbild an.
    eingabe: Arc<Mutex<Option<ChildStdin>>>,
    buendler: Option<JoinHandle<()>>,
}

impl Tap {
    fn starten(
        c: &Connection,
        pane: &str,
        endgueltig: Arc<Mutex<Option<String>>>,
    ) -> Result<Tap, String> {
        let mut kind = Command::new("imkr")
            .args(["tap", pane, "--max", TAP_MAX])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("`imkr tap {pane}` liess sich nicht starten: {e}"))?;

        let ausgabe = kind.stdout.take().ok_or("imkr tap ohne Ausgabe.")?;
        let eingabe = Arc::new(Mutex::new(kind.stdin.take()));

        // Lesen und Senden getrennt: Während ein POST läuft, liest der eine
        // Faden weiter, und imkr staut nicht. Der andere bündelt, was sich
        // bis dahin angesammelt hat, zu einem Block.
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            for zeile in BufReader::new(ausgabe).lines() {
                let Ok(zeile) = zeile else { break };

                if let Some(z) = tap_zeile(&zeile) {
                    if tx.send(z).is_err() {
                        break;
                    }
                }
            }
        });

        let c = c.clone();
        let p = pane.to_string();
        let e = eingabe.clone();
        let buendler = std::thread::spawn(move || posten(&c, &p, rx, &e, &endgueltig));

        Ok(Tap {
            pane: pane.to_string(),
            kind,
            eingabe,
            buendler: Some(buendler),
        })
    }

    fn schreiben(&self, wert: &serde_json::Value) {
        if let Ok(mut eingabe) = self.eingabe.lock() {
            if let Some(stdin) = eingabe.as_mut() {
                let _ = writeln!(stdin, "{wert}");
                let _ = stdin.flush();
            }
        }
    }

    fn lebt(&mut self) -> bool {
        matches!(self.kind.try_wait(), Ok(None))
    }

    /// SIGTERM an imkr tap; der Bündler läuft aus und schickt den Endemarker.
    fn beenden(mut self) -> Option<JoinHandle<()>> {
        if let Ok(mut eingabe) = self.eingabe.lock() {
            eingabe.take();
        }

        if self.lebt() {
            signal_beenden(self.kind.id());

            let beginn = Instant::now();
            while self.lebt() && beginn.elapsed() < Duration::from_secs(3) {
                std::thread::sleep(Duration::from_millis(50));
            }

            if self.lebt() {
                let _ = self.kind.kill();
            }
        }

        let _ = self.kind.wait();

        self.buendler.take()
    }
}

/// Der Bündler-Faden: nimmt Tap-Zeilen und schickt sie als Blöcke an Brain.
fn posten(
    c: &Connection,
    pane: &str,
    rx: mpsc::Receiver<TapZeile>,
    eingabe: &Arc<Mutex<Option<ChildStdin>>>,
    endgueltig: &Arc<Mutex<Option<String>>>,
) {
    let mut b = Buendler::neu(pane);

    let vollbild_anfordern = || {
        if let Ok(mut e) = eingabe.lock() {
            if let Some(stdin) = e.as_mut() {
                let _ = writeln!(stdin, "{}", serde_json::json!({ "screen": true }));
                let _ = stdin.flush();
            }
        }
    };

    while !b.beendet {
        let zeilen: Vec<TapZeile> = match rx.recv() {
            Ok(erste) => std::iter::once(erste)
                .chain(rx.try_iter().take(255))
                .collect(),
            Err(_) => {
                // imkr ist weg, ohne `exit` zu sagen (SIGTERM). Der Browser
                // soll trotzdem wissen, dass hier Schluss ist.
                vec![TapZeile::Ende]
            }
        };

        for block in b.aufnehmen(zeilen) {
            // Der Riegel vor dem Netz: Brain sagte sonst 422, Reverb gar nichts.
            // Er verkleinert auch: sonst käme dasselbe Vollbild gleich schwer
            // wieder, und die Leitung drehte sich im Kreis.
            if block.json.len() > b.grenze {
                melden(&format!(
                    "FEHLER: Block mit {} Bytes nicht gesendet, bündle kleiner.",
                    block.json.len()
                ));
                b.verkleinern(b.grenze as u64, block.json.len() as u64);
                b.nicht_angenommen(&block);
                b.verloren(block.json.len() as u64);
                vollbild_anfordern();
                break;
            }

            match anfrage(c, "POST", "geraete/ausgabe", Some(&block.json)) {
                Ok(_) => b.zugestellt(&block),
                Err(HttpFehler::Vorbei) => {
                    melden(&format!(
                        "{pane}: Brain hat den Tap beendet (409), beginne neu mit Vollbild."
                    ));
                    b.neu_beginnen();
                    vollbild_anfordern();
                    break;
                }
                Err(HttpFehler::Verboten(grund)) => {
                    if let Ok(mut g) = endgueltig.lock() {
                        *g = Some(format!(
                            "AI Brain verweigert die Ausgabe: {grund} — lege auf, kein neuer Versuch."
                        ));
                    }
                    return;
                }
                Err(HttpFehler::ZuGross {
                    grund,
                    grenze,
                    gewicht,
                }) => {
                    // Bei richtiger Stückelung darf das nicht vorkommen —
                    // also ein Fehler im Protokoll. Behandelt wird es
                    // trotzdem: Block weg, kleiner bündeln, neues Vollbild.
                    melden(&format!("FEHLER: Block abgelehnt — {grund}"));

                    if let (Some(grenze), Some(gewicht)) = (grenze, gewicht) {
                        b.verkleinern(grenze, gewicht);
                    }

                    b.nicht_angenommen(&block);
                    b.verloren(block.json.len() as u64);
                    vollbild_anfordern();
                    break;
                }
                Err(HttpFehler::Sonst(grund)) | Err(HttpFehler::Unbekannt(grund)) => {
                    // Ob Brain den Block noch bekam, ist unbekannt. Also nicht
                    // dieselbe seq noch einmal, sondern neu aufsetzen: was bis
                    // zum nächsten Vollbild kommt, ist ohnehin überholt.
                    melden(&format!("{pane}: Ausgabe nicht zugestellt — {grund}"));
                    b.verloren(block.json.len() as u64);
                    vollbild_anfordern();
                    break;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Der Bündler (rein, ohne Netz)
// ---------------------------------------------------------------------------

/// Eine Zeile von `imkr tap`.
#[derive(Debug, PartialEq, Eq)]
pub enum TapZeile {
    Schirm {
        bytes: Vec<u8>,
        rows: u64,
        cols: u64,
    },
    Aus(Vec<u8>),
    Luecke(u64),
    Ende,
}

pub fn tap_zeile(zeile: &str) -> Option<TapZeile> {
    let wert: serde_json::Value = serde_json::from_str(zeile).ok()?;
    let b64 = |s: &serde_json::Value| {
        base64::engine::general_purpose::STANDARD
            .decode(s.as_str()?)
            .ok()
    };

    if let Some(schirm) = wert.get("screen") {
        return Some(TapZeile::Schirm {
            bytes: b64(schirm)?,
            rows: wert.get("rows")?.as_u64()?,
            cols: wert.get("cols")?.as_u64()?,
        });
    }

    if let Some(aus) = wert.get("out") {
        return Some(TapZeile::Aus(b64(aus)?));
    }

    if let Some(n) = wert.get("gap") {
        return Some(TapZeile::Luecke(n.as_u64()?));
    }

    wert.get("exit").map(|_| TapZeile::Ende)
}

/// Wie viele Bytes am Ende ein angebrochenes UTF-8-Zeichen sind.
///
/// Die werden zurückgehalten und dem nächsten Block vorangestellt. Sonst
/// wäre ein Block, der mitten in einem „ä“ endet, kein UTF-8 mehr und ginge
/// als base64 — und der nächste ebenso.
pub fn angebrochen_am_ende(bytes: &[u8]) -> usize {
    let n = bytes.len();

    for k in 1..=n.min(3) {
        let b = bytes[n - k];

        if b & 0xC0 == 0x80 {
            continue;
        }

        let noetig = match b {
            0xF0.. => 4,
            0xE0.. => 3,
            0xC0.. => 2,
            _ => 1,
        };

        return if noetig > k { k } else { 0 };
    }

    0
}

/// Teilt in Stücke von höchstens `roh` Bytes, nie mitten in einem
/// UTF-8-Zeichen.
pub fn stuecke(bytes: &[u8], roh: usize) -> Vec<&[u8]> {
    let mut aus = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let mut ende = (i + roh).min(bytes.len());

        if ende < bytes.len() {
            while ende > i && bytes[ende] & 0xC0 == 0x80 {
                ende -= 1;
            }

            if ende == i {
                ende = (i + roh).min(bytes.len());
            }
        }

        aus.push(&bytes[i..ende]);
        i = ende;
    }

    aus
}

/// `text`, wenn das als JSON kürzer ist als base64 — sonst `b64`.
///
/// Terminal-Ausgabe ist voller ESC, und jedes wird in JSON zu sechs Zeichen.
/// Für eine bunte Ausgabe ist base64 deshalb manchmal die kürzere Form.
pub fn kodieren(bytes: &[u8]) -> (&'static str, String) {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);

    if let Ok(text) = std::str::from_utf8(bytes) {
        let escaped = serde_json::to_string(text)
            .map(|s| s.len() - 2)
            .unwrap_or(usize::MAX);

        if escaped <= b64.len() {
            return ("text", text.to_string());
        }
    }

    ("b64", b64)
}

/// Eine fertige Nutzlast für `/geraete/ausgabe`.
#[derive(Debug)]
pub struct Block {
    pub json: String,
    pub seq: u64,
    pub vollbild: bool,
}

pub struct Buendler {
    pane: String,
    seq: u64,
    rest: Vec<u8>,
    verworfen: u64,
    wartet_auf_schirm: bool,
    /// Kam für diesen Tap schon ein Vollbild bei Brain AN? Ohne das kein `ende`:
    /// Brain kennt die Sitzung je Gerät, und ein `ende` ohne Anfang träfe im
    /// Zweifel eine fremde.
    begonnen: bool,
    pub beendet: bool,
    /// Rohbytes je Block; schrumpft, wenn Brain mit 422 und `grenze` antwortet.
    roh: usize,
    /// Was eine ganze Nutzlast wiegen darf — Brains `grenze`, sobald bekannt.
    pub grenze: usize,
}

impl Buendler {
    pub fn neu(pane: &str) -> Self {
        Buendler {
            pane: pane.to_string(),
            seq: 0,
            rest: Vec::new(),
            verworfen: 0,
            wartet_auf_schirm: true,
            begonnen: false,
            beendet: false,
            roh: ROH_BLOCK,
            grenze: GRENZE_BLOCK,
        }
    }

    /// Nimmt eine Folge von Tap-Zeilen und gibt die fertigen Nutzlasten.
    ///
    /// Aufeinanderfolgende `out` werden zusammengelegt — das ist das Bündeln.
    /// Alles vor dem ersten Vollbild (und nach einer Lücke bis zum nächsten)
    /// wird nur gezählt: ohne Vollbild davor kann der Browser es nicht
    /// einordnen.
    pub fn aufnehmen(&mut self, zeilen: Vec<TapZeile>) -> Vec<Block> {
        let mut aus = Vec::new();
        let mut puffer = Vec::new();

        for zeile in zeilen {
            match zeile {
                TapZeile::Aus(bytes) => {
                    if self.wartet_auf_schirm {
                        self.verworfen += bytes.len() as u64;
                    } else {
                        puffer.extend(bytes);
                    }
                }
                TapZeile::Schirm { bytes, rows, cols } => {
                    self.fliessend(&mut puffer, &mut aus);
                    self.schirm(&bytes, rows, cols, &mut aus);
                }
                TapZeile::Luecke(n) => {
                    self.fliessend(&mut puffer, &mut aus);
                    self.verworfen += n + self.rest.len() as u64;
                    self.rest.clear();
                    self.wartet_auf_schirm = true;
                }
                TapZeile::Ende => {
                    self.fliessend(&mut puffer, &mut aus);
                    self.beendet = true;

                    // Auch ein Vollbild aus DIESEM Schub zählt: es geht vor
                    // dem `ende` hinaus und eröffnet die Sitzung.
                    if self.begonnen || aus.iter().any(|b| b.vollbild) {
                        aus.push(self.ende());
                    }

                    return aus;
                }
            }
        }

        self.fliessend(&mut puffer, &mut aus);

        aus
    }

    /// Nach 409: von vorn, seq 0, erst wieder ab einem Vollbild.
    ///
    /// `beendet` fällt mit zurück: ging der Endemarker im selben Schub
    /// verloren, schickt der Faden ihn neu, sobald imkr weg ist.
    pub fn neu_beginnen(&mut self) {
        self.seq = 0;
        self.begonnen = false;
        self.beendet = false;
        self.rest.clear();
        self.wartet_auf_schirm = true;
    }

    /// Brain hat den Block sicher NICHT angenommen (422, oder der eigene
    /// Riegel): seine seq und alle danach gelten als nie vergeben.
    ///
    /// Sonst begänne die nächste angenommene Folge mit einer Lücke, und der
    /// Browser hielte das für verlorene Ausgabe. Bei einem Netzfehler gilt
    /// das NICHT — da ist offen, ob Brain den Block bekam.
    pub fn nicht_angenommen(&mut self, block: &Block) {
        self.seq = self.seq.min(block.seq);
    }

    /// Brain hat den Block angenommen.
    pub fn zugestellt(&mut self, block: &Block) {
        if block.vollbild {
            self.begonnen = true;
        }
    }

    /// Brain meldet 422 mit `grenze` und `gewicht`: künftig kleiner bündeln.
    ///
    /// Das Verhältnis aus beiden sagt, wie stark der Block über der Grenze
    /// lag; zehn Prozent Abschlag dazu, damit nicht der nächste Block gerade
    /// wieder knapp darüber liegt. Nie unter 1 KB — kleiner hilft nicht, nur
    /// mehr Anfragen.
    pub fn verkleinern(&mut self, grenze: u64, gewicht: u64) {
        self.grenze = self.grenze.min(grenze as usize);

        if gewicht > 0 {
            let neu = (self.roh as u128 * grenze as u128 * 9 / (gewicht as u128 * 10)) as usize;
            self.roh = neu.clamp(1024, self.roh.max(1024));
        }
    }

    /// Ein Block kam nicht an: zählen und auf das nächste Vollbild warten.
    pub fn verloren(&mut self, bytes: u64) {
        self.verworfen += bytes;
        self.beendet = false;
        self.rest.clear();
        self.wartet_auf_schirm = true;
    }

    fn fliessend(&mut self, puffer: &mut Vec<u8>, aus: &mut Vec<Block>) {
        if puffer.is_empty() {
            return;
        }

        let mut bytes = std::mem::take(&mut self.rest);
        bytes.append(puffer);

        let halt = angebrochen_am_ende(&bytes);
        self.rest = bytes.split_off(bytes.len() - halt);

        for stueck in stuecke(&bytes, self.roh) {
            aus.push(self.block(stueck, false, None));
        }
    }

    fn schirm(&mut self, bytes: &[u8], rows: u64, cols: u64, aus: &mut Vec<Block>) {
        // Ein Vollbild ersetzt alles davor — auch ein angebrochenes Zeichen.
        self.verworfen += self.rest.len() as u64;
        self.rest.clear();
        self.wartet_auf_schirm = false;

        let teile = stuecke(bytes, self.roh);
        let teile: Vec<&[u8]> = if teile.is_empty() {
            vec![&[][..]]
        } else {
            teile
        };

        for (i, stueck) in teile.into_iter().enumerate() {
            let groesse = (i == 0).then_some((rows, cols));
            aus.push(self.block(stueck, i == 0, groesse));
        }
    }

    fn ende(&mut self) -> Block {
        let rest = std::mem::take(&mut self.rest);

        let mut block = self.roh_block(&rest, false, None);
        block.insert("ende".to_string(), true.into());

        Block {
            json: serde_json::Value::Object(block).to_string(),
            seq: self.seq - 1,
            vollbild: false,
        }
    }

    fn block(&mut self, bytes: &[u8], vollbild: bool, groesse: Option<(u64, u64)>) -> Block {
        let json = serde_json::Value::Object(self.roh_block(bytes, vollbild, groesse)).to_string();

        Block {
            json,
            seq: self.seq - 1,
            vollbild,
        }
    }

    fn roh_block(
        &mut self,
        bytes: &[u8],
        vollbild: bool,
        groesse: Option<(u64, u64)>,
    ) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("pane".to_string(), self.pane.clone().into());
        m.insert("seq".to_string(), self.seq.into());
        self.seq += 1;

        if vollbild {
            m.insert("vollbild".to_string(), true.into());
            m.insert(
                "verworfen".to_string(),
                std::mem::take(&mut self.verworfen).into(),
            );
        }

        if let Some((rows, cols)) = groesse {
            m.insert(
                "groesse".to_string(),
                serde_json::json!({ "rows": rows, "cols": cols }),
            );
        }

        let (feld, wert) = kodieren(bytes);
        m.insert(feld.to_string(), wert.into());

        m
    }
}

// ---------------------------------------------------------------------------
// Befehle
// ---------------------------------------------------------------------------

/// Kürzt auf höchstens `grenze` Bytes und behält das ENDE — bei einem Befehl
/// steht dort, was schiefging.
///
/// **Gemessen wird, was auf dem Draht steht: die JSON-Form.** Brain lehnt ein
/// Ergebnis über 40 000 Bytes ab, und Befehlsausgabe ist oft bunt — jedes ESC
/// wird in JSON zu sechs Zeichen. 16 KB Rohbytes wären so bis zu 96 KB.
/// Das `bool` sagt, ob gekürzt wurde (`gekuerzt` im Ergebnis).
pub fn kuerzen(bytes: &[u8], grenze: usize) -> (String, bool) {
    let text = String::from_utf8_lossy(bytes);

    if json_laenge(&text) <= grenze {
        return (text.into_owned(), false);
    }

    let vermerk = format!("[… {} Bytes gekürzt]\n", text.len());
    let mut platz = grenze.saturating_sub(json_laenge(&vermerk));
    let mut ab = text.len();

    for (i, z) in text.char_indices().rev() {
        let gewicht = json_zeichen(z);

        if gewicht > platz {
            break;
        }

        platz -= gewicht;
        ab = i;
    }

    (format!("{vermerk}{}", &text[ab..]), true)
}

/// So lang wird ein Zeichen in einer JSON-Zeichenkette (wie serde_json schreibt).
fn json_zeichen(z: char) -> usize {
    match z {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
        z if (z as u32) < 0x20 => 6,
        z => z.len_utf8(),
    }
}

fn json_laenge(text: &str) -> usize {
    text.chars().map(json_zeichen).sum()
}

/// Fährt einen Befehl und meldet das Ergebnis an Brain.
fn befehl_melden(c: &Connection, befehl: &Befehl) {
    let (exit, ausgabe, abgebrochen) = befehl_fahren(befehl);
    let (ausgabe, gekuerzt) = kuerzen(&ausgabe, GRENZE_ERGEBNIS);

    // `exit` steht IMMER drin, auch als null (Vertrag): ein fehlendes Feld
    // hiesse für Brain etwas anderes als „kein Exit-Code“.
    let rumpf = serde_json::json!({
        "auftrag": befehl.auftrag,
        "exit": exit,
        "ausgabe": ausgabe,
        "gekuerzt": gekuerzt,
        "abgebrochen": abgebrochen,
    })
    .to_string();

    match anfrage(c, "POST", "geraete/befehl-ergebnis", Some(&rumpf)) {
        Ok(_) => melden(&format!(
            "Ergebnis für Auftrag {} gemeldet (exit {exit:?}).",
            befehl.auftrag
        )),
        // Unbekannt, abgelaufen, fremd oder schon gemeldet: ein Neuversuch
        // träfe dieselbe Antwort.
        Err(HttpFehler::Unbekannt(grund)) => melden(&format!(
            "Auftrag {} kennt Brain nicht (mehr) — verworfen: {grund}",
            befehl.auftrag
        )),
        Err(e) => melden(&format!(
            "Ergebnis für Auftrag {} nicht gemeldet: {e:?}",
            befehl.auftrag
        )),
    }
}

/// Die Befehlszeile: in die Shell des Panes, oder als Nachricht an den Agenten darin.
pub fn befehl_zeile(befehl: &Befehl, ist_agent: bool) -> Vec<String> {
    if ist_agent {
        return vec![
            "msg".into(),
            "--wait".into(),
            befehl.pane.clone(),
            befehl.befehl.clone(),
        ];
    }

    let (sitzung, nummer) = befehl.pane.split_once('/').unwrap_or(("", &befehl.pane));

    vec![
        "-s".into(),
        sitzung.into(),
        "run".into(),
        "--pane".into(),
        nummer.into(),
        "--timeout".into(),
        befehl.frist.to_string(),
        "--".into(),
        format!("sh -c {}", sh_quote(&befehl.befehl)),
    ]
}

/// Einfache Anführungszeichen für die Shell; ein `'` darin wird zu `'\''`.
///
/// **Nötig, weil `imkr run` die Wörter nach `--` ungequotet in das Pane
/// tippt** (am 26.10.2026 nachgemessen: `-- echo "a b;echo ZWEI"` gab zwei
/// Zeilen). Getrennt übergeben, liefe `sh -c ls; exit 3` in der Shell des
/// Panes — und das `exit` schlösse das Pane. Darum geht der ganze Befehl als
/// EIN fertig gequotetes Wort.
pub fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn befehl_fahren(befehl: &Befehl) -> (Option<i32>, Vec<u8>, bool) {
    let ist_agent = imkr::zustand(&befehl.pane) != PaneZustand::Weg;

    let mut kind = match Command::new("imkr")
        .args(befehl_zeile(befehl, ist_agent))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(k) => k,
        Err(e) => {
            return (
                None,
                format!("imkr liess sich nicht starten: {e}").into_bytes(),
                false,
            )
        }
    };

    // Beide Ströme in eigenen Fäden leeren: ein voller Pipe-Puffer hielte den
    // Befehl an, und die Frist liefe ab, ohne dass er etwas falsch gemacht hätte.
    let leeren = |mut quelle: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut puffer = Vec::new();
            let _ = quelle.read_to_end(&mut puffer);
            puffer
        })
    };

    let aus = kind.stdout.take().map(|s| leeren(Box::new(s)));
    let fehl = kind.stderr.take().map(|s| leeren(Box::new(s)));

    // imkr run hat seine eigene Frist; unsere ist der Riegel dahinter,
    // und `msg --wait` hat keine.
    let frist = Duration::from_secs(befehl.frist + 5);
    let beginn = Instant::now();
    let mut abgebrochen = false;

    let status = loop {
        match kind.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if beginn.elapsed() > frist => {
                abgebrochen = true;
                signal_beenden(kind.id());
                std::thread::sleep(Duration::from_millis(500));
                let _ = kind.kill();
                break kind.wait().ok();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => break None,
        }
    };

    // `imkr run` gibt nach seiner Frist auf, der Befehl im Pane läuft aber
    // weiter (nachgesehen: `sleep 30` mit Frist 2 stand danach noch dort).
    // Abgebrochen ist er erst mit einem Ctrl+C.
    if !ist_agent && beginn.elapsed() >= Duration::from_secs(befehl.frist) {
        abgebrochen = true;

        if let Some((sitzung, nummer)) = befehl.pane.split_once('/') {
            let _ = Command::new("imkr")
                .args([
                    "-s",
                    sitzung,
                    "send",
                    "--pane",
                    nummer,
                    "--no-enter",
                    "\u{3}",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    let mut ausgabe = aus.and_then(|h| h.join().ok()).unwrap_or_default();
    ausgabe.extend(fehl.and_then(|h| h.join().ok()).unwrap_or_default());

    (status.and_then(|s| s.code()), ausgabe, abgebrochen)
}

// ---------------------------------------------------------------------------
// Kleinkram
// ---------------------------------------------------------------------------

fn jetzt() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn uhrzeit(unix: i64) -> String {
    let s = unix.rem_euclid(86_400);
    format!("{:02}:{:02}", s / 3600, (s % 3600) / 60)
}

fn melden(text: &str) {
    let s = jetzt().rem_euclid(86_400);
    eprintln!(
        "[{:02}:{:02}:{:02}] leitung: {text}",
        s / 3600,
        (s % 3600) / 60,
        s % 60
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auftrag_auch_aus_unlesbarem_senden() {
        let daten = r#"{"art":"senden","pane":"","session":"bau","text":"x","enter":true,"auftrag":"9f1c"}"#;
        assert!(eingabe_lesen(daten).is_err());
        assert_eq!(senden_auftrag(daten), Some(serde_json::json!("9f1c")));
        assert_eq!(senden_auftrag(r#"{"art":"senden","pane":1}"#), None);
        assert_eq!(senden_auftrag(r#"{"art":"befehl","auftrag":"9f1c"}"#), None);
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn schalter_ist_fail_closed() {
        let jetzt = 1_000_000;

        assert_eq!(schalter_bis(None, jetzt), None, "keine Datei = aus");
        assert_eq!(schalter_bis(Some("kaputt"), jetzt), None, "unlesbar = aus");
        assert_eq!(
            schalter_bis(Some(r#"{"bis":999999}"#), jetzt),
            None,
            "abgelaufen = aus"
        );
        assert_eq!(
            schalter_bis(Some(r#"{"bis":1000000}"#), jetzt),
            None,
            "genau jetzt = aus"
        );
        assert_eq!(
            schalter_bis(
                Some(&format!(r#"{{"bis":{}}}"#, jetzt + HOECHSTDAUER + 1)),
                jetzt
            ),
            None,
            "jenseits der Höchstdauer = aus, nicht gekappt"
        );
        assert_eq!(
            schalter_bis(Some(r#"{"bis":1001800}"#), jetzt),
            Some(1_001_800)
        );
    }

    #[test]
    fn dauern() {
        assert_eq!(dauer_lesen("30m"), Ok(1800));
        assert_eq!(dauer_lesen("2h"), Ok(7200));
        assert_eq!(dauer_lesen("90s"), Ok(90));
        assert_eq!(dauer_lesen("15"), Ok(900));
        assert!(dauer_lesen("9h").is_err());
        assert!(dauer_lesen("0m").is_err());
        assert!(dauer_lesen("bald").is_err());
    }

    #[test]
    fn nur_panes_auf_diesem_rechner() {
        assert!(pane_gueltig("imkr/3"));
        assert!(pane_gueltig("ai-brain_2.x/10"));
        assert!(!pane_gueltig("server:api/1"), "fremder Rechner");
        assert!(!pane_gueltig("imkr"));
        assert!(!pane_gueltig("imkr/"));
        assert!(!pane_gueltig("/3"));
        assert!(!pane_gueltig("imkr/3/4"));
        assert!(
            !pane_gueltig("imkr/claude"),
            "nur Nummern — ein Name könnte wandern"
        );
    }

    #[test]
    fn eingaben_lesen() {
        assert_eq!(
            eingabe_lesen(r#"{"art":"erstbild","pane":"imkr/3"}"#),
            Ok(Eingabe::Erstbild {
                pane: "imkr/3".into()
            })
        );
        assert_eq!(
            eingabe_lesen(r#"{"art":"tasten","pane":"imkr/3","b64":"bHMN"}"#),
            Ok(Eingabe::Tasten {
                pane: "imkr/3".into(),
                b64: "bHMN".into()
            })
        );
        assert_eq!(
            eingabe_lesen(r#"{"art":"groesse","cols":80,"rows":24}"#),
            Ok(Eingabe::Groesse)
        );
        assert_eq!(
            eingabe_lesen(r#"{"art":"ende","pane":"imkr/3"}"#),
            Ok(Eingabe::Ende {
                pane: "imkr/3".into()
            })
        );
        assert!(eingabe_lesen(r#"{"art":"tasten","pane":"imkr/3","b64":"%%"}"#).is_err());
        assert!(eingabe_lesen(r#"{"art":"erstbild","pane":"laptop:imkr/3"}"#).is_err());
        assert!(eingabe_lesen(r#"{"art":"erstbild"}"#).is_err());
        assert!(eingabe_lesen(r#"{"art":"loeschen","pane":"imkr/3"}"#).is_err());
    }

    #[test]
    fn befehl_verschachtelt_und_flach() {
        let erwartet = Eingabe::Befehl(Befehl {
            pane: "imkr/1".into(),
            befehl: "ls".into(),
            frist: 600,
            auftrag: json("42"),
        });

        assert_eq!(
            eingabe_lesen(
                r#"{"art":"befehl","pane":"imkr/1","befehl":{"befehl":"ls","frist":9999,"auftrag":42}}"#
            ),
            Ok(erwartet)
        );

        let Ok(Eingabe::Befehl(flach)) =
            eingabe_lesen(r#"{"art":"befehl","pane":"imkr/1","befehl":"ls","auftrag":"a-1"}"#)
        else {
            panic!("flacher Befehl nicht gelesen");
        };
        assert_eq!(flach.frist, FRIST_BEFEHL_VORGABE);
        assert_eq!(flach.auftrag, json(r#""a-1""#));

        assert!(
            eingabe_lesen(r#"{"art":"befehl","pane":"imkr/1","befehl":{"befehl":"ls"}}"#).is_err(),
            "ohne auftrag gäbe es keinen Empfänger"
        );
    }

    fn ereignis(seq: u64, taste: &str) -> String {
        format!(r#"{{"art":"tasten","pane":"default/0","b64":"{taste}","seq":{seq}}}"#)
    }

    fn tasten(aus: &[String]) -> String {
        aus.iter()
            .map(|d| json(d)["b64"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn eingaben_werden_geordnet() {
        let t0 = Instant::now();
        let mut o = Ordner::neu();

        assert_eq!(tasten(&o.eingang(ereignis(1, "e"), t0)), "e");
        assert!(o.eingang(ereignis(3, "h"), t0).is_empty(), "2 fehlt noch");
        assert_eq!(tasten(&o.eingang(ereignis(2, "c"), t0)), "ch");
        assert_eq!(tasten(&o.eingang(ereignis(4, "o"), t0)), "o");
        assert!(
            o.takt(t0 + Duration::from_secs(5)).is_empty(),
            "nichts offen"
        );
    }

    #[test]
    fn eine_luecke_haelt_hoechstens_300_ms_auf() {
        let t0 = Instant::now();
        let mut o = Ordner::neu();

        o.eingang(ereignis(1, "a"), t0);
        assert!(o.eingang(ereignis(3, "c"), t0).is_empty());
        assert!(o
            .eingang(ereignis(4, "d"), t0 + Duration::from_millis(100))
            .is_empty());
        assert!(
            o.takt(t0 + Duration::from_millis(250)).is_empty(),
            "noch warten"
        );
        assert_eq!(tasten(&o.takt(t0 + Duration::from_millis(300))), "cd");

        // Der Nachzügler geht sofort durch, statt verloren zu gehen.
        assert_eq!(
            tasten(&o.eingang(ereignis(2, "b"), t0 + Duration::from_millis(400))),
            "b"
        );
        assert_eq!(
            tasten(&o.eingang(ereignis(5, "e"), t0 + Duration::from_millis(400))),
            "e"
        );
    }

    #[test]
    fn ohne_seq_und_neue_zaehlung() {
        let t0 = Instant::now();
        let mut o = Ordner::neu();

        let ohne = r#"{"art":"erstbild","pane":"default/0"}"#.to_string();
        assert_eq!(o.eingang(ohne.clone(), t0), vec![ohne]);

        o.eingang(ereignis(40, "x"), t0);
        assert!(o.eingang(ereignis(42, "z"), t0).is_empty());

        // Neu geöffnete Ansicht zählt ab 0: das Gepufferte geht vorher hinaus.
        assert_eq!(tasten(&o.eingang(ereignis(0, "n"), t0)), "zn");
        assert_eq!(tasten(&o.eingang(ereignis(1, "e"), t0)), "e");
    }

    #[test]
    fn web_eingaben() {
        assert_eq!(
            eingabe_lesen(r#"{"art":"zuschauer","seq":3}"#),
            Ok(Eingabe::Zuschauer)
        );
        assert_eq!(
            eingabe_lesen(r#"{"art":"seite","seq":4}"#),
            Ok(Eingabe::Seite)
        );

        let Ok(Eingabe::Senden(s)) = eingabe_lesen(
            r#"{"art":"senden","seq":5,"auftrag":"s-1","host":null,"session":"default","pane":2,"text":"ls","enter":true}"#,
        ) else {
            panic!("senden nicht gelesen");
        };
        assert_eq!(
            (s.session.as_str(), s.pane, s.enter, s.host),
            ("default", 2, true, None)
        );

        let Ok(Eingabe::Senden(s)) = eingabe_lesen(
            r#"{"art":"senden","auftrag":7,"host":"laptop","session":"imker","pane":"0","text":""}"#,
        ) else {
            panic!("pane als Text nicht gelesen");
        };
        assert_eq!(
            (s.pane, s.enter, s.host.as_deref()),
            (0, false, Some("laptop"))
        );

        let Ok(Eingabe::Senden(ohne)) =
            eingabe_lesen(r#"{"art":"senden","session":"x","pane":"1","text":"a","enter":true}"#)
        else {
            panic!("senden ohne auftrag (Brain-Vertrag) nicht gelesen");
        };
        assert_eq!(ohne.auftrag, None);
        assert!(eingabe_lesen(
            r#"{"art":"senden","auftrag":1,"session":"a b","pane":1,"text":"a"}"#
        )
        .is_err());
        assert!(eingabe_lesen(
            r#"{"art":"senden","auftrag":1,"session":"x","pane":-1,"text":"a"}"#
        )
        .is_err());
    }

    #[test]
    fn fremde_rechner_bleiben_draussen() {
        let roh = r#"{"machine":"laptop","version":"0.21.68","device":null,"sessions":[{"name":"imker"}],"hosts":[{"host":"codingmachine","sessions":[]}]}"#;
        let (rechner, zustand) = zustand_filtern(roh).unwrap();
        let zustand = json(&zustand);

        assert_eq!(rechner, "laptop");
        assert_eq!(zustand["hosts"], json("[]"));
        assert_eq!(zustand["sessions"][0]["name"], "imker", "Eigenes bleibt");
        assert!(zustand_filtern("{}").is_err());

        let id = "011dbb98-3dee-4a43";
        assert!(host_erlaubt(None, "laptop", "harness-probe", id));
        assert!(host_erlaubt(Some("laptop"), "laptop", "harness-probe", id));
        assert!(
            host_erlaubt(Some("harness-probe"), "laptop", "harness-probe", id),
            "Gerätename aus Brain"
        );
        assert!(
            host_erlaubt(Some("harness-probe 011d"), "laptop", "harness-probe", id),
            "mit Kennung"
        );
        assert!(host_erlaubt(
            Some("harness-probe·011dbb"),
            "laptop",
            "harness-probe",
            id
        ));
        assert!(
            !host_erlaubt(Some("harness-probe 99ab"), "laptop", "harness-probe", id),
            "fremde Kennung"
        );
        assert!(
            !host_erlaubt(Some("harness-probe-x"), "laptop", "harness-probe", id),
            "zu kurz"
        );
        assert!(!host_erlaubt(
            Some("codingmachine"),
            "laptop",
            "harness-probe",
            id
        ));
    }

    #[test]
    fn befehlszeilen() {
        let b = Befehl {
            pane: "imkr/3".into(),
            befehl: "echo 'a b'".into(),
            frist: 30,
            auftrag: json("1"),
        };

        assert_eq!(
            befehl_zeile(&b, false),
            [
                "-s",
                "imkr",
                "run",
                "--pane",
                "3",
                "--timeout",
                "30",
                "--",
                r#"sh -c 'echo '\''a b'\'''"#
            ]
        );
        assert_eq!(
            befehl_zeile(&b, true),
            ["msg", "--wait", "imkr/3", "echo 'a b'"]
        );
    }

    #[test]
    fn quoten() {
        assert_eq!(sh_quote("ls; exit 3"), "'ls; exit 3'");
        assert_eq!(sh_quote("it's"), r#"'it'\''s'"#);
    }

    #[test]
    fn tap_zeilen() {
        assert_eq!(
            tap_zeile(&format!(
                r#"{{"screen":"{}","rows":24,"cols":80}}"#,
                b64(b"hallo")
            )),
            Some(TapZeile::Schirm {
                bytes: b"hallo".to_vec(),
                rows: 24,
                cols: 80
            })
        );
        assert_eq!(
            tap_zeile(&format!(r#"{{"out":"{}"}}"#, b64(b"x"))),
            Some(TapZeile::Aus(b"x".to_vec()))
        );
        assert_eq!(
            tap_zeile(r#"{"gap":412000}"#),
            Some(TapZeile::Luecke(412_000))
        );
        assert_eq!(tap_zeile(r#"{"exit":true}"#), Some(TapZeile::Ende));
        assert_eq!(tap_zeile("kein json"), None);
    }

    #[test]
    fn angebrochenes_utf8() {
        let ae = "ä".as_bytes(); // C3 A4
        let euro = "€".as_bytes(); // E2 82 AC

        assert_eq!(angebrochen_am_ende(b"abc"), 0);
        assert_eq!(angebrochen_am_ende(ae), 0);
        assert_eq!(angebrochen_am_ende(&ae[..1]), 1);
        assert_eq!(angebrochen_am_ende(&euro[..2]), 2);
        assert_eq!(angebrochen_am_ende(&[b'a', euro[0]]), 1);
        assert_eq!(angebrochen_am_ende(&[0xF0, 0x9F, 0x98]), 3);
        assert_eq!(angebrochen_am_ende(&[]), 0);
    }

    #[test]
    fn text_oder_b64() {
        assert_eq!(kodieren(b"ls -la\r\n").0, "text");
        // Lauter ESC: escaped sechsmal so lang, base64 nur 4/3.
        assert_eq!(kodieren(&[0x1b; 30]).0, "b64");
        assert_eq!(kodieren(&[0xff, 0xfe]).0, "b64", "kein UTF-8");
    }

    fn parse(blocks: &[Block]) -> Vec<serde_json::Value> {
        blocks.iter().map(|b| json(&b.json)).collect()
    }

    #[test]
    fn vor_dem_ersten_vollbild_wird_nur_gezaehlt() {
        let mut b = Buendler::neu("imkr/3");
        let aus = b.aufnehmen(vec![
            TapZeile::Aus(b"vorher".to_vec()),
            TapZeile::Schirm {
                bytes: b"bild".to_vec(),
                rows: 24,
                cols: 80,
            },
            TapZeile::Aus(b"a".to_vec()),
            TapZeile::Aus(b"b".to_vec()),
        ]);
        let aus = parse(&aus);

        assert_eq!(aus.len(), 2, "zwei out zu einem Block gebündelt");
        assert_eq!(aus[0]["seq"], 0);
        assert_eq!(aus[0]["vollbild"], true);
        assert_eq!(aus[0]["verworfen"], 6);
        assert_eq!(aus[0]["groesse"], json(r#"{"rows":24,"cols":80}"#));
        assert_eq!(aus[0]["text"], "bild");
        assert_eq!(aus[1]["seq"], 1);
        assert_eq!(aus[1]["text"], "ab");
        assert!(aus[1].get("vollbild").is_none());
    }

    #[test]
    fn luecke_verwirft_bis_zum_naechsten_vollbild() {
        let mut b = Buendler::neu("imkr/3");
        b.aufnehmen(vec![TapZeile::Schirm {
            bytes: b"x".to_vec(),
            rows: 1,
            cols: 1,
        }]);

        let aus = parse(&b.aufnehmen(vec![
            TapZeile::Luecke(1000),
            TapZeile::Aus(b"weg".to_vec()),
            TapZeile::Schirm {
                bytes: b"neu".to_vec(),
                rows: 1,
                cols: 1,
            },
        ]));

        assert_eq!(aus.len(), 1);
        assert_eq!(aus[0]["seq"], 1, "seq bleibt lückenlos");
        assert_eq!(aus[0]["verworfen"], 1003);
    }

    #[test]
    fn utf8_wird_ueber_blockgrenzen_zurueckgehalten() {
        let mut b = Buendler::neu("imkr/3");
        b.aufnehmen(vec![TapZeile::Schirm {
            bytes: vec![],
            rows: 1,
            cols: 1,
        }]);

        let ae = "ä".as_bytes();
        let erst = parse(&b.aufnehmen(vec![TapZeile::Aus(vec![b'a', ae[0]])]));
        let dann = parse(&b.aufnehmen(vec![TapZeile::Aus(vec![ae[1], b'b'])]));

        assert_eq!(erst[0]["text"], "a");
        assert_eq!(dann[0]["text"], "äb");
    }

    #[test]
    fn grosses_wird_gestueckelt_und_bleibt_unter_der_grenze() {
        let mut b = Buendler::neu(&format!("{}/1", "s".repeat(100)));

        // Der schlimmste Fall für text: lauter Steuerzeichen.
        let schirm: Vec<u8> = (0..100_000u32).map(|i| (i % 32) as u8).collect();
        let bunt: Vec<u8> = (0..150_000u32)
            .map(|i| if i % 2 == 0 { 0x1b } else { b'[' })
            .collect();
        let umlaute = "äöü€".repeat(30_000).into_bytes();

        let aus = b.aufnehmen(vec![
            TapZeile::Schirm {
                bytes: schirm,
                rows: 1000,
                cols: 1000,
            },
            TapZeile::Aus(bunt),
            TapZeile::Aus(umlaute.clone()),
        ]);

        assert!(aus.len() > 5);
        for (i, block) in aus.iter().enumerate() {
            assert!(
                block.json.len() <= GRENZE_BLOCK,
                "Block {i} wiegt {}",
                block.json.len()
            );
        }

        let aus = parse(&aus);
        for (i, block) in aus.iter().enumerate() {
            assert_eq!(block["seq"], i as u64);
        }
        assert_eq!(aus[0]["vollbild"], true);
        assert!(
            aus[1].get("vollbild").is_none(),
            "nur das erste Stück ist Vollbild"
        );

        // Die Umlaute kommen als text an, nicht zerschnitten.
        let text: String = aus
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect();
        assert!(text.ends_with(&"äöü€".repeat(100)));
    }

    #[test]
    fn ende_traegt_den_rest_und_den_marker() {
        let mut b = Buendler::neu("imkr/3");
        for block in b.aufnehmen(vec![TapZeile::Schirm {
            bytes: b"x".to_vec(),
            rows: 1,
            cols: 1,
        }]) {
            b.zugestellt(&block);
        }
        let aus = parse(&b.aufnehmen(vec![TapZeile::Aus(b"tschuess".to_vec()), TapZeile::Ende]));

        assert_eq!(aus.len(), 2);
        assert_eq!(aus[1]["ende"], true);
        assert_eq!(aus[1]["seq"], 2);
        assert!(b.beendet);
    }

    #[test]
    fn kein_ende_ohne_anfang() {
        let mut b = Buendler::neu("imkr/3");
        let aus = b.aufnehmen(vec![TapZeile::Aus(b"vorher".to_vec()), TapZeile::Ende]);

        assert!(
            aus.is_empty(),
            "ein ende ohne Vollbild träfe die Sitzung des ganzen Geräts"
        );
        assert!(b.beendet);

        // Ein Vollbild, das nie ankam, eröffnet auch keine Sitzung.
        let mut b = Buendler::neu("imkr/3");
        b.aufnehmen(vec![TapZeile::Schirm {
            bytes: b"x".to_vec(),
            rows: 1,
            cols: 1,
        }]);
        assert!(b.aufnehmen(vec![TapZeile::Ende]).is_empty());

        // Im selben Schub zählt es: es geht vor dem ende hinaus.
        let mut b = Buendler::neu("imkr/3");
        let aus = parse(&b.aufnehmen(vec![
            TapZeile::Schirm {
                bytes: b"x".to_vec(),
                rows: 1,
                cols: 1,
            },
            TapZeile::Ende,
        ]));
        assert_eq!(aus.len(), 2);
        assert_eq!(aus[1]["ende"], true);
    }

    #[test]
    fn nach_422_kleiner_buendeln() {
        let mut b = Buendler::neu("imkr/3");
        b.verkleinern(20_000, 50_000);

        assert_eq!(b.grenze, 20_000);
        assert_eq!(b.roh, ROH_BLOCK * 9 / 25);

        let bunt: Vec<u8> = (0..100_000u32).map(|i| (i % 32) as u8).collect();
        let aus = b.aufnehmen(vec![TapZeile::Schirm {
            bytes: bunt,
            rows: 1,
            cols: 1,
        }]);

        assert!(aus.iter().all(|block| block.json.len() <= 20_000));

        b.verkleinern(1, 1_000_000);
        assert_eq!(b.roh, 1024, "nie unter 1 KB");

        // Der abgelehnte Block gibt seine seq zurück: die nächste Folge
        // beginnt dort, wo die abgelehnte begann.
        let erster = aus.into_iter().next().unwrap();
        b.nicht_angenommen(&erster);
        b.verloren(0);
        let neu = parse(&b.aufnehmen(vec![TapZeile::Schirm {
            bytes: b"y".to_vec(),
            rows: 1,
            cols: 1,
        }]));
        assert_eq!(neu[0]["seq"], 0);
    }

    #[test]
    fn nach_409_von_vorn() {
        let mut b = Buendler::neu("imkr/3");
        b.aufnehmen(vec![TapZeile::Schirm {
            bytes: b"x".to_vec(),
            rows: 1,
            cols: 1,
        }]);
        b.neu_beginnen();

        let aus = parse(&b.aufnehmen(vec![
            TapZeile::Aus(b"zu frueh".to_vec()),
            TapZeile::Schirm {
                bytes: b"y".to_vec(),
                rows: 1,
                cols: 1,
            },
        ]));

        assert_eq!(aus.len(), 1);
        assert_eq!(aus[0]["seq"], 0);
        assert_eq!(aus[0]["vollbild"], true);
    }

    #[test]
    fn kuerzen_behaelt_das_ende() {
        assert_eq!(kuerzen(b"kurz", 100), ("kurz".to_string(), false));

        let lang = format!("{}ENDE", "ä".repeat(20_000));
        let (k, gekuerzt) = kuerzen(lang.as_bytes(), GRENZE_ERGEBNIS);

        assert!(gekuerzt);
        assert!(k.len() <= GRENZE_ERGEBNIS);
        assert!(k.starts_with("[… "));
        assert!(k.ends_with("ENDE"));
    }

    #[test]
    fn bunte_ausgabe_bleibt_unter_brains_grenze() {
        // 16 KB lauter ESC wären roh klein, in JSON aber 96 KB.
        let bunt = vec![0x1b_u8; 16 * 1024];
        let (k, gekuerzt) = kuerzen(&bunt, GRENZE_ERGEBNIS);

        assert!(gekuerzt);
        assert!(serde_json::to_string(&k).unwrap().len() - 2 <= GRENZE_ERGEBNIS);

        let rumpf = serde_json::json!({
            "auftrag": "x".repeat(100), "exit": null, "ausgabe": k,
            "gekuerzt": true, "abgebrochen": false,
        })
        .to_string();
        assert!(
            rumpf.len() <= 40_000,
            "Brain lehnt über 40 000 Bytes ab: {}",
            rumpf.len()
        );

        // Die Messung stimmt mit serde_json überein.
        for z in ["a", "\"", "\\", "\n", "\u{1}", "\u{7f}", "ä", "€", "😀"] {
            assert_eq!(
                json_laenge(z),
                serde_json::to_string(z).unwrap().len() - 2,
                "{z:?}"
            );
        }
    }
}
