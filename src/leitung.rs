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

/// Ab dieser Grösse beginnt das Protokoll beim nächsten Start neu (das alte
/// bleibt als `leitung.log.1`).
const PROTOKOLL_GROESSE: u64 = 1024 * 1024;

/// Vorgabe und Obergrenze einer Befehlsfrist, wie bei `rechner-befehl`.
const FRIST_BEFEHL_VORGABE: u64 = 60;
const FRIST_BEFEHL_HOECHST: u64 = 600;

const HILFE: &str = "\
Live-Leitung zu AI Brain: ein imkr-Pane im Browser zeigen und steuern.

Aufrufe:
  ai-brain-harness leitung an [--dauer 30m]   einschalten (hoechstens 8h; s, m oder h)
  ai-brain-harness leitung aus                ausschalten und auflegen
  ai-brain-harness leitung status [--json]    Schalter und Verbindung zeigen
  ai-brain-harness leitung laufen             die Leitung im Vordergrund halten
  ai-brain-harness leitung dienst             halten und selbst an-/ausschalten
  ai-brain-harness leitung autostart          den Dienst beim Anmelden starten
  ai-brain-harness leitung kein-autostart     das wieder abbestellen

Der Dienst schaltet die Leitung selbst an, sobald ein Channel diesen Rechner
in AI Brain freigegeben hat — und wieder aus, sobald die Freigabe faellt.
Ohne Freigabe bleibt sie aus.

`leitung aus` und der Linksklick im Tray haben Vorrang: Das bleibt aus, bis du
wieder einschaltest, nicht nur bis zum naechsten Herzschlag.

Von Hand eingeschaltet gilt hoechstens 8 h; die automatische Leitung hat keine
Frist, sondern eine Bedingung. Welches Pane gezeigt wird, bestimmt AI Brain —
aber nur Panes auf diesem Rechner.
";

// ---------------------------------------------------------------------------
// Der Schalter
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
struct Schalter {
    /// Von Hand eingeschaltet, bis zu diesem Zeitpunkt. 0 heisst: nicht.
    #[serde(default)]
    bis: i64,
    /// Automatisch an, solange ein Channel diesen Rechner freigegeben hat.
    /// Setzt und löscht ausschliesslich der Dienst, nach einer Auskunft von
    /// AI Brain. Ohne Frist — die Bedingung ist die Freigabe (#8932).
    #[serde(default)]
    auto: bool,
    /// Der Mensch hat abgeschaltet. Gilt bis zum nächsten Einschalten.
    #[serde(default)]
    veto: bool,
}

/// Warum die Leitung an oder aus ist.
///
/// **Die Reihenfolge IST die Regel**, deshalb steht sie in einer einzigen
/// reinen Funktion und nicht verteilt über drei `if`s an drei Stellen
/// ({@see schalter_stand}).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stand {
    /// Niemand hat eingeschaltet.
    Aus,
    /// Der Mensch hat abgeschaltet — und das bleibt so, bis er wieder
    /// einschaltet. Nicht bis zum nächsten Herzschlag.
    Veto,
    /// Von Hand, bis zu diesem Zeitpunkt (höchstens 8 h).
    VonHand(i64),
    /// Automatisch, weil eine Freigabe besteht.
    Automatisch,
}

impl Stand {
    pub fn an(self) -> bool {
        matches!(self, Stand::VonHand(_) | Stand::Automatisch)
    }

    /// Ein Wort für Mensch und Tray.
    pub fn wort(self) -> &'static str {
        match self {
            Stand::Aus => "aus",
            Stand::Veto => "abgeschaltet",
            Stand::VonHand(_) => "von Hand",
            Stand::Automatisch => "automatisch",
        }
    }
}

/// Die Vorrangregel, als eine Funktion.
///
/// | `veto` | `bis` gültig | `auto` | Ergebnis |
/// |---|---|---|---|
/// | ja | — | — | [`Stand::Veto`] — AUS |
/// | nein | ja | — | [`Stand::VonHand`] — AN |
/// | nein | nein | ja | [`Stand::Automatisch`] — AN |
/// | nein | nein | nein | [`Stand::Aus`] |
///
/// **Das Veto steht ganz oben, und es hat keine Frist.** `leitung aus` und der
/// Linksklick im Tray schreiben es. Wäre es nur ein gelöschtes `auto`, würde
/// die nächste Auskunft von AI Brain es binnen einer Minute wieder setzen —
/// ein Schalter, der sich selbst zurückstellt, ist kein Schalter. Und ein
/// Veto mit Ablaufzeit ginge wieder an, wenn niemand hinsieht; wer wieder
/// mitlesen lassen will, schaltet ein, und das ist ein Klick.
///
/// **Der Hand-Schalter behält seine Höchstdauer**, die automatische Leitung
/// hat keine. Der Deckel existiert, weil niemand ans Ausschalten denkt — eine
/// Bedingung, die jede Minute neu gelesen wird, tut das strikt besser, und sie
/// hängt am Besitzer in AI Brains Maske statt an dem, der zuletzt am Rechner
/// sass. Ein `bis` jenseits der Höchstdauer gilt als nicht gesetzt: Den hat
/// niemand über `leitung an` geschrieben, und eine von Hand verlängerte Datei
/// ist genau der Fall, gegen den der Deckel schützt.
///
/// Rein, damit die Regel ohne Uhr und Dateisystem prüfbar ist — ein Test je
/// Zeile der Tabelle.
pub fn schalter_stand(roh: Option<&str>, jetzt: i64) -> Stand {
    let Some(roh) = roh else {
        return Stand::Aus;
    };

    // Unlesbar ist AUS, nicht AN: Eine halbe oder fremde Datei ist kein
    // Einschaltbefehl.
    let Ok(schalter) = serde_json::from_str::<Schalter>(roh) else {
        return Stand::Aus;
    };

    if schalter.veto {
        return Stand::Veto;
    }

    if schalter.bis > jetzt && schalter.bis <= jetzt + HOECHSTDAUER {
        return Stand::VonHand(schalter.bis);
    }

    if schalter.auto {
        return Stand::Automatisch;
    }

    Stand::Aus
}

fn ablage(name: &str) -> Result<PathBuf, String> {
    Ok(config::config_dir()?.join(name))
}

fn schalter_roh() -> Option<String> {
    ablage("leitung.json")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
}

fn stand_jetzt() -> Stand {
    schalter_stand(schalter_roh().as_deref(), jetzt())
}

/// Bis wann der HAND-Schalter gilt. Die automatische Leitung hat kein „bis".
fn schalter_lesen() -> Option<i64> {
    match stand_jetzt() {
        Stand::VonHand(bis) => Some(bis),
        _ => None,
    }
}

fn schalter_gilt() -> bool {
    stand_jetzt().an()
}

/// Die Schalterdatei ändern, ohne die anderen Felder zu verlieren.
///
/// Lesen-Ändern-Schreiben und kein Neuschreiben: `an`, `aus` und der Dienst
/// fassen je ein anderes Feld an. Wer die Datei überschreibt, löscht das Veto
/// des Menschen oder die Auskunft des Dienstes mit — und zwar unbemerkt.
fn schalter_aendern(f: impl FnOnce(&mut Schalter)) -> Result<Schalter, String> {
    let mut schalter = schalter_roh()
        .and_then(|roh| serde_json::from_str::<Schalter>(&roh).ok())
        .unwrap_or_default();

    f(&mut schalter);

    let pfad = ablage("leitung.json")?;

    if let Some(ordner) = pfad.parent() {
        std::fs::create_dir_all(ordner)
            .map_err(|e| format!("{} ist nicht anlegbar: {e}", ordner.display()))?;
    }

    // Alle drei Felder immer schreiben, auch die leeren: Ein älterer Harness
    // verlangt `bis` und käme mit einer Datei ohne es nicht zurecht — er läse
    // sie als „unlesbar" und damit als AUS. Das ist der richtige Ausgang, aber
    // nur, wenn er die Datei überhaupt lesen kann.
    std::fs::write(
        &pfad,
        serde_json::to_string(&schalter).unwrap_or_default(),
    )
    .map_err(|e| format!("{} ist nicht schreibbar: {e}", pfad.display()))?;

    Ok(schalter)
}

/// Der Dienst sagt, ob eine Freigabe besteht. Er fasst `bis` und `veto` nicht an.
fn auto_setzen(an: bool) {
    if let Ok(roh) = schalter_roh().ok_or(()) {
        // Nichts schreiben, wenn sich nichts ändert: Die Datei liegt im
        // Konfigurationsordner, und eine Schreiboperation je Minute ist für
        // eine Angabe, die sich selten ändert, nur Verschleiss.
        if serde_json::from_str::<Schalter>(&roh).is_ok_and(|s| s.auto == an) {
            return;
        }
    }

    if let Err(grund) = schalter_aendern(|s| s.auto = an) {
        melden(&format!("Schalter nicht schreibbar: {grund}"));
    }
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
        Some("status") | None if args.get(1).map(String::as_str) == Some("--json") => {
            println!("{}", status_json()?);
            Ok(())
        }
        Some("status") | None => status(),
        Some("laufen") => laufen(),
        Some("dienst") => dienst(),
        Some("autostart") => autostart_ein(),
        Some("kein-autostart") => autostart_aus(),
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

    // Das Veto nimmt das Einschalten zurück — genau dafür ist es da. „An" ist
    // die Handlung, die ein „aus" aufhebt, und nur sie.
    schalter_aendern(|s| {
        s.bis = bis;
        s.veto = false;
    })?;

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

/// Auflegen — und zwar BIS ZUM NÄCHSTEN EINSCHALTEN.
///
/// **Die Datei wird nicht gelöscht, sondern bekommt ein Veto.** Bis zum
/// 27.10.2026 war Löschen richtig: Ohne Datei war die Leitung aus, Punkt.
/// Seit es eine automatische Leitung gibt (#8932), wäre Löschen eine Falle —
/// der Dienst fragt AI Brain, sieht die Freigabe, schreibt `auto` und wäre
/// binnen einer Minute wieder da. Der Mensch hätte abgeschaltet und zugesehen,
/// wie es von selbst angeht.
///
/// Das Veto hat deshalb keine Frist, und `leitung an` ist das Einzige, was es
/// aufhebt. Siehe die Tabelle an {@see schalter_stand}.
fn aus() -> Result<(), String> {
    schalter_aendern(|s| {
        s.bis = 0;
        s.veto = true;
    })?;

    let Some(pid) = laufende_pid() else {
        println!("Leitung ist aus — und bleibt aus, bis du sie einschaltest.");
        return Ok(());
    };

    // Erst die Leitung selbst auflegen lassen: sie sieht den Schalter binnen
    // einer Sekunde fallen, beendet den Tap und schickt den Endemarker. Ein
    // Kill schnitte das ab, und der Browser sähe ein Terminal, das einfriert.
    let beginn = Instant::now();

    while beginn.elapsed() < Duration::from_secs(8) {
        if !prozess_lebt(pid) {
            println!("Leitung ist aus, Verbindung beendet — und bleibt aus, bis du sie einschaltest.");
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
    // Zuerst die Verbindung, dann die Leitung: Wer „Schalter: aus" liest,
    // soll nicht glauben, der Rechner haenge an keiner Instanz (#8914).
    match verbindung_zeigen() {
        Some(v) => println!("Verbindung: {} ({})", v.instanz, v.geraet),
        None => println!("Verbindung: nicht verbunden — `ai-brain-harness connect`"),
    }

    match stand_jetzt() {
        Stand::VonHand(bis) => println!("Schalter : an von Hand, bis {} UTC", uhrzeit(bis)),
        Stand::Automatisch => println!("Schalter : an automatisch (ein Channel hat diesen Rechner freigegeben)"),
        Stand::Veto => println!("Schalter : aus — abgeschaltet, bis du wieder einschaltest"),
        Stand::Aus => println!("Schalter : aus"),
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

/// Dasselbe wie `status`, für Programme — der Tray liest es.
///
/// `lage` stammt aus `leitung-zustand.json` und gilt nur, solange der Prozess
/// lebt — oder wenn er endgültig aufgelegt hat, damit der Grund sichtbar
/// bleibt. Was ein abgestürzter Prozess zurückliess, zählt nicht.
fn status_json() -> Result<String, String> {
    let prozess = laufende_pid();
    let lage = std::fs::read_to_string(ablage(ZUSTAND_DATEI)?)
        .ok()
        .and_then(|roh| serde_json::from_str::<Lage>(&roh).ok())
        .filter(|l| lage_gilt(l, prozess.is_some()));

    let stand = stand_jetzt();

    Ok(serde_json::json!({
        // `an` bleibt die Frage „steht der Schalter auf an" und umfasst jetzt
        // auch die automatische Leitung. Ein aelterer Tray zeigt damit das
        // Richtige, ohne etwas von `grund` zu wissen.
        "an": stand.an(),
        // Nur der HAND-Schalter hat ein „bis". Die automatische Leitung hat
        // eine Bedingung, und eine erfundene Ablaufzeit daneben waere eine
        // Zahl, auf die sich niemand verlassen koennte.
        "bis": schalter_lesen(),
        // Additiv: `aus`, `abgeschaltet`, `von Hand` oder `automatisch` (#8932).
        // Ein Schalter, der von allein angeht, braucht ein Wort dazu.
        "grund": stand.wort(),
        "prozess": prozess,
        "lage": lage,
        "protokoll": ablage("leitung.log")?,
        // Additiv: Ein aelterer Tray liest das Feld nicht und bleibt heil, ein
        // neuerer sieht an `null`, dass dieser Rechner keinen Zugang hat.
        "verbindung": verbindung_zeigen(),
    })
    .to_string())
}

fn lage_gilt(lage: &Lage, prozess_lebt: bool) -> bool {
    prozess_lebt || lage.stand == "aufgelegt"
}

/// Was der Tray ueber die Verbindung wissen muss — und nichts darueber hinaus.
///
/// Weder Client-Secret noch Token-Endpunkt: Das hier wandert in ein Menue,
/// das jeder sieht, der am Rechner sitzt.
#[derive(Serialize)]
pub struct VerbindungKurz {
    pub instanz: String,
    pub geraet: String,
    pub basis: String,
}

/// Die hinterlegte Verbindung — oder `None`, wenn dieser Rechner keine hat.
///
/// ## Das ist der Zugang, nicht die Erreichbarkeit (Stand 27.10.2026)
///
/// „Verbunden" heisst hier: Es liegt ein Geraetezugang zu einer Instanz vor.
/// Ob die Instanz in dieser Sekunde antwortet, steht hier NICHT — und das ist
/// Absicht: Der Tray liest diesen Status alle zwei Sekunden, und eine
/// HTTP-Anfrage in diesem Takt waere eine Dauerlast auf Brain fuer eine
/// Auskunft, die niemand so oft braucht.
///
/// Die Erreichbarkeit sagt die Leitung selbst, solange sie laeuft: `lage`
/// steht dann auf `verbunden` bzw. nennt den Grund. Deshalb zeigt der Tray
/// beides nebeneinander statt einer Zahl, die beides vermischt.
fn verbindung_zeigen() -> Option<VerbindungKurz> {
    let c = config::load().ok()?;
    let geraet = geraet_name(&c);

    Some(VerbindungKurz {
        instanz: c.instance_name,
        geraet,
        basis: c.base_url,
    })
}

// ---------------------------------------------------------------------------
// Wie dieser Geraetezugang heisst
// ---------------------------------------------------------------------------

/// Wo der Name liegt, den AI Brain fuer diesen Zugang fuehrt.
const GERAET_DATEI: &str = "geraet.json";

/// Wie lange eine Abschrift gilt, bevor neu gefragt wird.
///
/// Sechs Stunden: Ein Umbenennen ist selten, und der Name steht in einem
/// Menue, nicht in einer Abrechnung. Haeufiger zu fragen waere Last ohne
/// Gegenwert, seltener waere eine Anzeige, die einen halben Tag luegt.
const GERAET_FRIST: i64 = 6 * 3600;

/// Die Frist fuer genau diese Frage — kurz, siehe `anfrage_frist`.
const GERAET_HTTP: Duration = Duration::from_secs(4);

#[derive(Serialize, Deserialize)]
struct GeraetAbschrift {
    name: String,
    /// Zu welchem Zugang der Name gehoert. Ohne das wuerde nach einem
    /// `disconnect` + neuem `connect` der Name des ALTEN Zugangs weiterstehen.
    client_id: String,
    /// Wann zuletzt gefragt wurde (Unix-Sekunden) — nicht, wann geantwortet
    /// wurde: Ein Fehlschlag soll die Frist genauso starten, sonst fragt jeder
    /// Aufruf neu und wartet.
    gefragt: i64,
}

/// Der Name des Geraetezugangs, wie AI Brain ihn fuehrt.
///
/// ## Warum nicht einfach `connection.device_name` (Stand 27.10.2026)
///
/// Der steht dort als **Abschrift vom Kopplungstag**. Wer den Zugang spaeter
/// unter „Meine Geraete" umbenennt, aendert ihn nicht mit. Gemeldet wurde das
/// am Tray: Dort stand `Gerät omarchy (linux x86_64)` — der Hostname, und
/// weil Desktop und Laptop beide `omarchy` heissen, war die Anzeige richtig
/// und trotzdem nutzlos.
///
/// Gefragt wird hoechstens alle sechs Stunden, und der Zeitstempel wird VOR
/// der Anfrage geschrieben: Eine Instanz, die nicht antwortet, kostet dann
/// einmal vier Sekunden und nicht bei jedem Aufruf.
fn geraet_name(c: &Connection) -> String {
    let abschrift = ablage(GERAET_DATEI)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|roh| serde_json::from_str::<GeraetAbschrift>(&roh).ok())
        .filter(|a| a.client_id == c.client_id);

    if let Some(a) = &abschrift {
        if a.gefragt + GERAET_FRIST > jetzt() {
            return a.name.clone();
        }
    }

    // Was wir bis zur Antwort zeigen: die alte Abschrift, sonst der Name vom
    // Koppeln. Nie ein leerer Platz — „kein Name" ist keine Auskunft.
    let bisher = abschrift
        .map(|a| a.name)
        .unwrap_or_else(|| c.device_name.clone());

    geraet_merken(c, &bisher);

    match geraet_fragen(c) {
        Some(neu) if neu != bisher => {
            geraet_merken(c, &neu);
            neu
        }
        _ => bisher,
    }
}

fn geraet_merken(c: &Connection, name: &str) {
    let Ok(pfad) = ablage(GERAET_DATEI) else {
        return;
    };

    if let Some(ordner) = pfad.parent() {
        let _ = std::fs::create_dir_all(ordner);
    }

    let abschrift = GeraetAbschrift {
        name: name.to_string(),
        client_id: c.client_id.clone(),
        gefragt: jetzt(),
    };

    let _ = std::fs::write(
        &pfad,
        serde_json::to_string(&abschrift).unwrap_or_default(),
    );
}

/// Wie lange der Dienst zwischen zwei Freigabe-Fragen schläft.
///
/// Fünf Minuten, nicht eine: Ein Haken wird selten gesetzt, und wer ihn gerade
/// setzt, wartet ohnehin ein paar Sekunden am Browser. Eine Minute wäre der
/// Takt des Herzschlags — der gilt aber für eine STEHENDE Leitung, wo die
/// Anfrage ohnehin läuft. Hier geht es um einen Rechner, der wartet, und der
/// soll nicht im Minutentakt an eine Instanz klopfen.
const FREIGABE_TAKT: Duration = Duration::from_secs(5 * 60);

/// Hat ein Channel diesen Rechner freigegeben? `None` heisst „keine Auskunft".
///
/// **Und „keine Auskunft" ist nicht „ja".** Kein Netz, eine Instanz ohne das
/// Feld (vor #8932), eine kaputte Antwort — in allen drei Fällen öffnet die
/// automatische Leitung nicht. Fail-closed ist hier die richtige Richtung,
/// anders als bei der Frage „lebt der Rechner?" ({@see GeraeteHerzschlag} auf
/// der Brain-Seite): Dort war das fehlende Wissen UNSERES und hätte einen
/// Befehl verhindert, den jemand ausdrücklich geschickt hat. Hier hat niemand
/// etwas angefordert — die Leitung geht nur auf, WEIL ein Haken es sagt, und
/// ohne Auskunft über den Haken gibt es keinen Anlass.
///
/// Von Hand einschalten geht unabhängig davon weiter.
/// Gibt es eine Freigabe, und von wem? Ein Aufruf für beides.
///
/// Vorher waren es zwei: einer für das Ja, einer für die Namen. Zwei Anfragen
/// für eine Auskunft sind nicht nur doppelte Last — sie können auch
/// widersprüchliche Antworten liefern, wenn zwischen ihnen ein Haken fällt.
fn freigabe_fragen(c: &Connection) -> Option<(bool, Vec<String>)> {
    let text = anfrage_frist(c, "GET", "geraete/ich", None, GERAET_HTTP).ok()?;

    Some((freigabe_aus_ich(&text)?, channels_aus_ich(&text)))
}

/// Die Freigabe aus der Antwort von `geraete/ich`.
pub fn freigabe_aus_ich(text: &str) -> Option<bool> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()?
        .get("imkr")?
        .get("freigegeben")?
        .as_bool()
}

/// Die Channel-Namen aus derselben Antwort.
pub fn channels_aus_ich(text: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| {
            Some(
                v.get("imkr")?
                    .get("channels")?
                    .as_array()?
                    .iter()
                    .filter_map(|c| c.as_str().map(str::to_owned))
                    .collect(),
            )
        })
        .unwrap_or_default()
}

fn geraet_fragen(c: &Connection) -> Option<String> {
    let text = anfrage_frist(c, "GET", "geraete/ich", None, GERAET_HTTP).ok()?;

    let name = serde_json::from_str::<serde_json::Value>(&text)
        .ok()?
        .get("name")?
        .as_str()?
        .trim()
        .to_string();

    (!name.is_empty()).then_some(name)
}

// ---------------------------------------------------------------------------
// Die Zustandsdatei
// ---------------------------------------------------------------------------

/// Wo `laufen` festhält, wie es der Leitung gerade geht.
const ZUSTAND_DATEI: &str = "leitung-zustand.json";

/// Wie es der Leitung geht — für den Tray, der sie nur anzeigt.
///
/// Eine Datei statt eines Sockets: Der Tray ruft ohnehin `leitung status
/// --json`, und so gibt es keinen zweiten Weg in den laufenden Prozess.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lage {
    /// `verbindet`, `verbunden`, `getrennt` (versucht es wieder) oder
    /// `aufgelegt` (endgültig, z. B. 403).
    ///
    /// Dazu zwei Stände, die nur der Dienst kennt (#8932): `wartet` — er lebt,
    /// die Leitung steht aber nicht, und das ist auf einem Rechner ohne
    /// Freigabe der Normalfall und kein Fehler; `abgeschaltet` — ein Mensch
    /// hat `leitung aus` gesagt. Der `grund` daneben sagt, welcher von beiden
    /// es ist.
    pub stand: String,
    /// Seit wann (Unix-Sekunden).
    pub seit: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grund: Option<String>,
    /// Das Pane, das gerade gezeigt wird.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
    /// Welche Channels diesen Rechner freigegeben haben (#8932).
    ///
    /// **Der Tray zeigt sie, und darum geht es.** „Automatisch an" beantwortet
    /// nur die halbe Frage; die ganze lautet *wer sieht da mit?*. Wer mehrere
    /// Channels hat, liest „ein Channel" und weiss es nicht.
    ///
    /// Leer heisst „keine Auskunft" und nicht „keiner": Eine Instanz vor
    /// #8932 schickt die Namen nicht mit, und eine von Hand eingeschaltete
    /// Leitung braucht sie nicht.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub freigaben: Vec<String>,
}

static LAGE: Mutex<Option<Lage>> = Mutex::new(None);

/// Setzt den Stand und schreibt die Datei; das Pane bleibt, wie es ist.
fn lage_stand(stand: &str, grund: Option<&str>) {
    lage_aendern(|l| {
        if l.stand != stand {
            l.seit = jetzt();
        }
        l.stand = stand.to_string();
        l.grund = grund.map(str::to_string);
    });
}

fn lage_pane(pane: Option<&str>) {
    lage_aendern(|l| l.pane = pane.map(str::to_string));
}

/// Wer diesen Rechner freigegeben hat — für die Leiste.
///
/// Nur schreiben, wenn sich etwas ändert: Die Datei wird bei jedem Herzschlag
/// berührt, und ein Umbenennen je Minute ist für eine Angabe, die sich selten
/// ändert, nur Verschleiss.
fn lage_freigaben(channels: &[String]) {
    let gleich = LAGE
        .lock()
        .ok()
        .is_some_and(|g| g.as_ref().is_some_and(|l| l.freigaben == channels));

    if gleich {
        return;
    }

    lage_aendern(|l| l.freigaben = channels.to_vec());
}

fn lage_aendern(f: impl FnOnce(&mut Lage)) {
    let Ok(mut g) = LAGE.lock() else { return };
    let lage = g.get_or_insert_with(Lage::default);
    f(lage);

    // Erst daneben schreiben, dann umbenennen: der Tray liest nie eine halbe Datei.
    if let Ok(pfad) = ablage(ZUSTAND_DATEI) {
        let neu = pfad.with_extension("json.neu");
        if std::fs::write(&neu, serde_json::to_string(lage).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&neu, &pfad);
        }
    }
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

    // Anhängen statt neu anlegen: Die letzten Zeilen vor einem Neustart sind
    // genau die, nach denen man bei einem Abriss sucht. Zu gross geworden,
    // wandert die Datei einmal nach `.1`.
    if std::fs::metadata(&protokoll).is_ok_and(|m| m.len() > PROTOKOLL_GROESSE) {
        let _ = std::fs::rename(&protokoll, protokoll.with_extension("log.1"));
    }

    let datei = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&protokoll)
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

// ---------------------------------------------------------------------------
// Beim Anmelden starten
// ---------------------------------------------------------------------------

/// Der Autostart-Eintrag des Dienstes.
///
/// Eigenes Label, eigener Dateiname, eigener Run-Wert — er steht neben dem des
/// Trays und nicht an seiner Stelle. Beide dürfen auf einem Rechner liegen:
/// Der Tray braucht eine Desktop-Sitzung, der Dienst nicht, und ein Rechner
/// ohne Leiste soll seine Leitung trotzdem halten.
const DIENST_AUTOSTART: ai_brain_autostart::Eintrag = ai_brain_autostart::Eintrag {
    label: "de.peppermint.ai-brain-leitung",
    datei: "ai-brain-leitung",
    name: "AI Brain — Leitung",
    kommentar: "Haelt die Leitung zu AI Brain, solange ein Channel diesen Rechner freigegeben hat",
    run_wert: "ai-brain-leitung",
    args: &["leitung", "dienst"],
};

fn autostart_ein() -> Result<(), String> {
    ai_brain_autostart::ein(&DIENST_AUTOSTART)?;

    println!("Der Dienst startet ab dem nächsten Anmelden. Jetzt sofort starten:");
    println!("  ai-brain-harness leitung dienst &");
    println!();
    println!("Die Leitung geht dann an, sobald ein Channel diesen Rechner freigegeben hat");
    println!("— und aus, sobald die Freigabe fällt. `leitung aus` schaltet sie ab, und das");
    println!("bleibt so, bis du sie wieder einschaltest.");

    Ok(())
}

fn autostart_aus() -> Result<(), String> {
    ai_brain_autostart::aus(&DIENST_AUTOSTART)
}

/// Die PID-Datei belegen — eine Leitung je Gerät.
fn merker_belegen() -> Result<Merker, String> {
    if let Some(pid) = laufende_pid().filter(|p| *p != std::process::id()) {
        return Err(format!(
            "Eine Leitung läuft schon (Prozess {pid}). Eine je Gerät."
        ));
    }

    let pfad = ablage("leitung.pid")?;
    std::fs::write(&pfad, format!("{}\n", std::process::id()))
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", pfad.display()))?;

    Ok(Merker(pfad))
}

/// Warten, aber sofort aufwachen, wenn jemand einschaltet.
///
/// In Sekundenschritten statt eines langen `sleep`: Wer `leitung an` tippt,
/// soll nicht bis zu fünf Minuten auf seine Leitung warten. `true` heisst
/// „der Schalter steht jetzt auf an".
fn warten_bis_an(dauer: Duration) -> bool {
    let beginn = Instant::now();

    while beginn.elapsed() < dauer {
        if schalter_gilt() {
            return true;
        }

        std::thread::sleep(Duration::from_secs(1));
    }

    schalter_gilt()
}

/// Ob die letzte Freigabe-Frage ohne Antwort blieb.
static STUMME_INSTANZ: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Der Dienst: läuft beim Anmelden an und bleibt (#8932).
///
/// **Was er tut, und was er bewusst nicht tut.** Er hält die Leitung, solange
/// der Schalter auf an steht, und er führt das Feld `auto` nach der Auskunft
/// von AI Brain. Er entscheidet nichts über Rechte: Ob ein Agent etwas darf,
/// prüft Brain bei jedem Aufruf gegen `channel_geraete`. Dieser Dienst
/// beseitigt nur den zweiten Schalter, der neben dem Haken stand.
///
/// **Kein systemd-Dienst, sondern der Autostart-Mechanismus** aus
/// `ai-brain-autostart` (XDG-`.desktop`, LaunchAgent, Run-Key). Die Leitung
/// gehört in die Nutzersitzung — sie spricht mit dem imkr-Server dieses
/// Nutzers und liest seinen Konfigurationsordner. Ein Systemdienst wäre der
/// falsche Geltungsbereich, und `systemd --user` gäbe es nur unter Linux.
///
/// **Er hält die PID-Datei für seine ganze Lebenszeit**, auch während er
/// wartet. Sonst startete ein `leitung an` einen zweiten Prozess daneben, und
/// zwei Leitungen auf einem Gerät schicken sich gegenseitig Endemarker.
/// Stattdessen schreibt `an` nur den Schalter, und dieser Dienst wacht auf.
fn dienst() -> Result<(), String> {
    let _merker = merker_belegen()?;
    let connection = config::load()?;

    melden("Dienst gestartet.");

    loop {
        // 1 ─ Veto: nicht fragen, nicht öffnen. Ein abgeschalteter Rechner
        //     soll auch keine Auskunft mehr einholen — das wäre Last für eine
        //     Antwort, auf die er nicht reagieren darf.
        if matches!(stand_jetzt(), Stand::Veto) {
            lage_stand("abgeschaltet", Some("von Hand ausgeschaltet"));
            warten_bis_an(FREIGABE_TAKT);
            continue;
        }

        // 2 ─ Freigabe nachführen. Nur der Dienst schreibt `auto`.
        let freigabe = freigabe_fragen(&connection);
        let vorher = matches!(stand_jetzt(), Stand::Automatisch);

        match &freigabe {
            Some((an, channels)) => {
                auto_setzen(*an);
                lage_freigaben(channels);

                if *an && !vorher {
                    melden(&format!(
                        "Freigabe für diesen Rechner: {} — Leitung geht automatisch an.",
                        if channels.is_empty() {
                            "ja".to_string()
                        } else {
                            channels.join(", ")
                        }
                    ));
                }
            }
            // Keine Auskunft heisst nicht „nein": `auto` bleibt, wie es war.
            // Ein Netzausfall soll keine Leitung auflegen, die zu Recht stand.
            //
            // Gemeldet wird nur der ERSTE Ausfall und die Erholung, nicht jeder
            // Versuch — dasselbe Muster wie beim Herzschlag und bei den
            // Brain-Ausfällen des Bündlers. Alle fünf Minuten dieselbe Zeile
            // macht das Protokoll genau dann unlesbar, wenn man darin nach
            // einem echten Ereignis sucht.
            None => {
                if !STUMME_INSTANZ.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    melden("Keine Auskunft über die Freigabe — Schalter bleibt, wie er ist.");
                }
            }
        }

        if freigabe.is_some() && STUMME_INSTANZ.swap(false, std::sync::atomic::Ordering::Relaxed) {
            melden("AI Brain antwortet wieder.");
        }

        // 3 ─ An? Dann halten. Sonst warten und neu fragen.
        if !schalter_gilt() {
            lage_stand(
                "wartet",
                Some(match &freigabe {
                    Some((false, _)) => "kein Channel hat diesen Rechner freigegeben",
                    Some((true, _)) => "Schalter steht auf aus",
                    None => "AI Brain antwortet nicht",
                }),
            );

            warten_bis_an(FREIGABE_TAKT);
            continue;
        }

        // Ein endgültiges Nein (403: Besitz oder Recht fehlt) beendet den
        // Dienst. Dagegen anzurennen würde die Antwort nicht ändern — und ein
        // Autostart ohne KeepAlive bringt ihn beim nächsten Anmelden wieder.
        halten(&connection)?;
    }
}

fn laufen() -> Result<(), String> {
    if !schalter_gilt() {
        return Err(
            "Die Leitung ist ausgeschaltet. Einschalten: ai-brain-harness leitung an [--dauer 30m]"
                .to_string(),
        );
    }

    let _merker = merker_belegen()?;
    let connection = config::load()?;

    halten(&connection)
}

/// Die Leitung halten, bis der Schalter fällt oder Brain endgültig abweist.
///
/// Setzt voraus, dass die PID-Datei schon belegt ist — darum kümmern sich
/// {@see laufen} und {@see dienst}, und nur sie.
fn halten(connection: &Connection) -> Result<(), String> {
    lage_stand("verbindet", None);

    let mut steuerung = Steuerung::neu(connection.clone());
    let mut pause = 1;

    let ergebnis = loop {
        if !schalter_gilt() {
            melden("Schalter aus oder abgelaufen — lege auf.");
            break Ok(());
        }

        match sitzung(connection, &mut steuerung, &mut pause) {
            Ok(()) => {
                melden("Schalter aus oder abgelaufen — lege auf.");
                break Ok(());
            }
            Err(Abbruch::Endgueltig(grund)) => {
                melden(&grund);
                lage_stand("aufgelegt", Some(&grund));
                break Err(grund);
            }
            Err(Abbruch::Wieder(grund)) => {
                melden(&format!("{grund} — neuer Versuch in {pause} s."));
                lage_stand("getrennt", Some(&grund));

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

    // Ein endgültiges Auflegen bleibt stehen, damit der Tray den Grund zeigt;
    // ein Ausschalten räumt auf.
    if ergebnis.is_ok() {
        let _ = std::fs::remove_file(ablage(ZUSTAND_DATEI)?);
    }

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
    lage_stand("verbunden", None);
    *pause = 1;

    {
        let c = c.clone();
        let e = st.endgueltig.clone();
        std::thread::spawn(move || seite_wenn_neu(&c, &e));
    }

    // Je Verbindung neu: nach einem Neuaufbau zählt die Gegenseite womöglich
    // anders weiter, und ein alter Puffer hielte dann Ereignisse fest.
    let mut ordner = Ordner::neu();
    let mut letzter_schlag: Option<Instant> = None;

    loop {
        if !schalter_gilt() {
            st.alles_beenden();
            verbindung.schliessen();
            return Ok(());
        }

        if letzter_schlag.is_none_or(|t| t.elapsed() >= HERZSCHLAG_TAKT) {
            let erster = letzter_schlag.is_none();
            letzter_schlag = Some(Instant::now());
            let c = c.clone();
            let e = st.endgueltig.clone();
            std::thread::spawn(move || herzschlag(&c, &e, erster));
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
    grund_aus(&antwort.into_string().unwrap_or_default())
}

/// Der lesbare Teil einer Fehlerantwort. Eine HTML-Seite (etwa vom Proxy,
/// während Brain neu startet) wird nicht ins Protokoll gekippt.
fn grund_aus(text: &str) -> String {
    if text.trim_start().starts_with('<') {
        return "HTML-Seite statt JSON (Proxy oder Neustart?)".to_string();
    }

    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| {
            v.get("message")
                .or_else(|| v.get("fehler"))
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| text.chars().take(300).collect())
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
    anfrage_frist(c, methode, pfad, rumpf, FRIST_HTTP)
}

/// Dasselbe mit eigener Frist.
///
/// Fuer Aufrufe, die in einem Pfad liegen, der auf nichts warten darf: Der
/// Tray liest `leitung status --json` im Zwei-Sekunden-Takt, und eine Instanz,
/// die 15 s nicht antwortet, haette dort einen sichtbar haengenden Tray zur
/// Folge.
fn anfrage_frist(
    c: &Connection,
    methode: &str,
    pfad: &str,
    rumpf: Option<&str>,
    frist: Duration,
) -> Result<String, HttpFehler> {
    for versuch in 0..2 {
        let token = mcp::access_token(c).map_err(HttpFehler::Sonst)?;

        let anfrage = ureq::request(methode, &api_url(c, pfad))
            .timeout(frist)
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
    /// Die letzten Zeilen eines Panes lesen, für einen Agenten.
    Lesen(Lesen),
    /// Sitzungen, Fenster und Panes auflisten, für einen Agenten.
    Sitzungen(Sitzungen),
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

/// `art: lesen` — die letzten Zeilen eines Panes, für einen Agenten (#8792).
///
/// **Bewusst ohne Textfeld.** Ausgeführt wird ausschliesslich `imkr read
/// <pane> --lines <zeilen>`, gebaut hier aus Adresse und Zahl. Könnte ein
/// `lesen` je einen mitgeschickten Befehl ausführen, wäre „darf sehen“ nur ein
/// anderes Wort für „darf befehlen“ — und diese Grenze hängt allein hier.
#[derive(Debug, PartialEq, Eq)]
pub struct Lesen {
    pub pane: String,
    pub zeilen: u64,
    pub frist: u64,
    pub auftrag: serde_json::Value,
}

const LESEN_ZEILEN_VORGABE: u64 = 50;
const LESEN_ZEILEN_HOECHST: u64 = 2000;

/// `art: sitzungen` — welche Sitzungen, Fenster und Panes es hier gibt (#8932).
///
/// **Die Nutzlast trägt nichts als Auftrag und Frist.** Noch eine Stufe
/// strenger als [`Lesen`]: Dort ist eine Zahl im Spiel, hier nicht einmal
/// das. Ausgeführt wird allein `imkr ls --json`, gebaut in [`sitzungen_zeile`]
/// aus nichts als der Art. Es gibt kein Feld, über das der Aufrufer die
/// Befehlszeile beeinflussen könnte — auch keine Option, kein `--all`, keinen
/// Rechnernamen. Ein `--all` würde über ssh fremde Rechner befragen, und der
/// Harness bleibt strikt lokal.
#[derive(Debug, PartialEq, Eq)]
pub struct Sitzungen {
    pub frist: u64,
    pub auftrag: serde_json::Value,
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

    // Kein führendes `-`: sonst läse imkr die Adresse als Option.
    !sitzung.is_empty()
        && !sitzung.starts_with('-')
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
        // Hier oben, weil es kein Pane HAT: Die Frage lautet „welche Panes
        // gibt es". Ein Pane als Pflichtfeld wäre die Antwort in der Frage.
        "sitzungen" => {
            return Ok(Eingabe::Sitzungen(Sitzungen {
                frist: wert
                    .get("frist")
                    .and_then(|f| f.as_u64())
                    .unwrap_or(FRIST_BEFEHL_VORGABE)
                    .clamp(1, FRIST_BEFEHL_HOECHST),
                auftrag: wert
                    .get("auftrag")
                    .filter(|a| !a.is_null())
                    .cloned()
                    .ok_or("Sitzungen ohne auftrag — die Liste hätte keinen Empfänger.")?,
            }))
        }
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
        "lesen" => Ok(Eingabe::Lesen(Lesen {
            pane,
            zeilen: wert
                .get("zeilen")
                .and_then(|z| z.as_u64())
                .unwrap_or(LESEN_ZEILEN_VORGABE)
                .clamp(1, LESEN_ZEILEN_HOECHST),
            frist: wert
                .get("frist")
                .and_then(|f| f.as_u64())
                .unwrap_or(FRIST_BEFEHL_VORGABE)
                .clamp(1, FRIST_BEFEHL_HOECHST),
            auftrag: wert
                .get("auftrag")
                .filter(|a| !a.is_null())
                .cloned()
                .ok_or("Lesen ohne auftrag — das Ergebnis hätte keinen Empfänger.")?,
        })),
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

/// Der `auftrag` eines abgelehnten `befehl` oder `lesen` — bei `befehl` auch
/// verschachtelt, wie [`eingabe_lesen`] ihn annimmt.
fn abgelehnter_auftrag(daten: &str) -> Option<serde_json::Value> {
    let wert: serde_json::Value = serde_json::from_str(daten).ok()?;
    let art = wert.get("art")?.as_str()?;

    if art != "befehl" && art != "lesen" && art != "sitzungen" {
        return None;
    }

    wert.get("auftrag")
        .or_else(|| wert.get("befehl")?.get("auftrag"))
        .filter(|a| !a.is_null())
        .cloned()
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
            melden(&format!(
                "Seite für imkr {version} geschickt ({} Bytes).",
                rumpf.len()
            ));
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

/// Herzschlag: solange die Leitung steht, unabhängig von Zuschauern (#8792).
/// Daran sieht Brain, ob das Gerät für Agent-Befehle live ist.
const PFAD_HERZSCHLAG: &str = "geraete/herzschlag";
const HERZSCHLAG_TAKT: Duration = Duration::from_secs(60);

fn herzschlag_rumpf(imkr: Option<&str>, pane: Option<&str>) -> String {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "imkr": imkr,
        "pane": pane,
    })
    .to_string()
}

/// Ob Brain den Herzschlag schon einmal mit 404 abgelehnt hat.
static HERZSCHLAG_UNBEKANNT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Gemeldet wird nur der erste Schlag je Verbindung und jeder Fehler —
/// sonst stünde das Protokoll jede Minute voll.
///
/// **404 ist kein Fehler.** Es heisst: diese Brain-Instanz kennt den
/// Herzschlag noch nicht. Brain bleibt fail-closed und schickt Befehle
/// trotzdem mit Frist los; der Schlag macht die Meldung nur freundlicher.
/// Deshalb einmal erwähnen und still weiterschlagen, statt jemanden suchen
/// zu lassen.
fn herzschlag(c: &Connection, endgueltig: &Arc<Mutex<Option<String>>>, erster: bool) {
    use std::sync::atomic::Ordering;

    let pane = LAGE.lock().ok().and_then(|l| l.as_ref()?.pane.clone());
    let rumpf = herzschlag_rumpf(imkr_version().as_deref(), pane.as_deref());

    match anfrage(c, "POST", PFAD_HERZSCHLAG, Some(&rumpf)) {
        Ok(antwort) => {
            if erster || HERZSCHLAG_UNBEKANNT.swap(false, Ordering::Relaxed) {
                melden("Herzschlag angenommen, weiter alle 60 s.");
            }

            freigabe_aus_schlag(&antwort);
        }
        Err(HttpFehler::Unbekannt(_)) => {
            if !HERZSCHLAG_UNBEKANNT.swap(true, Ordering::Relaxed) {
                melden("Brain kennt den Herzschlag noch nicht (404) — schlage still weiter.");
            }
        }
        Err(HttpFehler::Verboten(grund)) => verbieten(endgueltig, &grund),
        Err(e) => melden(&format!("Herzschlag nicht angenommen: {e:?}")),
    }
}

/// Was der Schlag über die Freigabe sagt — der Rückweg für einen ENTZUG.
///
/// **Ohne diesen Rückweg bliebe eine automatische Leitung offen, bis jemand
/// den Rechner neu startet.** Der Haken fällt in AI Brain, und der Rechner
/// erfährt es nie — der Herzschlag war bis zum 27.10.2026 ein reines
/// Einbahn-Signal.
///
/// Fällt der Haken, verschwindet `auto`, und die Leitung sieht ihren Schalter
/// binnen einer Sekunde fallen und legt selbst auf — derselbe Weg wie beim
/// Ablauf des Hand-Schalters, also einer statt zweier.
///
/// **Eine Antwort OHNE das Feld ändert nichts.** Eine Instanz vor #8932
/// antwortet 204 ohne Rumpf; das heisst „ich kann die Frage nicht
/// beantworten" und nicht „nein". Hier `auto` zu löschen hiesse, eine Leitung
/// wegen fehlenden Wissens aufzulegen, die eben noch zu Recht stand.
fn freigabe_aus_schlag(antwort: &str) {
    let Some(freigabe) = freigabe_im_schlag(antwort) else {
        return;
    };

    if !freigabe && matches!(stand_jetzt(), Stand::Automatisch) {
        melden("Keine Freigabe mehr für diesen Rechner — lege die automatische Leitung auf.");
    }

    auto_setzen(freigabe);

    // Die Namen für die Leiste, und zwar HIER und nicht nur beim Aufbau der
    // Leitung: Ein zweiter Haken, der dazukommt, während sie steht, macht die
    // Anzeige sonst still unvollständig — und „wer sieht mit?" ist genau die
    // Frage, für die sie da ist. Eine Instanz vor #8932 schickt das Feld nicht
    // mit; dann bleibt die Liste, wie sie war, statt leer zu werden.
    if let Some(channels) = channels_im_schlag(antwort) {
        lage_freigaben(&channels);
    }
}

/// Was im Rumpf einer Herzschlag-Antwort steht. `None` = keine Aussage.
pub fn freigabe_im_schlag(antwort: &str) -> Option<bool> {
    serde_json::from_str::<serde_json::Value>(antwort)
        .ok()?
        .get("freigabe")?
        .as_bool()
}

/// Die Channel-Namen aus derselben Antwort. `None` = das Feld fehlt.
///
/// Der Unterschied zu einer leeren Liste ist der Punkt: Eine Instanz vor
/// #8932 sagt nichts über die Namen, und dann soll die Anzeige behalten, was
/// sie hat. Eine leere Liste dagegen heisst „derzeit keine".
pub fn channels_im_schlag(antwort: &str) -> Option<Vec<String>> {
    Some(
        serde_json::from_str::<serde_json::Value>(antwort)
            .ok()?
            .get("channels")?
            .as_array()?
            .iter()
            .filter_map(|c| c.as_str().map(str::to_owned))
            .collect(),
    )
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
            // imkr kann sich unter einer laufenden Leitung erneuern; wer jetzt
            // zusieht, soll die Seite der Fassung bekommen, die antwortet.
            seite_wenn_neu(&c, &endgueltig);

            let mut zuletzt: Option<(Instant, String)> = None;
            // Brain startet mal neu; dann zählt der erste Fehler und die Erholung,
            // nicht jede Sekunde dazwischen.
            let mut gestoert = false;

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
                                Ok(_) => {
                                    if gestoert {
                                        melden("Zustand wieder angenommen.");
                                        gestoert = false;
                                    }
                                    if zuletzt.is_none() {
                                        melden(&format!(
                                            "Zustand angenommen ({} Bytes).",
                                            rumpf.len()
                                        ));
                                    }
                                    zuletzt = Some((Instant::now(), zustand));
                                }
                                Err(HttpFehler::Verboten(grund)) => {
                                    verbieten(&endgueltig, &grund);
                                    break;
                                }
                                Err(e) => {
                                    if !gestoert {
                                        melden(&format!(
                                            "Zustand nicht geschickt: {e:?} — melde erst die Erholung."
                                        ));
                                    }
                                    gestoert = true;
                                }
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
/// besser als eine verschluckte. Alles außer `tasten` geht ungeordnet
/// durch, auch mit `seq`.
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
        // Nur Tasten zählen auf derselben Folge; `seite` hat einen eigenen
        // Zähler je Gerät und würde sonst Lücken vortäuschen.
        let seq = serde_json::from_str::<serde_json::Value>(&daten)
            .ok()
            .filter(|v| v.get("art").and_then(|a| a.as_str()) == Some("tasten"))
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
                } else if let Some(auftrag) = abgelehnter_auftrag(daten) {
                    // Dasselbe für `befehl` und `lesen`: der Agent soll den
                    // Grund lesen, statt bis zur Frist „keine Antwort“ zu sehen.
                    let c = self.c.clone();
                    std::thread::spawn(move || abgelehnt_melden(&c, &auftrag, &grund));
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
                melden("Seite angefordert.");
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
                    self.tap_beenden("ende vom Browser");
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
            Eingabe::Lesen(lesen) => {
                melden(&format!(
                    "Lesen für Auftrag {}: {} Zeilen aus {}",
                    lesen.auftrag, lesen.zeilen, lesen.pane
                ));
                let c = self.c.clone();
                std::thread::spawn(move || lesen_melden(&c, &lesen));
            }
            Eingabe::Sitzungen(sitzungen) => {
                melden(&format!(
                    "Sitzungen für Auftrag {} (Frist {} s)",
                    sitzungen.auftrag, sitzungen.frist
                ));
                let c = self.c.clone();
                std::thread::spawn(move || sitzungen_melden(&c, &sitzungen));
            }
        }
    }

    /// Der Tap für dieses Pane — gestartet, wenn es noch keinen gibt. Das
    /// `bool` sagt, ob er gerade neu ist.
    fn tap_fuer(&mut self, pane: &str) -> Option<(&mut Tap, bool)> {
        if self.tap.as_ref().is_some_and(|t| t.pane != pane) {
            self.tap_beenden(&format!("Wechsel zu {pane}"));
        }

        let neu = self.tap.is_none();

        if neu {
            match Tap::starten(&self.c, pane, self.endgueltig.clone()) {
                Ok(tap) => {
                    melden(&format!("zeige {pane}"));
                    lage_pane(Some(pane));
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
    fn tap_beenden(&mut self, grund: &str) {
        if let Some(tap) = self.tap.take() {
            melden(&format!("beende {}: {grund}", tap.pane));
            lage_pane(None);

            if let Some(buendler) = tap.beenden() {
                let _ = buendler.join();
            }
        }
    }

    /// Ein Tap, dessen Pane endete, wird weggeräumt.
    fn aufraeumen(&mut self) {
        if self.tap.as_mut().is_some_and(|t| !t.lebt()) {
            self.tap_beenden("Pane oder Tap endete");
        }
    }

    fn alles_beenden(&mut self) {
        if let Some(z) = self.zuschauer.take() {
            z.beenden();
        }

        self.tap_beenden("Leitung legt auf");
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
    ergebnis_melden(c, &befehl.auftrag, exit, &ausgabe, abgebrochen);
}

/// Die Befehlszeile für `lesen` — nur aus Adresse und Zahl gebaut.
pub fn lesen_zeile(lesen: &Lesen) -> Vec<String> {
    vec![
        "read".into(),
        lesen.pane.clone(),
        "--lines".into(),
        lesen.zeilen.to_string(),
    ]
}

/// Liest das Pane und meldet den Fensterinhalt auf demselben Weg wie ein
/// Befehlsergebnis.
fn lesen_melden(c: &Connection, lesen: &Lesen) {
    let (exit, ausgabe, abgebrochen) =
        imkr_mit_frist(&lesen_zeile(lesen), Duration::from_secs(lesen.frist));
    ergebnis_melden(c, &lesen.auftrag, exit, &ausgabe, abgebrochen);
}

/// Die Befehlszeile für `sitzungen` — aus nichts als der Art gebaut.
///
/// **Kein `--all`.** Das würde über ssh andere Rechner befragen; der Harness
/// bleibt strikt lokal, und der Agent hat eine Freigabe für DIESEN Rechner,
/// nicht für dessen Nachbarn.
pub fn sitzungen_zeile() -> Vec<String> {
    vec!["ls".into(), "--json".into()]
}

/// Die Felder, die aus `imkr ls --json` herausgehen — eine POSITIVLISTE.
///
/// **Warum eine Positivliste und nicht ein Verbot von `tail`.** `imkr ls
/// --json` legt zu jedem Pane ein `tail` bei, nämlich den unteren Rand des
/// Bildschirms — das ist Inhalt, und Inhalt gibt es nur über `lesen`, das ein
/// eigenes Werkzeug mit eigenem Haken ist. Es per Sperrliste zu entfernen
/// würde heute genauso wirken; der Unterschied kommt mit der nächsten
/// imkr-Fassung. Legt die ein Feld dazu — eine Vorschau, eine Zwischenablage,
/// einen Befehlsverlauf —, fährt es bei einer Sperrliste von selbst mit, und
/// zwar still. Hier muss jemand dieses Feld erst hinschreiben.
///
/// Was durchgeht, ist deshalb genau das, was ein Agent zum Adressieren und
/// Einordnen braucht: Adresse, Fenster, Programm, Zustand, Titel.
fn pane_gefiltert(sitzung: &str, pane: &serde_json::Value) -> serde_json::Value {
    let nummer = pane.get("pane").and_then(|p| p.as_u64()).unwrap_or(0);
    let text = |feld: &str| {
        pane.get(feld)
            .and_then(|w| w.as_str())
            .unwrap_or_default()
            .to_string()
    };

    let mut m = serde_json::Map::new();

    // Genau die Form, die `pane` in imkr-lesen und imkr-befehl erwartet. Sie
    // steht hier fertig da, damit der Agent sie nicht zusammensetzen muss —
    // eine selbstgebaute Adresse ist die Fehlerquelle, die dieses Werkzeug
    // beseitigen soll.
    m.insert("adresse".into(), format!("{sitzung}/{nummer}").into());
    m.insert("pane".into(), nummer.into());
    m.insert(
        "fenster".into(),
        pane.get("workspace")
            .and_then(|w| w.as_u64())
            .unwrap_or(0)
            .into(),
    );
    // `dir` heisst in imkr der Fenstername, sonst der Verzeichnisname — das,
    // was Agenten unterscheidet, die alle `claude` heissen.
    m.insert("fenster_name".into(), text("dir").into());
    m.insert("programm".into(), text("program").into());
    m.insert("zustand".into(), text("state").into());
    m.insert(
        "seit".into(),
        pane.get("since").and_then(|w| w.as_u64()).unwrap_or(0).into(),
    );
    m.insert("meta".into(), text("meta").into());
    m.insert("titel".into(), text("title").into());

    serde_json::Value::Object(m)
}

/// Was die Sitzungsliste höchstens wiegen darf.
///
/// **Der Grund ist kein Geschmack, sondern ein Fehler, der sonst still
/// passiert.** Ein Ergebnis über [`GRENZE_ERGEBNIS`] geht durch
/// [`kuerzen`] — und das behält das ENDE und setzt einen Vermerk davor. Bei
/// einer Befehlsausgabe ist das richtig (dort steht unten, was schiefging);
/// bei JSON ergibt es ein **ungültiges Dokument**, und der Agent bekommt
/// etwas, das er nicht lesen kann, ohne zu erfahren warum.
///
/// Gemessen am 27.10.2026: 16 Sitzungen mit 10 Panes wogen kompakt 3,3 kB.
/// Ein Rechner mit 16 LAUFENDEN Sitzungen à 5 Panes käme auf das Fünffache
/// und läge damit drüber. Also vorher deckeln, nicht hinterher kürzen.
///
/// Die Reserve ist für den Vermerk, der dann dazukommt.
const SITZUNGEN_BUDGET: usize = GRENZE_ERGEBNIS - 512;

/// Setzt das Dokument zusammen; die letzten `ohne_liste` Sitzungen ohne Panes.
fn sitzungen_dokument(sitzungen: &[serde_json::Value], ohne_liste: usize) -> String {
    let grenze = sitzungen.len().saturating_sub(ohne_liste);
    let mut aus = Vec::with_capacity(sitzungen.len());

    for (i, sitzung) in sitzungen.iter().enumerate() {
        let mut m = sitzung.as_object().cloned().unwrap_or_default();

        if i >= grenze {
            m.remove("pane_liste");
            // Ausdrücklich, nicht durch Abwesenheit: Ein fehlendes Feld sieht
            // genauso aus wie eine Sitzung ohne Panes, und das ist eine
            // andere Aussage.
            m.insert("pane_liste_fehlt".into(), true.into());
        }

        aus.push(serde_json::Value::Object(m));
    }

    let mut wurzel = serde_json::Map::new();
    wurzel.insert("sitzungen".into(), aus.into());

    if ohne_liste > 0 {
        wurzel.insert(
            "gekuerzt".into(),
            serde_json::json!({
                "sitzungen_ohne_pane_liste": ohne_liste,
                "hinweis": "Zu viele Panes für eine Antwort. Die Sitzungen stehen alle da, \
                            bei den letzten fehlt die Pane-Liste. Die Adressen der uebrigen \
                            gelten unveraendert.",
            }),
        );
    }

    // Kompakt und nicht eingerückt: Einrückung kostete bei der Messung oben
    // 70 % mehr Bytes, und die gehen von den Panes ab, die noch hineinpassen.
    serde_json::Value::Object(wurzel).to_string()
}

/// Projiziert die Antwort von `imkr ls --json` auf die Positivliste.
///
/// Was hier nicht ausdrücklich abgeschrieben wird, verlässt den Rechner nicht.
pub fn sitzungen_filtern(roh: &str) -> Result<String, String> {
    let wert: serde_json::Value =
        serde_json::from_str(roh).map_err(|e| format!("imkr ls --json unlesbar: {e}"))?;

    let liste = wert
        .as_array()
        .ok_or("imkr ls --json lieferte keine Liste.")?;

    let mut sitzungen = Vec::new();

    for sitzung in liste {
        let name = sitzung
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or_default()
            .to_string();

        let mut m = serde_json::Map::new();
        m.insert("name".into(), name.clone().into());
        m.insert(
            "fenster".into(),
            sitzung
                .get("windows")
                .and_then(|w| w.as_u64())
                .unwrap_or(0)
                .into(),
        );
        m.insert(
            "panes".into(),
            sitzung
                .get("panes")
                .and_then(|w| w.as_u64())
                .unwrap_or(0)
                .into(),
        );

        // Zustandszählung je Sitzung, so wie `imkr ls` sie zeigt.
        if let Some(agenten) = sitzung.get("agents").filter(|a| a.is_object()) {
            m.insert("agenten".into(), agenten.clone());
        }

        // Eine schlafende Sitzung hat keine Panes zum Lesen — das gehört
        // dazugesagt, sonst sucht ein Agent Adressen, die es nicht gibt.
        if let Some(ruht) = sitzung.get("dormant").and_then(|d| d.as_str()) {
            m.insert("ruht".into(), ruht.into());
        }

        let panes: Vec<serde_json::Value> = sitzung
            .get("all_panes")
            .and_then(|p| p.as_array())
            .map(|p| p.iter().map(|e| pane_gefiltert(&name, e)).collect())
            .unwrap_or_default();

        // `pane_liste` und nicht `fenster_panes`: Daneben steht `panes` als
        // ZAHL, und „fenster_panes" liest sich wie „Panes je Fenster" — eine
        // Gruppierung, die es hier nicht gibt. Die Liste ist flach, jeder
        // Eintrag nennt sein Fenster selbst.
        m.insert("pane_liste".into(), panes.into());

        sitzungen.push(serde_json::Value::Object(m));
    }

    // Von hinten Pane-Listen fallen lassen, bis es passt. Von hinten und
    // nicht nach einer Rangfolge: Jede andere Wahl hiesse, hier zu entscheiden,
    // welche Sitzung wichtiger ist — und das weiss der Agent, nicht wir.
    for ohne_liste in 0..=sitzungen.len() {
        let doc = sitzungen_dokument(&sitzungen, ohne_liste);

        if json_laenge(&doc) <= SITZUNGEN_BUDGET {
            return Ok(doc);
        }
    }

    // Selbst ohne jede Pane-Liste zu gross: dann gehen die Sitzungsnamen
    // allein heraus. Mehr als eine Behauptung ueber die Zahl der Sitzungen
    // bleibt nicht — aber sie ist wahr, und ungueltiges JSON waere es nicht.
    Ok(sitzungen_dokument(&sitzungen, sitzungen.len()))
}

/// Listet die Sitzungen und meldet die Liste auf demselben Weg wie ein
/// Befehlsergebnis.
///
/// **Gefiltert wird hier, nicht bei Brain.** Der Rechner ist die Stelle, an
/// der entschieden werden muss, was ihn verlässt: Was einmal draussen ist,
/// steht in einem Auftragsbuch und in einem Protokoll, und beides kann ein
/// nachträglicher Filter nicht mehr einholen.
fn sitzungen_melden(c: &Connection, sitzungen: &Sitzungen) {
    let (exit, roh, abgebrochen) =
        imkr_mit_frist(&sitzungen_zeile(), Duration::from_secs(sitzungen.frist));

    // Nur eine erfolgreiche Antwort wird gefiltert. Bei einem Fehlschlag steht
    // in `roh` die Meldung von imkr und kein JSON — die geht unverändert
    // durch, denn sie ist die Auskunft.
    if exit != Some(0) || abgebrochen {
        ergebnis_melden(c, &sitzungen.auftrag, exit, &roh, abgebrochen);
        return;
    }

    match sitzungen_filtern(&String::from_utf8_lossy(&roh)) {
        Ok(liste) => ergebnis_melden(c, &sitzungen.auftrag, exit, liste.as_bytes(), false),
        // Unlesbar heisst abgelehnt, nicht „dann eben ungefiltert": Eine
        // Antwort, die wir nicht zerlegen können, können wir auch nicht
        // säubern.
        Err(grund) => abgelehnt_melden(c, &sitzungen.auftrag, &grund),
    }
}

/// Ein abgelehntes `befehl`/`lesen`: kein Exit-Code, der Grund als Ausgabe.
/// Nichts davon ist gelaufen — auch nicht teilweise.
pub fn abgelehnt_rumpf(auftrag: &serde_json::Value, grund: &str) -> String {
    serde_json::json!({
        "auftrag": auftrag,
        "exit": null,
        "ausgabe": grund,
        "gekuerzt": false,
        "abgebrochen": false,
        "abgelehnt": true,
    })
    .to_string()
}

fn abgelehnt_melden(c: &Connection, auftrag: &serde_json::Value, grund: &str) {
    match anfrage(
        c,
        "POST",
        "geraete/befehl-ergebnis",
        Some(&abgelehnt_rumpf(auftrag, grund)),
    ) {
        Ok(_) => melden(&format!("Ablehnung für Auftrag {auftrag} gemeldet.")),
        Err(e) => melden(&format!(
            "Ablehnung für Auftrag {auftrag} nicht gemeldet: {e:?}"
        )),
    }
}

/// Meldet ein Ergebnis an `geraete/befehl-ergebnis` — für `befehl` und `lesen`.
fn ergebnis_melden(
    c: &Connection,
    auftrag: &serde_json::Value,
    exit: Option<i32>,
    ausgabe: &[u8],
    abgebrochen: bool,
) {
    let (ausgabe, gekuerzt) = kuerzen(ausgabe, GRENZE_ERGEBNIS);

    // `exit` steht IMMER drin, auch als null (Vertrag): ein fehlendes Feld
    // hiesse für Brain etwas anderes als „kein Exit-Code“.
    let rumpf = serde_json::json!({
        "auftrag": auftrag,
        "exit": exit,
        "ausgabe": ausgabe,
        "gekuerzt": gekuerzt,
        "abgebrochen": abgebrochen,
    })
    .to_string();

    match anfrage(c, "POST", "geraete/befehl-ergebnis", Some(&rumpf)) {
        Ok(_) => melden(&format!(
            "Ergebnis für Auftrag {auftrag} gemeldet (exit {exit:?})."
        )),
        // Unbekannt, abgelaufen, fremd oder schon gemeldet: ein Neuversuch
        // träfe dieselbe Antwort.
        Err(HttpFehler::Unbekannt(grund)) => melden(&format!(
            "Auftrag {auftrag} kennt Brain nicht (mehr) — verworfen: {grund}"
        )),
        Err(e) => melden(&format!(
            "Ergebnis für Auftrag {auftrag} nicht gemeldet: {e:?}"
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
    let beginn = Instant::now();

    // imkr run hat seine eigene Frist; unsere ist der Riegel dahinter,
    // und `msg --wait` hat keine.
    let (exit, ausgabe, mut abgebrochen) = imkr_mit_frist(
        &befehl_zeile(befehl, ist_agent),
        Duration::from_secs(befehl.frist + 5),
    );

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

    (exit, ausgabe, abgebrochen)
}

/// Ruft imkr mit einer Frist; nach ihr wird der Aufruf beendet.
fn imkr_mit_frist(args: &[String], frist: Duration) -> (Option<i32>, Vec<u8>, bool) {
    let mut kind = match Command::new("imkr")
        .args(args)
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
    fn abgelehntes_lesen_und_befehl_haben_einen_empfaenger() {
        assert_eq!(
            abgelehnter_auftrag(r#"{"art":"lesen","pane":"-s/0","auftrag":"a1"}"#),
            Some(serde_json::json!("a1"))
        );
        assert_eq!(
            abgelehnter_auftrag(
                r#"{"art":"befehl","pane":"x","befehl":{"befehl":"ls","auftrag":7}}"#
            ),
            Some(serde_json::json!(7))
        );
        assert_eq!(
            abgelehnter_auftrag(r#"{"art":"lesen","pane":"-s/0"}"#),
            None
        );
        assert_eq!(
            abgelehnter_auftrag(r#"{"art":"tasten","auftrag":"a1"}"#),
            None
        );

        let r: serde_json::Value = serde_json::from_str(&abgelehnt_rumpf(
            &serde_json::json!("a1"),
            "Pane abgelehnt.",
        ))
        .unwrap();
        assert_eq!(r["exit"], serde_json::Value::Null);
        assert_eq!(r["abgelehnt"], true);
        assert_eq!(r["ausgabe"], "Pane abgelehnt.");
    }

    #[test]
    fn lesen_fuehrt_nur_imkr_read_aus_egal_was_mitkommt() {
        let Ok(Eingabe::Lesen(l)) = eingabe_lesen(
            r#"{"art":"lesen","pane":"bau/2","auftrag":"a1","befehl":"rm -rf ~","text":"x"}"#,
        ) else {
            panic!("lesen nicht erkannt");
        };
        assert_eq!(l.zeilen, LESEN_ZEILEN_VORGABE);
        assert_eq!(l.frist, FRIST_BEFEHL_VORGABE);
        assert_eq!(lesen_zeile(&l), vec!["read", "bau/2", "--lines", "50"]);
    }

    #[test]
    fn lesen_deckelt_und_verlangt_auftrag() {
        let Ok(Eingabe::Lesen(l)) = eingabe_lesen(
            r#"{"art":"lesen","pane":"bau/2","zeilen":99999,"frist":99999,"auftrag":"a1"}"#,
        ) else {
            panic!("lesen nicht erkannt");
        };
        assert_eq!(
            (l.zeilen, l.frist),
            (LESEN_ZEILEN_HOECHST, FRIST_BEFEHL_HOECHST)
        );

        assert!(eingabe_lesen(r#"{"art":"lesen","pane":"bau/2"}"#).is_err());
        assert!(eingabe_lesen(r#"{"art":"lesen","pane":"laptop:bau/2","auftrag":"a"}"#).is_err());
    }

    // ── Sitzungen auflisten (#8932) ─────────────────────────────────────────

    #[test]
    fn sitzungen_fuehrt_nur_imkr_ls_json_aus_egal_was_mitkommt() {
        // Alles, was ein Aufrufer unterschieben könnte, ist hier dabei: ein
        // Befehl, ein Text, ein Pane, eine Option, ein Rechnername. Nichts
        // davon darf die Befehlszeile erreichen — die steht in
        // `sitzungen_zeile()` fest und kennt die Nutzlast gar nicht.
        let Ok(Eingabe::Sitzungen(sz)) = eingabe_lesen(
            r#"{"art":"sitzungen","auftrag":"a1","befehl":"rm -rf ~","text":"x",
                "pane":"-s/0","args":["--all"],"host":"anderer"}"#,
        ) else {
            panic!("sitzungen nicht erkannt");
        };
        assert_eq!(sz.frist, FRIST_BEFEHL_VORGABE);
        assert_eq!(sitzungen_zeile(), vec!["ls", "--json"]);
        // Und ausdrücklich: kein --all, auch nicht hinten dran.
        assert!(!sitzungen_zeile().iter().any(|a| a == "--all"));
    }

    #[test]
    fn sitzungen_deckelt_die_frist_und_verlangt_einen_auftrag() {
        let Ok(Eingabe::Sitzungen(sz)) =
            eingabe_lesen(r#"{"art":"sitzungen","frist":99999,"auftrag":7}"#)
        else {
            panic!("sitzungen nicht erkannt");
        };
        assert_eq!(sz.frist, FRIST_BEFEHL_HOECHST);
        assert_eq!(sz.auftrag, serde_json::json!(7));

        // Ohne Auftrag hätte die Liste keinen Empfänger.
        assert!(eingabe_lesen(r#"{"art":"sitzungen"}"#).is_err());
        assert!(eingabe_lesen(r#"{"art":"sitzungen","auftrag":null}"#).is_err());
    }

    #[test]
    fn sitzungen_braucht_kein_pane() {
        // Die Pane-Pflicht der anderen Arten darf hier nicht greifen: Die
        // Frage lautet „welche Panes gibt es".
        assert!(eingabe_lesen(r#"{"art":"sitzungen","auftrag":"a1"}"#).is_ok());
        // Gegenprobe, dass die Pflicht überhaupt noch da ist.
        assert!(eingabe_lesen(r#"{"art":"lesen","auftrag":"a1"}"#).is_err());
    }

    #[test]
    fn sitzungen_filter_laesst_den_bildschirminhalt_nicht_raus() {
        // `tail` ist der untere Rand des Bildschirms. Inhalt gibt es nur über
        // `lesen`, und das hängt an einem eigenen Haken.
        let roh = r#"[{"name":"imkr","panes":2,"windows":1,
            "agents":{"working":1,"idle":1},
            "all_panes":[
              {"pane":3,"program":"claude","workspace":1,"dir":"ai-brain",
               "state":"working","since":42,"meta":"opus 5 · 300k",
               "tail":"GEHEIM: das Passwort lautet hunter2","title":"baut #8932"},
              {"pane":0,"program":"bash","workspace":1,"dir":"ai-brain",
               "state":"","since":0}
            ]}]"#;

        let gefiltert = sitzungen_filtern(roh).expect("filtern");
        assert!(
            !gefiltert.contains("hunter2") && !gefiltert.contains("tail"),
            "Bildschirminhalt im Ergebnis: {gefiltert}"
        );

        let v: serde_json::Value = serde_json::from_str(&gefiltert).unwrap();
        let s0 = &v["sitzungen"][0];
        assert_eq!(s0["name"], "imkr");
        assert_eq!(s0["fenster"], 1);
        assert_eq!(s0["panes"], 2);
        assert_eq!(s0["agenten"]["working"], 1);

        let p0 = &s0["pane_liste"][0];
        // Die Adresse steht fertig da — genau die Form, die imkr-lesen will.
        assert_eq!(p0["adresse"], "imkr/3");
        assert_eq!(p0["programm"], "claude");
        assert_eq!(p0["zustand"], "working");
        assert_eq!(p0["titel"], "baut #8932");
        assert_eq!(p0["fenster_name"], "ai-brain");
        assert_eq!(p0["seit"], 42);
        assert_eq!(p0["meta"], "opus 5 · 300k");

        // Auch eine Shell ohne Agent wird aufgeführt: Ein Befehl soll dort
        // laufen können, und ohne Adresse findet sie niemand.
        assert_eq!(s0["pane_liste"][1]["adresse"], "imkr/0");
        assert_eq!(s0["pane_liste"][1]["zustand"], "");
    }

    #[test]
    fn sitzungen_filter_ist_eine_positivliste_und_keine_sperrliste() {
        // Der eigentliche Grund für die Positivliste: ein Feld, das eine
        // spätere imkr-Fassung dazulegt. Eine Sperrliste gegen `tail` hätte
        // dieses hier still durchgelassen.
        let roh = r#"[{"name":"x","panes":1,"windows":1,"geheimnis":"im Kopf",
            "all_panes":[{"pane":1,"program":"bash","vorschau":"noch mehr Inhalt",
                          "zwischenablage":"ein Token"}]}]"#;

        let gefiltert = sitzungen_filtern(roh).expect("filtern");

        for fremd in ["geheimnis", "im Kopf", "vorschau", "zwischenablage", "ein Token"] {
            assert!(
                !gefiltert.contains(fremd),
                "\u{201e}{fremd}\u{201c} kam durch: {gefiltert}"
            );
        }
    }

    #[test]
    fn sitzungen_filter_nennt_eine_schlafende_sitzung_als_solche() {
        let roh = r#"[{"name":"alt","panes":4,"windows":2,"dormant":"stopped",
            "dormant_since":1700000000}]"#;

        let v: serde_json::Value =
            serde_json::from_str(&sitzungen_filtern(roh).unwrap()).unwrap();

        assert_eq!(v["sitzungen"][0]["ruht"], "stopped");
        // Keine Adressen: Dort ist nichts zu lesen, und erfundene Adressen
        // wären Arbeit, die ins Leere läuft.
        assert_eq!(v["sitzungen"][0]["pane_liste"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn sitzungen_filter_lehnt_unlesbares_ab_statt_es_durchzulassen() {
        assert!(sitzungen_filtern("keine Sitzungen hier").is_err());
        assert!(sitzungen_filtern(r#"{"name":"x"}"#).is_err());
        // Leer ist in Ordnung: ein Rechner ohne Sitzung ist eine Auskunft.
        let v: serde_json::Value = serde_json::from_str(&sitzungen_filtern("[]").unwrap()).unwrap();
        assert_eq!(v["sitzungen"].as_array().unwrap().len(), 0);
    }

    #[test]
    #[allow(non_snake_case)]
    fn sitzungen_filter_haelt_die_ECHTE_form_von_imkr_ls_json_fest() {
        // Wortwörtlich aus `imkr ls --json` auf codingmachine am 27.10.2026,
        // nur um ein `tail` ergänzt — das liefert imkr bei einem wartenden
        // Agenten, und dieser Testlauf hatte gerade keinen.
        //
        // **Gegen die Quelle, nicht gegen eine Beschreibung.** Beim Bau der
        // eingebetteten Oberfläche (#530) war ein erfundenes Protokoll der
        // teuerste Fehler des Tages. Ein Feld, das imkr umbenennt, fällt hier
        // auf — und nicht erst daran, dass ein Agent leere Programmnamen liest.
        let roh = r#"[{"name":"avatar-imkr","panes":2,"windows":2,
            "agents":{"working":1},
            "agent_panes":[{"pane":1,"program":"claude","workspace":2,"dir":"avatar",
              "state":"working","since":883,"meta":"opus 5 \u00b7 252k",
              "tail":"Soll ich das Passwort nehmen?","title":"Avatar findet imkr-Sessions"}],
            "all_panes":[
              {"pane":0,"program":"zsh","workspace":1,"dir":"codingmachine","state":"",
               "since":0,"title":"codingmachine@omarchy:~"},
              {"pane":1,"program":"claude","workspace":2,"dir":"avatar","state":"working",
               "since":883,"meta":"opus 5 \u00b7 252k",
               "tail":"Soll ich das Passwort nehmen?","title":"Avatar findet imkr-Sessions"}]}]"#;

        let gefiltert = sitzungen_filtern(roh).expect("filtern");

        // Das `tail` steckt hier in BEIDEN Listen. `agent_panes` ist eine
        // Teilmenge von `all_panes`; der Filter liest nur die zweite, und
        // damit kann die erste auch nichts durchlassen.
        assert!(
            !gefiltert.contains("Passwort") && !gefiltert.contains("agent_panes"),
            "{gefiltert}"
        );

        let v: serde_json::Value = serde_json::from_str(&gefiltert).unwrap();
        let panes = v["sitzungen"][0]["pane_liste"].as_array().unwrap();

        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0]["adresse"], "avatar-imkr/0");
        assert_eq!(panes[0]["programm"], "zsh");
        assert_eq!(panes[1]["adresse"], "avatar-imkr/1");
        assert_eq!(panes[1]["programm"], "claude");
        assert_eq!(panes[1]["titel"], "Avatar findet imkr-Sessions");
        assert_eq!(panes[1]["fenster"], 2);
    }

    #[test]
    #[allow(non_snake_case)]
    fn sitzungen_filter_bleibt_unter_dem_deckel_und_liefert_GUELTIGES_json() {
        // Der Fehler, den dieser Deckel verhindert: Ueber GRENZE_ERGEBNIS
        // greift `kuerzen`, und das behaelt das ENDE mit einem Vermerk davor.
        // Bei einer Befehlsausgabe ist das richtig; bei JSON ist es ein
        // Dokument, das der Agent nicht lesen kann — und er erfaehrt nicht,
        // warum.
        let mut sitzungen = Vec::new();

        for s in 0..40 {
            let panes: Vec<String> = (0..8)
                .map(|p| {
                    format!(
                        r#"{{"pane":{p},"program":"claude","workspace":{p},
                            "dir":"ein-ziemlich-langer-verzeichnisname-{s}",
                            "state":"working","since":1234,
                            "meta":"opus 5 · 300k",
                            "title":"ein Fenstertitel, der so lang ist wie echte Titel es sind {s}/{p}"}}"#
                    )
                })
                .collect();

            sitzungen.push(format!(
                r#"{{"name":"sitzung-nummer-{s}","panes":8,"windows":8,
                    "agents":{{"working":8}},"all_panes":[{}]}}"#,
                panes.join(",")
            ));
        }

        let roh = format!("[{}]", sitzungen.join(","));
        let gefiltert = sitzungen_filtern(&roh).expect("filtern");

        // 1 ─ Es passt unter die Grenze, die `kuerzen` sonst anwendet.
        assert!(
            json_laenge(&gefiltert) <= GRENZE_ERGEBNIS,
            "{} Zeichen",
            json_laenge(&gefiltert)
        );

        // 2 ─ Und es ist gueltiges JSON. Das ist der eigentliche Punkt.
        let v: serde_json::Value = serde_json::from_str(&gefiltert).expect("gueltiges JSON");

        // 3 ─ JEDE Sitzung steht da, auch die ohne Pane-Liste. Ein Agent, der
        //     nach einer Sitzung sucht, soll nicht erfahren, sie gaebe es nicht.
        assert_eq!(v["sitzungen"].as_array().unwrap().len(), 40);

        // 4 ─ Und es sagt, was fehlt — ausdruecklich, nicht durch Abwesenheit.
        let fehlt = v["gekuerzt"]["sitzungen_ohne_pane_liste"].as_u64().unwrap();
        assert!(fehlt > 0, "bei dieser Menge muss gekuerzt werden");
        assert!(fehlt < 40, "nicht alles, es passt ja etwas");

        let ohne: Vec<_> = v["sitzungen"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["pane_liste_fehlt"].as_bool() == Some(true))
            .collect();

        assert_eq!(ohne.len() as u64, fehlt);
        // Die weggelassenen stehen am ENDE, nicht verstreut: Wer hier eine
        // Rangfolge einbaut, entscheidet, welche Sitzung wichtiger ist.
        assert!(ohne[0]["pane_liste"].is_null());
        assert!(!v["sitzungen"][0]["pane_liste"].as_array().unwrap().is_empty());
    }

    #[test]
    fn eine_kleine_sitzungsliste_wird_nicht_angefasst() {
        // Gegenprobe: Der Deckel darf im Normalfall nichts tun. Sonst faellt
        // nicht auf, wenn er zu scharf eingestellt ist.
        let roh = r#"[{"name":"imkr","panes":1,"windows":1,
            "all_panes":[{"pane":0,"program":"zsh","workspace":1,"dir":"x","state":""}]}]"#;

        let v: serde_json::Value =
            serde_json::from_str(&sitzungen_filtern(roh).unwrap()).unwrap();

        assert!(v.get("gekuerzt").is_none(), "nichts zu kuerzen");
        assert!(v["sitzungen"][0].get("pane_liste_fehlt").is_none());
        assert_eq!(v["sitzungen"][0]["pane_liste"][0]["adresse"], "imkr/0");
    }

    #[test]
    fn eine_abgelehnte_sitzungsliste_findet_ihren_auftrag() {
        assert_eq!(
            abgelehnter_auftrag(r#"{"art":"sitzungen","auftrag":"a1"}"#),
            Some(serde_json::json!("a1"))
        );
    }

    #[test]
    fn sitzung_mit_bindestrich_vorn_ist_keine_adresse() {
        assert!(!pane_gueltig("-s/0"));
        assert!(!pane_gueltig("--help/1"));
        assert!(pane_gueltig("mein-bau/1"));
    }

    #[test]
    fn html_fehlerseite_landet_nicht_im_protokoll() {
        assert_eq!(
            grund_aus("<!doctype html>\n<html><title>NetBird Service</title>"),
            "HTML-Seite statt JSON (Proxy oder Neustart?)"
        );
        assert_eq!(grund_aus(r#"{"message":"zu gross"}"#), "zu gross");
        assert_eq!(grund_aus(&"x".repeat(1000)).len(), 300);
    }

    #[test]
    fn herzschlag_ist_klein_und_nennt_fehlendes_als_null() {
        let r: serde_json::Value = serde_json::from_str(&herzschlag_rumpf(None, None)).unwrap();
        assert_eq!(r["imkr"], serde_json::Value::Null);
        assert_eq!(r["pane"], serde_json::Value::Null);
        assert_eq!(r["version"], env!("CARGO_PKG_VERSION"));

        let r = herzschlag_rumpf(Some("0.21.70"), Some("default/0"));
        assert!(r.len() < 100);
    }

    #[test]
    fn lage_zaehlt_nur_mit_prozess_ausser_aufgelegt() {
        let lage = |stand: &str| Lage {
            stand: stand.to_string(),
            ..Lage::default()
        };
        assert!(lage_gilt(&lage("verbunden"), true));
        assert!(!lage_gilt(&lage("verbunden"), false));
        assert!(lage_gilt(&lage("aufgelegt"), false));

        let json = serde_json::to_string(&lage("verbunden")).unwrap();
        assert!(!json.contains("grund") && !json.contains("pane"));
    }

    #[test]
    fn seite_mit_eigener_seq_stoert_die_tasten_nicht() {
        let t = Instant::now();
        let mut o = Ordner::neu();
        let taste =
            |n: u64| format!(r#"{{"art":"tasten","pane":"imkr/3","b64":"YQ==","seq":{n}}}"#);
        assert_eq!(o.eingang(taste(0), t), vec![taste(0)]);
        let seite = r#"{"art":"seite","seq":7}"#.to_string();
        assert_eq!(o.eingang(seite.clone(), t), vec![seite]);
        assert_eq!(o.eingang(taste(1), t), vec![taste(1)]);
    }

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

        assert_eq!(schalter_stand(None, jetzt), Stand::Aus, "keine Datei = aus");
        assert_eq!(
            schalter_stand(Some("kaputt"), jetzt),
            Stand::Aus,
            "unlesbar = aus"
        );
        assert_eq!(
            schalter_stand(Some(r#"{"bis":999999}"#), jetzt),
            Stand::Aus,
            "abgelaufen = aus"
        );
        assert_eq!(
            schalter_stand(Some(r#"{"bis":1000000}"#), jetzt),
            Stand::Aus,
            "genau jetzt = aus"
        );
        assert_eq!(
            schalter_stand(
                Some(&format!(r#"{{"bis":{}}}"#, jetzt + HOECHSTDAUER + 1)),
                jetzt
            ),
            Stand::Aus,
            "jenseits der Höchstdauer = aus, nicht gekappt"
        );
        assert_eq!(
            schalter_stand(Some(r#"{"bis":1001800}"#), jetzt),
            Stand::VonHand(1_001_800)
        );
    }

    // ── Die Vorrangregel, ein Test je Zeile der Tabelle (#8932) ─────────────
    //
    // | veto | bis gültig | auto | Ergebnis |
    // |---|---|---|---|
    // | ja   | —    | —    | Veto — AUS        |
    // | nein | ja   | —    | VonHand — AN      |
    // | nein | nein | ja   | Automatisch — AN  |
    // | nein | nein | nein | Aus               |

    /// Zeile 1: Das Veto schlägt ALLES. Auch eine gültige Hand-Frist, auch
    /// eine Freigabe. Sonst wäre „aus" eine Bitte und keine Anweisung.
    #[test]
    fn zeile_1_das_veto_schlaegt_alles() {
        let jetzt = 1_000_000;

        for datei in [
            r#"{"bis":0,"auto":false,"veto":true}"#,
            r#"{"bis":1001800,"auto":false,"veto":true}"#,
            r#"{"bis":0,"auto":true,"veto":true}"#,
            r#"{"bis":1001800,"auto":true,"veto":true}"#,
        ] {
            let stand = schalter_stand(Some(datei), jetzt);

            assert_eq!(stand, Stand::Veto, "{datei}");
            assert!(!stand.an(), "{datei}");
        }
    }

    /// Zeile 2: Von Hand schlägt die Freigabe — und ist das Einzige mit Frist.
    #[test]
    fn zeile_2_von_hand_gilt_und_hat_eine_frist() {
        let jetzt = 1_000_000;

        assert_eq!(
            schalter_stand(Some(r#"{"bis":1001800,"auto":false,"veto":false}"#), jetzt),
            Stand::VonHand(1_001_800)
        );
        // Auch mit Freigabe daneben gewinnt die Hand: Sie nennt eine Frist,
        // und wer sie gesetzt hat, soll sie in `status` wiederfinden.
        assert_eq!(
            schalter_stand(Some(r#"{"bis":1001800,"auto":true,"veto":false}"#), jetzt),
            Stand::VonHand(1_001_800)
        );
        // Läuft die Hand-Frist ab, übernimmt die Freigabe — kein Loch.
        assert_eq!(
            schalter_stand(Some(r#"{"bis":999999,"auto":true,"veto":false}"#), jetzt),
            Stand::Automatisch
        );
    }

    /// Zeile 3: Automatisch an, ohne Frist. Die Bedingung steht in AI Brain.
    #[test]
    fn zeile_3_automatisch_gilt_ohne_frist() {
        let jetzt = 1_000_000;
        let stand = schalter_stand(Some(r#"{"bis":0,"auto":true,"veto":false}"#), jetzt);

        assert_eq!(stand, Stand::Automatisch);
        assert!(stand.an());
        // Kein „bis": Eine erfundene Ablaufzeit wäre eine Zahl, auf die sich
        // niemand verlassen könnte.
        assert!(!matches!(stand, Stand::VonHand(_)));
        // Und keine Höchstdauer, die sie nach 8 h fallen liesse: Das wäre
        // „Harness installieren = alles läuft" mit einem Verfallsdatum.
        assert_eq!(
            schalter_stand(
                Some(r#"{"bis":0,"auto":true,"veto":false}"#),
                jetzt + 100 * HOECHSTDAUER
            ),
            Stand::Automatisch
        );
    }

    /// Zeile 4: Nichts gesetzt ist aus — und unterscheidbar vom Veto.
    #[test]
    fn zeile_4_nichts_gesetzt_ist_aus_und_nicht_abgeschaltet() {
        let jetzt = 1_000_000;
        let stand = schalter_stand(Some(r#"{"bis":0,"auto":false,"veto":false}"#), jetzt);

        assert_eq!(stand, Stand::Aus);
        assert!(!stand.an());
        // Der Unterschied ist nicht kosmetisch: Beim Veto fragt der Dienst
        // nicht einmal nach der Freigabe, bei „aus" fragt er.
        assert_ne!(stand, Stand::Veto);
        assert_eq!(stand.wort(), "aus");
        assert_eq!(Stand::Veto.wort(), "abgeschaltet");
    }

    /// Eine Datei aus der Zeit vor dem Veto liest sich unverändert.
    #[test]
    fn eine_alte_schalterdatei_gilt_weiter() {
        // Genau so lag sie bis zum 27.10.2026 im Konfigurationsordner. Wer
        // aktualisiert, soll seine laufende Leitung nicht verlieren.
        assert_eq!(
            schalter_stand(Some(r#"{"bis":1001800}"#), 1_000_000),
            Stand::VonHand(1_001_800)
        );
    }

    // ── Die Auskunft von AI Brain (#8932) ──────────────────────────────────

    #[test]
    fn die_freigabe_steht_in_geraete_ich() {
        let mit = r#"{"id":"a5","name":"Bastian Laptop","besitzer":"Bastian",
            "imkr":{"freigegeben":true,"sehen":1,"befehlen":1,"channels":["avatar-1"]}}"#;

        assert_eq!(freigabe_aus_ich(mit), Some(true));
        assert_eq!(channels_aus_ich(mit), vec!["avatar-1".to_string()]);

        let ohne = r#"{"id":"a5","name":"Bastian Laptop",
            "imkr":{"freigegeben":false,"sehen":0,"befehlen":0,"channels":[]}}"#;

        assert_eq!(freigabe_aus_ich(ohne), Some(false));
        assert!(channels_aus_ich(ohne).is_empty());
    }

    #[test]
    #[allow(non_snake_case)]
    fn eine_instanz_ohne_das_feld_sagt_NICHT_nein() {
        // Der Unterschied, an dem alles hängt: `None` heisst „keine Auskunft",
        // und daraufhin öffnet die automatische Leitung nicht — aber sie legt
        // auch keine auf, die zu Recht stand. Eine Instanz vor #8932 antwortet
        // genau so.
        let alt = r#"{"id":"a5","name":"Bastian Laptop","besitzer":"Bastian"}"#;

        assert_eq!(freigabe_aus_ich(alt), None);
        assert_eq!(freigabe_aus_ich("keine Antwort"), None);
        assert_eq!(freigabe_aus_ich(r#"{"imkr":null}"#), None);
        assert!(channels_aus_ich(alt).is_empty());
    }

    #[test]
    fn der_herzschlag_bringt_auch_die_namen_mit() {
        let mit = r#"{"freigabe":true,"channels":["avatar-1","werkstatt"]}"#;

        assert_eq!(
            channels_im_schlag(mit),
            Some(vec!["avatar-1".to_string(), "werkstatt".to_string()])
        );
        // Leere Liste heisst „derzeit keine" …
        assert_eq!(
            channels_im_schlag(r#"{"freigabe":false,"channels":[]}"#),
            Some(vec![])
        );
        // … das FEHLENDE Feld heisst „keine Auskunft". Eine Instanz vor #8932
        // sagt nichts dazu, und dann soll die Leiste behalten, was sie hat.
        assert_eq!(channels_im_schlag(r#"{"freigabe":true}"#), None);
        assert_eq!(channels_im_schlag(""), None);
    }

    #[test]
    fn der_herzschlag_bringt_die_freigabe_mit() {
        assert_eq!(freigabe_im_schlag(r#"{"freigabe":true}"#), Some(true));
        assert_eq!(freigabe_im_schlag(r#"{"freigabe":false}"#), Some(false));
        // 204 ohne Rumpf: eine Instanz vor #8932. Keine Aussage, also keine
        // Änderung am Schalter — sonst legte ein Teil-Ausrollen jede
        // automatische Leitung auf.
        assert_eq!(freigabe_im_schlag(""), None);
        assert_eq!(freigabe_im_schlag("<html>502</html>"), None);
        assert_eq!(freigabe_im_schlag(r#"{"freigabe":"ja"}"#), None);
    }

    /// Und die neue Datei bleibt für einen ÄLTEREN Harness lesbar.
    #[test]
    fn die_neue_schalterdatei_bricht_einen_aelteren_harness_nicht() {
        // Der alte verlangte `bis` als Pflichtfeld. Deshalb schreibt
        // `schalter_aendern` alle drei Felder, auch die leeren: Eine Datei
        // ohne `bis` wäre für ihn unlesbar — was ihn zwar auf AUS stellt
        // (richtig), aber er soll die Lage überhaupt beurteilen können.
        #[derive(serde::Deserialize)]
        struct Alt {
            bis: i64,
        }

        let neu = serde_json::to_string(&Schalter {
            bis: 0,
            auto: true,
            veto: false,
        })
        .unwrap();

        let alt: Alt = serde_json::from_str(&neu).expect("der alte Harness liest die Datei");

        // Er sieht `bis: 0`, also „abgelaufen", also aus. Eine automatische
        // Leitung geht einem alten Harness verloren — sie geht ihm nicht
        // versehentlich an.
        assert_eq!(alt.bis, 0);
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
