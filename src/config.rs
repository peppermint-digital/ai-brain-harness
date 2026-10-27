//! Ablage der Gerätezugangsdaten und Eintrag in Claude Code.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub base_url: String,
    pub instance_name: String,
    pub device_name: String,
    pub client_id: String,
    pub client_secret: String,
    pub token_endpoint: String,
    pub mcp_url: String,
    pub scope: String,
    pub owner: String,
}

/// Das Heimatverzeichnis des Benutzers.
///
/// Windows setzt `HOME` nicht — dort heißt es `USERPROFILE`. Ohne diesen Zweig
/// bricht das Programm auf Windows vor der ersten Aufgabe ab, mit einer Meldung
/// über eine Variable, die dort niemand kennt.
pub fn heimat() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| "Weder HOME noch USERPROFILE ist gesetzt.".to_string())
}

pub fn config_dir() -> Result<PathBuf, String> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg).join("ai-brain-cli"));
    }

    // Unter Windows gehört so etwas nach %APPDATA%, nicht in ein verstecktes
    // Verzeichnis im Heimatordner. XDG gibt es dort nicht.
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return Ok(PathBuf::from(appdata).join("ai-brain-cli"));
        }
    }

    Ok(heimat()?.join(".config").join("ai-brain-cli"))
}

pub fn config_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("connection.json"))
}

/// Der Ordner, unter dem bis zur ersten Umbenennung alles lag (#7664).
///
/// **Der Name der Ablage bleibt `ai-brain-harness`, auch nachdem das Programm
/// `ai-brain-harness` heisst.** Das ist Absicht: Die Ablage ist unsichtbar, und
/// ein zweiter Datenumzug auf drei Rechnern waere Risiko ohne Gegenwert.
/// `install.sh` schreibt `instanz` hinein, BEVOR das Programm das erste Mal
/// laeuft — eine Umbenennung muesste also auf beiden Seiten gleichzeitig
/// stimmen, sonst sucht das Programm an einer Stelle, die der Installer nie
/// befuellt hat. Wer es doch aufraeumen will, erweitert diese Funktion um eine
/// LISTE alter Namen statt den einen zu ersetzen.
fn alter_config_dir() -> Result<PathBuf, String> {
    let neu = config_dir()?;
    let name = neu
        .file_name()
        .ok_or_else(|| format!("{} hat keinen Namen.", neu.display()))?;

    if name != "ai-brain-cli" {
        return Err("Unerwarteter Name der Ablage.".to_string());
    }

    Ok(neu.with_file_name("ai-brain-companion"))
}

/// Holt die Ablage des alten Namens herüber — einmalig, beim ersten Lauf (#7664).
///
/// Die Bedingung hängt an der **Verbindung**, nicht am Ordner. Das ist kein
/// Feinschliff: `install.sh` legt die Ablage selbst an und schreibt `instanz`
/// hinein, bevor dieses Programm das erste Mal läuft. Eine Prüfung auf „neuer
/// Ordner existiert nicht" würde nach jedem Installer-Lauf schweigen — und die
/// Zugangsdaten blieben unter dem alten Namen liegen, während das Programm
/// „Dieser Rechner ist noch nicht verbunden" meldet.
///
/// Es zieht der **ganze Inhalt** um, nicht nur `connection.json`: daneben liegen
/// `sync-state.json` (welcher Eintrag in welcher Fassung hier liegt) und
/// `gesicherte-fassungen/`. Bliebe der Abgleichstand zurück, meldete `sync`
/// anschliessend jeden Eintrag als geändert — und `--force` schöbe die Arbeit
/// der Nutzer als `.lokal` weg, obwohl sich nichts getan hat.
///
/// Was schon im neuen Ordner liegt, bleibt liegen: Die `instanz`, die der
/// Installer gerade geschrieben hat, nennt die Instanz, von der der Nutzer
/// eben installiert hat — die ist aktueller als die alte.
pub fn uebernimm_alte_ablage() {
    let (Ok(neu), Ok(alt)) = (config_dir(), alter_config_dir()) else {
        return;
    };

    if neu.join("connection.json").exists() || !alt.join("connection.json").exists() {
        return;
    }

    if let Err(e) = fs::create_dir_all(&neu) {
        eprintln!(
            "ai-brain-harness: {} lässt sich nicht anlegen ({e}).",
            neu.display()
        );
        return;
    }

    let Ok(eintraege) = fs::read_dir(&alt) else {
        eprintln!("ai-brain-harness: {} lässt sich nicht lesen.", alt.display());
        return;
    };

    let mut geholt = 0usize;
    let mut stehengeblieben = 0usize;

    for eintrag in eintraege.flatten() {
        let ziel = neu.join(eintrag.file_name());

        if ziel.exists() {
            stehengeblieben += 1;
            continue;
        }

        match fs::rename(eintrag.path(), &ziel) {
            Ok(()) => geholt += 1,
            Err(e) => {
                stehengeblieben += 1;
                eprintln!(
                    "ai-brain-harness: {} liess sich nicht übernehmen ({e}).",
                    eintrag.path().display()
                );
            }
        }
    }

    // Nur wenn nichts zurückblieb, ist der alte Ordner wirklich erledigt.
    // `remove_dir` (nicht `_all`) ist Absicht: Es darf nur ein LEERES
    // Verzeichnis verschwinden — alles andere wäre ein Löschweg für Daten,
    // die wir gerade nicht holen konnten.
    if stehengeblieben == 0 {
        let _ = fs::remove_dir(&alt);
    }

    eprintln!(
        "ai-brain-harness: Ablage übernommen — {} Eintrag/Einträge aus {} nach {}{}",
        geholt,
        alt.display(),
        neu.display(),
        if stehengeblieben > 0 {
            format!(", {stehengeblieben} blieb(en) stehen")
        } else {
            String::new()
        }
    );
}

pub fn save(connection: &Connection) -> Result<PathBuf, String> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir)
        .map_err(|e| format!("{} lässt sich nicht anlegen: {e}", dir.display()))?;

    let path = config_path()?;
    let json = serde_json::to_string_pretty(connection)
        .map_err(|e| format!("Verbindung lässt sich nicht schreiben: {e}"))?;

    fs::write(&path, json)
        .map_err(|e| format!("{} lässt sich nicht schreiben: {e}", path.display()))?;

    // Die Datei enthält ein Client-Secret. Auf Unix eng stellen, damit sie
    // nicht für andere Konten des Rechners lesbar ist. Ein Schlüsselbund wäre
    // besser und steht auf der Liste — aber lieber jetzt 0600 als später
    // vielleicht Keyring.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }

    Ok(path)
}

pub fn load() -> Result<Connection, String> {
    let path = config_path()?;

    let raw = fs::read_to_string(&path).map_err(|_| {
        "Dieser Rechner ist noch nicht verbunden.\n\
         Führe zuerst aus:  ai-brain-harness connect <domain>"
            .to_string()
    })?;

    serde_json::from_str(&raw).map_err(|e| format!("{} ist beschädigt: {e}", path.display()))
}

pub fn print_status() -> Result<(), String> {
    let connection = load()?;

    println!("Verbunden mit : {}", connection.base_url);
    println!("Instanz       : {}", connection.instance_name);
    println!("Gerät         : {}", connection.device_name);
    println!("Zugerechnet an: {}", connection.owner);
    println!("Client-ID     : {}", connection.client_id);
    println!("MCP-Endpunkt  : {}", connection.mcp_url);

    Ok(())
}

pub fn disconnect() -> Result<(), String> {
    let path = config_path()?;

    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| format!("{} lässt sich nicht löschen: {e}", path.display()))?;
        println!("Lokale Verbindung entfernt.");
    } else {
        println!("Es war keine Verbindung hinterlegt.");
    }

    println!();
    println!("Hinweis: Der Gerätezugang besteht auf der Instanz weiter.");
    println!("Endgültig entziehen kannst du ihn unter „Meine Geräte“.");

    Ok(())
}

/// Wo Claude Codes Konfiguration liegt — und warum NICHT `CLAUDE_CONFIG_DIR`.
///
/// Immer `~/.claude.json`, auch wenn `CLAUDE_CONFIG_DIR` gesetzt ist. Das war
/// bis zum 27.10.2026 keine Entscheidung, sondern nur so gebaut; seit #8949
/// ist es eine, und deshalb steht sie hier.
///
/// Auf Bastians Rechnern ist `CLAUDE_CONFIG_DIR` ausschliesslich **in
/// imkr-Panes** gesetzt und zeigt dort auf das Konto des Panes
/// (`~/.local/state/imkr/accounts/claude/<name>/`); in keiner Shell-Startdatei
/// steht es. Gemessen am 27.10.2026 auf Laptop und codingmachine.
///
/// Würden wir ihm folgen, schrieben wir den MCP-Eintrag in ein Kontoverzeichnis
/// — und `imkr account sync` ersetzt dort die ganze `mcpServers`-Struktur mit
/// der aus `~/.claude.json`. Der Eintrag wäre beim nächsten Abgleich weg, und
/// zwar still. Begründung ausführlich an {@link imkr_konten_versorgen}.
///
/// **Wer das später „korrigiert", baut genau diesen Fehler ein.** Der Weg zu
/// den Konten führt über imkr, nicht über diese Funktion.
pub fn claude_config_path() -> Result<PathBuf, String> {
    Ok(heimat()?.join(".claude.json"))
}

/// Trägt die CLI als MCP-Server in Claude Code ein.
///
/// Bewusst mit Sicherung vorher: `~/.claude.json` gehört Claude Code, enthält
/// weit mehr als unseren Eintrag, und ein kaputtes JSON dort kostet den Nutzer
/// seine gesamte Konfiguration. Wir lesen, ergänzen genau einen Schlüssel und
/// schreiben zurück — alles andere bleibt unangetastet.
/// Namen, unter denen dieses Programm schon gelaufen ist.
///
/// Gebraucht, um alte MCP-Eintraege zu erkennen. Auf Laptop und Desktop zeigte
/// der Eintrag am 15.10.2026 noch auf `ai-brain-companion` vom August — das
/// Programm lag dort nicht mehr, und AI Brain war in Claude Code deshalb
/// stillschweigend getrennt. Der Eintrag SAH richtig aus; nur der Pfad hinter
/// ihm war tot.
const EIGENE_NAMEN: &[&str] = &["ai-brain-harness", "ai-brain-cli", "ai-brain-companion"];

/// Zeigt dieser Befehl auf eine unserer Fassungen?
///
/// **Beide Trennzeichen selbst behandeln, nicht `Path` fragen.** `Path` richtet
/// sich nach dem System, auf dem es laeuft: Unter Linux ist `C:\Programs\x.exe`
/// EIN Dateiname mit Backslashes darin, und die Pruefung schlaegt fehl. Dieselbe
/// Konfiguration kann aber von einem Windows-Rechner stammen — und soll dort
/// wie hier gleich bewertet werden.
fn ist_unser_programm(befehl: &str) -> bool {
    let letzter = befehl
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(befehl);

    // Endung abschneiden, aber nur eine bekannte: ein Punkt im Namen selbst
    // darf nicht zum Kuerzen fuehren.
    let name = letzter.strip_suffix(".exe").unwrap_or(letzter);

    EIGENE_NAMEN.contains(&name)
}

/// Raeumt MCP-Eintraege weg, die unter einem ANDEREN Schluessel auf eine
/// unserer Fassungen zeigen.
///
/// **Warum das noetig ist.** Der Eintrag unter `ai-brain` wird beim Schreiben
/// ohnehin ersetzt. Ein Eintrag unter `ai-brain-companion` oder `ai-brain-cli`
/// bleibt dagegen stehen — und dann haengen zwei Server am Werkzeug, die beide
/// dieselbe Instanz anbieten. Doppelte Werkzeugnamen sind keine Kleinigkeit:
/// das Modell sieht jedes Werkzeug zweimal und muss raten, welches gilt.
///
/// Was NICHT angefasst wird: fremde Server. Ein Eintrag, dessen Befehl nicht
/// auf eines unserer Programme zeigt, gehoert jemand anderem.
fn alte_eintraege_raeumen(server: &mut serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    let zu_loeschen: Vec<String> = server
        .iter()
        .filter(|(schluessel, _)| schluessel.as_str() != "ai-brain")
        .filter(|(_, wert)| {
            wert.get("command")
                .and_then(|c| c.as_str())
                .map(ist_unser_programm)
                .unwrap_or(false)
        })
        .map(|(schluessel, _)| schluessel.clone())
        .collect();

    for schluessel in &zu_loeschen {
        server.remove(schluessel);
    }

    zu_loeschen
}

/// Schreibt den MCP-Eintrag und raeumt alte Fassungen weg.
///
/// Gibt den Pfad und die Schluessel zurueck, die dabei verschwunden sind —
/// stillschweigend etwas aus einer fremden Konfiguration zu loeschen waere
/// genau die Sorte Hilfsbereitschaft, die man spaeter nicht nachvollziehen kann.
pub fn write_claude_config_mit_bericht() -> Result<(PathBuf, Vec<String>), String> {
    let path = claude_config_path()?;

    let mut root: serde_json::Value = if path.exists() {
        let raw = fs::read_to_string(&path)
            .map_err(|e| format!("{} lässt sich nicht lesen: {e}", path.display()))?;

        let backup = path.with_extension("json.vor-companion");
        fs::write(&backup, &raw)
            .map_err(|e| format!("Sicherung {} schlug fehl: {e}", backup.display()))?;

        serde_json::from_str(&raw)
            .map_err(|e| format!("{} enthält kein gültiges JSON: {e}", path.display()))?
    } else {
        serde_json::json!({})
    };

    let executable = std::env::current_exe()
        .map_err(|e| format!("Eigener Programmpfad nicht ermittelbar: {e}"))?;

    let server = root
        .as_object_mut()
        .ok_or_else(|| format!("{} ist kein JSON-Objekt.", path.display()))?
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "mcpServers ist kein JSON-Objekt.".to_string())?;

    let geraeumt = alte_eintraege_raeumen(server);

    server.insert(
        "ai-brain".to_string(),
        serde_json::json!({
            "type": "stdio",
            "command": executable.to_string_lossy(),
            "args": ["mcp"],
        }),
    );

    let json = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Konfiguration lässt sich nicht schreiben: {e}"))?;

    fs::write(&path, json)
        .map_err(|e| format!("{} lässt sich nicht schreiben: {e}", path.display()))?;

    Ok((path, geraeumt))
}

/// Wie {@link write_claude_config_mit_bericht}, ohne den Bericht.
pub fn write_claude_config() -> Result<PathBuf, String> {
    write_claude_config_mit_bericht().map(|(pfad, _)| pfad)
}

/// Steht unser MCP-Eintrag wirklich in Claude Codes Konfiguration? (#8949)
///
/// **Gelesen, nicht erinnert.** Der Einrichtungsassistent berichtet am Ende,
/// was eingetragen ist — und eine Zeile, die nur den eigenen Vorsatz
/// wiedergibt („wir haben geschrieben"), ist genau die Sorte Anzeige, die
/// technisch richtig ist und nichts beantwortet.
///
/// Geprüft wird auch, dass der Befehl auf UNSER Programm zeigt: Ein Eintrag,
/// der auf eine frühere Fassung an einem Pfad zeigt, der nicht mehr existiert,
/// sieht in der Datei richtig aus und ist tot. Genau das lag am 15.10.2026 auf
/// Laptop und Desktop ({@link EIGENE_NAMEN}).
pub fn claude_hat_eintrag() -> bool {
    claude_config_path()
        .map(|p| datei_hat_eintrag(&p.to_string_lossy()))
        .unwrap_or(false)
}

/// Was ein Schreibversuch an einem Werkzeug ausgerichtet hat (#1123).
///
/// **Warum es dafuer eine Unterscheidung braucht.** Bis v0.5.0 meldeten Codex
/// und `pi` bei JEDEM Lauf „Eintrag „ai-brain" angelegt" — auch beim zweiten,
/// dritten, zehnten. Beide Werkzeuge ersetzen einen vorhandenen Eintrag
/// stillschweigend, und die Zeile gab den Vorsatz wieder statt des Ergebnisses.
/// Gefunden beim ersten echten Lauf auf Bastians Rechnern (Bug #1123).
///
/// Seitdem wird vorher nachgesehen. Steht der Eintrag schon genau so da, wird
/// gar nicht geschrieben: Ein Einrichtungsbefehl, den man zweimal aufruft,
/// soll beim zweiten Mal nichts tun und das auch sagen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eintragsstand {
    /// Stand schon genau so da — nichts geschrieben.
    Schon,
    /// Es gab keinen; jetzt steht er da.
    Angelegt,
    /// Es gab einen, aber er zeigte woanders hin.
    Erneuert,
    /// Geschrieben und danach nicht wiedergefunden. Das ist ein Befund, kein
    /// Erfolg — die Zeile sagt es, statt „eingetragen" zu behaupten.
    Verschwunden,
}

impl Eintragsstand {
    /// Ein ganzer Satz fuer die Zeile beim Schritt.
    pub fn wort(self) -> &'static str {
        match self {
            Eintragsstand::Schon => "Eintrag „ai-brain“ stand schon drin",
            Eintragsstand::Angelegt => "Eintrag „ai-brain“ angelegt",
            Eintragsstand::Erneuert => "Eintrag „ai-brain“ erneuert (zeigte vorher woanders hin)",
            Eintragsstand::Verschwunden => {
                "geschrieben, aber nicht wiedergefunden — bitte nachsehen"
            }
        }
    }

    /// Dasselbe in Kurzform, fuer eine Uebersichtszeile.
    pub fn kurz(self) -> &'static str {
        match self {
            Eintragsstand::Schon => "schon drin",
            Eintragsstand::Angelegt => "eingetragen",
            Eintragsstand::Erneuert => "erneuert",
            Eintragsstand::Verschwunden => "geschrieben, aber nicht wiedergefunden",
        }
    }
}

/// Der eigene Programmpfad, so wie er in einen MCP-Eintrag kommt.
fn eigener_pfad() -> Result<String, String> {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| format!("Eigener Programmpfad nicht ermittelbar: {e}"))
}

/// Zeigt der Eintrag „ai-brain" in dieser `mcpServers`-Datei genau auf uns?
///
/// `None` heisst „es gibt keinen" — das ist etwas anderes als „es gibt einen,
/// der woanders hinzeigt", und nur mit beidem auseinander laesst sich
/// „angelegt" von „erneuert" unterscheiden.
///
/// Verglichen wird der **ganze** Pfad, nicht nur der Dateiname wie in
/// {@link ist_unser_programm}: Die Frage ist hier nicht „gehoert der uns",
/// sondern „muss ich schreiben". Ein Eintrag auf eine frueher installierte
/// Fassung an einem anderen Ort gehoert uns und ist trotzdem falsch.
fn mcp_json_zeigt_auf(pfad: &Path, unser: &str) -> Option<bool> {
    let roh = fs::read_to_string(pfad).ok()?;
    let wert: serde_json::Value = serde_json::from_str(&roh).ok()?;
    let eintrag = wert.get("mcpServers")?.get("ai-brain")?;

    Some(befehl_zeigt_auf(eintrag, unser))
}

/// Dieselbe Frage fuer einen einzelnen Eintrag, egal woher er kommt.
///
/// Die Argumente zaehlen mit: Ein Eintrag auf das richtige Programm ohne
/// `mcp` startet keinen Server, sieht in der Datei aber richtig aus.
fn befehl_zeigt_auf(eintrag: &serde_json::Value, unser: &str) -> bool {
    let befehl = eintrag.get("command").and_then(|c| c.as_str());

    let args: Vec<&str> = eintrag
        .get("args")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|w| w.as_str()).collect())
        .unwrap_or_default();

    befehl == Some(unser) && args == ["mcp"]
}

/// Steht unser Eintrag in Claude Codes Konfiguration schon genau so da?
///
/// Fuer die Wortwahl des Einrichtungsassistenten: Geschrieben wird dort
/// weiterhin in jedem Fall (das Schreiben raeumt auch alte Schluessel weg),
/// aber „eingetragen" und „unveraendert" sind zwei verschiedene Auskuenfte.
pub fn claude_eintrag_genau() -> bool {
    let Ok(pfad) = claude_config_path() else {
        return false;
    };

    let Ok(unser) = eigener_pfad() else {
        return false;
    };

    mcp_json_zeigt_auf(&pfad, &unser).unwrap_or(false)
}

// ----------------------------------------------------------------- codex ----

/// Traegt den MCP-Server in Codex ein — ueber dessen eigenes Werkzeug.
///
/// Codex bringt `codex mcp add` mit und kennt sein TOML-Format selbst. Die
/// `~/.codex/config.toml` von Hand zu schreiben waere die schlechtere Loesung:
/// Sie enthaelt bereits `[features]` und `[projects."…"]`-Abschnitte, und ein
/// selbstgebauter Schreiber muesste die alle heil lassen.
///
/// Fehlt Codex auf dem Rechner, ist das kein Fehler — dann gibt es nichts
/// einzutragen.
pub fn write_codex_config() -> Result<Option<Eintragsstand>, String> {
    if which("codex").is_none() {
        return Ok(None);
    }

    let pfad = eigener_pfad()?;
    let vorher = codex_zeigt_auf(&pfad);

    // Steht er schon genau so da, wird nicht geschrieben (#1123). Das ist
    // nicht nur Hoeflichkeit gegenueber der Datei: `codex mcp remove` + `add`
    // raeumt den Eintrag kurz weg, und ein Lauf, der nichts aendern soll,
    // soll auch kein Fenster oeffnen, in dem er fehlt.
    if vorher == Some(true) {
        return Ok(Some(Eintragsstand::Schon));
    }

    // Ein vorhandener Eintrag laesst sich nicht ueberschreiben — erst weg,
    // dann neu. Schlaegt das Entfernen fehl, war keiner da; das ist der
    // Normalfall und kein Grund zur Sorge.
    let _ = std::process::Command::new("codex")
        .args(["mcp", "remove", "ai-brain"])
        .output();

    let ergebnis = std::process::Command::new("codex")
        .args(["mcp", "add", "ai-brain", "--", &pfad, "mcp"])
        .output()
        .map_err(|e| format!("codex liess sich nicht aufrufen: {e}"))?;

    if !ergebnis.status.success() {
        let fehler = String::from_utf8_lossy(&ergebnis.stderr);

        return Err(format!("codex mcp add schlug fehl: {}", fehler.trim()));
    }

    Ok(Some(nachgelesen(vorher, codex_zeigt_auf(&pfad))))
}

/// Zeigt Codex' Eintrag „ai-brain" genau auf uns? `None` = es gibt keinen.
///
/// **Gefragt wird Codex selbst, nicht seine `config.toml`.** Das Format dort
/// gehoert ihm (`[mcp_servers.ai-brain]`, dazu `[features]` und
/// `[projects."…"]`), und `codex mcp list --json` ist die Auskunft, die er
/// dafuer anbietet — sie liest nur und ist sofort da.
fn codex_zeigt_auf(unser: &str) -> Option<bool> {
    let aus = std::process::Command::new("codex")
        .args(["mcp", "list", "--json"])
        .output()
        .ok()?;

    if !aus.status.success() {
        return None;
    }

    let liste: Vec<serde_json::Value> = serde_json::from_slice(&aus.stdout).ok()?;

    let eintrag = liste
        .iter()
        .find(|e| e.get("name").and_then(|n| n.as_str()) == Some("ai-brain"))?;

    // Codex haengt den Befehl unter `transport` ein, nicht an die Wurzel.
    let transport = eintrag.get("transport")?;

    Some(befehl_zeigt_auf(transport, unser))
}

/// Aus Vorher und Nachher einen Stand machen — fuer jedes Werkzeug gleich.
///
/// Die Reihenfolge ist wichtig: Erst wird geprueft, ob es jetzt stimmt. Eine
/// Zeile „angelegt" nach einem Schreibversuch, der nicht angekommen ist, ist
/// genau der Fehler, um den es in #1123 geht.
fn nachgelesen(vorher: Option<bool>, nachher: Option<bool>) -> Eintragsstand {
    if nachher != Some(true) {
        return Eintragsstand::Verschwunden;
    }

    match vorher {
        None => Eintragsstand::Angelegt,
        _ => Eintragsstand::Erneuert,
    }
}

// -------------------------------------------------------------------- pi ----

/// Traegt den MCP-Server in `pi` ein — ueber dessen eigenes Werkzeug (#8949).
///
/// Dieselbe Form wie bei Codex, und aus demselben Grund: `pi mcp add <name>
/// -- <befehl> <args>` kennt sein eigenes Dateiformat
/// (`~/.pi/agent/mcp.json`). Wer es von Hand schreibt, uebernimmt die Pflege
/// eines Formats, das ihm nicht gehoert.
///
/// Fehlt `pi`, ist das kein Fehler, sondern der Normalfall.
pub fn write_pi_config() -> Result<Option<Eintragsstand>, String> {
    if which("pi").is_none() {
        return Ok(None);
    }

    let pfad = eigener_pfad()?;
    let datei = pi_mcp_pfad()?;
    let vorher = mcp_json_zeigt_auf(&datei, &pfad);

    if vorher == Some(true) {
        return Ok(Some(Eintragsstand::Schon));
    }

    // `add` ersetzt laut eigener Hilfe einen vorhandenen Eintrag („Add or
    // replace a server in mcp.json"), ein `remove` davor ist also nicht
    // nötig — anders als bei Codex.
    let ergebnis = std::process::Command::new("pi")
        .args(["mcp", "add", "ai-brain", "--", &pfad, "mcp"])
        .output()
        .map_err(|e| format!("pi liess sich nicht aufrufen: {e}"))?;

    if !ergebnis.status.success() {
        let fehler = String::from_utf8_lossy(&ergebnis.stderr);

        return Err(format!("pi mcp add schlug fehl: {}", fehler.trim()));
    }

    Ok(Some(nachgelesen(
        vorher,
        mcp_json_zeigt_auf(&datei, &pfad),
    )))
}

/// Wo `pi` seine MCP-Server fuehrt.
///
/// Aus seiner eigenen Hilfe: „Reads ~/.pi/agent/mcp.json and, in trusted
/// projects, .pi/mcp.json." Nur die globale Datei zaehlt hier — die
/// projektlokale gehoert dem Projekt, und wir richten den Rechner ein.
///
/// Gemessen am 27.10.2026 in einem Schein-HOME: Die Datei hat dieselbe Form
/// wie `~/.claude.json` (`{"mcpServers": {"<name>": {"command", "args"}}}`),
/// deshalb liest sie derselbe Helfer.
fn pi_mcp_pfad() -> Result<PathBuf, String> {
    Ok(heimat()?.join(".pi").join("agent").join("mcp.json"))
}

// ------------------------------------------------------------ imkr-Konten ----

/// Die MCP-Einträge in die imkr-Konten tragen — durch imkr selbst (#8949).
///
/// ## Warum NICHT in die Kontoverzeichnisse geschrieben wird
///
/// `imkr account` führt je Konto ein eigenes `.claude.json` (unter
/// `~/.local/state/imkr/accounts/claude/<name>/`), weil die Identität des
/// angemeldeten Kontos darin steht. Ein MCP-Eintrag in `~/.claude.json`
/// erreicht diese Konten also nicht von selbst.
///
/// Der naheliegende Weg wäre, in jedes Kontoverzeichnis zu schreiben. Das ist
/// **falsch**, und zwar nachweisbar: `imkr account sync` ruft für jedes Konto
/// `share_claude_json`, und das macht bei einer bestehenden Kontodatei
///
/// ```text
/// own.insert("mcpServers".into(), servers.clone());
/// ```
///
/// — es ERSETZT die ganze Struktur mit der aus `~/.claude.json`. Ein Eintrag,
/// den nur das Konto kennt, ist beim nächsten `sync` weg. Still, und bei einem
/// Konto, das vorher funktionierte.
///
/// Also eine Wahrheit (`~/.claude.json`) und ein Verteiler (`imkr account
/// sync`). Wir sind nicht der zweite Schreiber.
///
/// Gilt auch, wenn dieses Programm selbst in einem imkr-Pane läuft: Dort steht
/// `CLAUDE_CONFIG_DIR` auf das Konto des Panes, und genau diesen Pfad darf
/// {@link claude_config_path} nicht nehmen. Sie tut es nicht — siehe dort.
///
/// Fehlt imkr, gibt es keine Konten zu versorgen.
pub fn imkr_konten_versorgen() -> Result<Option<String>, String> {
    if which("imkr").is_none() {
        return Ok(None);
    }

    let ergebnis = std::process::Command::new("imkr")
        .args(["account", "sync"])
        .output()
        .map_err(|e| format!("imkr liess sich nicht aufrufen: {e}"))?;

    if !ergebnis.status.success() {
        let fehler = String::from_utf8_lossy(&ergebnis.stderr);

        return Err(format!("imkr account sync schlug fehl: {}", fehler.trim()));
    }

    Ok(Some(String::from_utf8_lossy(&ergebnis.stdout).trim().to_string()))
}

/// Läuft gerade ein Programm aus dem Bauverzeichnis? (#8949)
///
/// ## Warum es diesen Riegel gibt — der Vorfall vom 27.10.2026
///
/// Beim Bauen des Einrichtungsassistenten habe ich ihn auf diesem Rechner mit
/// echtem `HOME` laufen lassen, damit imkr seine Konten findet. Ergebnis:
/// `~/.claude.json`, `~/.codex/config.toml` und alle drei imkr-Konten zeigten
/// danach auf `target/debug/ai-brain-harness` statt auf das installierte
/// Programm — und `~/.pi/agent/mcp.json` lag neu da, wo vorher keine Datei war.
///
/// Das ist nicht „Pech beim Testen". Es ist die eingebaute Folge davon, dass
/// der MCP-Eintrag {@link std::env::current_exe} nimmt, und die ist richtig:
/// Der Eintrag soll auf **dieses** Programm zeigen. Nur ist „dieses Programm"
/// beim Entwickeln ein anderes als beim Benutzen.
///
/// Der Schaden ist still: Die Datei sieht richtig aus, der Pfad existiert
/// sogar — bis `cargo clean` läuft oder der Zweig wechselt. Dann ist AI Brain
/// in Claude Code weg, und niemand verbindet das mit einem Testlauf von vor
/// drei Tagen. Genau diese Form hatte schon der Befund vom 15.10.2026
/// ({@link EIGENE_NAMEN}): ein Eintrag, der richtig aussieht und ins Leere
/// zeigt.
///
/// Deshalb: Wer aus dem Bauverzeichnis schreibt, muss es ausdrücklich wollen.
///
/// `AI_BRAIN_HARNESS_AUS_DEM_BAUM=1` hebt den Riegel auf — für den
/// berechtigten Fall, nämlich einen Lauf in einem eigenen `HOME`, wo nichts
/// Echtes getroffen wird.
pub fn aus_dem_bauverzeichnis() -> Option<PathBuf> {
    if std::env::var_os("AI_BRAIN_HARNESS_AUS_DEM_BAUM").is_some() {
        return None;
    }

    let eigenes = std::env::current_exe().ok()?;

    ist_bauverzeichnis(&eigenes).then_some(eigenes)
}

/// Liegt dieser Pfad in `target/debug` oder `target/release`?
///
/// Rein, damit die Erkennung ohne einen echten Prozess prüfbar ist. Geprüft
/// wird das ELTERNPAAR und nicht ein Pfadanfang: Ein fester Anfang gälte nur
/// für dieses Arbeitsverzeichnis, und `CARGO_TARGET_DIR` kann überall liegen.
fn ist_bauverzeichnis(pfad: &std::path::Path) -> bool {
    let Some(eltern) = pfad.parent() else {
        return false;
    };

    let Some(name) = eltern.file_name() else {
        return false;
    };

    if !matches!(name.to_string_lossy().as_ref(), "debug" | "release") {
        return false;
    }

    // Noch eine Ebene höher muss `target` stehen — sonst wäre jedes
    // Verzeichnis namens „release" betroffen.
    eltern
        .parent()
        .and_then(|g| g.file_name())
        .is_some_and(|n| n.to_string_lossy() == "target")
}

/// Der Riegel als Fehler, mit dem Weg hinaus.
pub fn riegel_bauverzeichnis() -> Result<(), String> {
    let Some(eigenes) = aus_dem_bauverzeichnis() else {
        return Ok(());
    };

    // Zeilenweise zusammengesetzt und nicht als eine lange Zeichenkette mit
    // Fortsetzungen: Die fressen zwar den Umbruch, aber die Einrueckung des
    // Quelltextes landet im Text — beim ersten Versuch stand die Meldung mit
    // neun Leerzeichen Einzug auf dem Schirm.
    let zeilen = [
        "Dieses Programm läuft aus dem Bauverzeichnis:".to_string(),
        format!("  {}", eigenes.display()),
        String::new(),
        "Ein MCP-Eintrag würde auf diesen Pfad zeigen. Der verschwindet beim".to_string(),
        "nächsten `cargo clean` oder Zweigwechsel — und dann ist AI Brain in".to_string(),
        "deinen Werkzeugen still weg, während der Eintrag richtig aussieht.".to_string(),
        String::new(),
        "Nimm das installierte Programm:".to_string(),
        "  ai-brain-harness einrichten".to_string(),
        String::new(),
        "Zum Prüfen in einem eigenen HOME:".to_string(),
        "  AI_BRAIN_HARNESS_AUS_DEM_BAUM=1 …".to_string(),
    ];

    Err(zeilen.join("\n"))
}

/// Je imkr-Konto: trägt sein `.claude.json` unseren Eintrag? (#8949)
///
/// `None` heisst **keine Auskunft** — kein imkr, oder eines vor v0.21.76, das
/// `account ls --json` noch nicht kennt. Das ist etwas anderes als eine leere
/// Liste („es gibt keine Konten"), und die Zusammenfassung muss es
/// unterscheiden: Sonst behauptet sie „keine Konten", wo sie nur nicht
/// nachgesehen hat. Dasselbe Muster wie bei der Freigabe-Auskunft (#8932).
///
/// **Geprüft wird das `.claude.json` DES KONTOS, nicht unser `~/.claude.json`.**
/// Dass der Eintrag in der einen Wahrheit steht, hat `imkr account sync` noch
/// nicht in die Konten getragen — und genau das will die Zeile beantworten.
///
/// Der Pfad kommt aus imkrs eigener Auskunft (`claude_json`) und nicht aus
/// einem nachgebauten Pfad: Wo imkr seine Konten ablegt, gehört imkr. Beim
/// Standardkonto zeigt er auf `~/.claude.json` — dieselbe Datei, die wir
/// schreiben, und das ist richtig so.
///
/// @return (Kontoname, Eintrag vorhanden) je claude-Konto
pub fn imkr_konten_stand() -> Option<Vec<(String, bool)>> {
    which("imkr")?;

    let ausgabe = std::process::Command::new("imkr")
        .args(["account", "ls", "--json"])
        .output()
        .ok()?;

    if !ausgabe.status.success() {
        return None;
    }

    let liste: Vec<serde_json::Value> =
        serde_json::from_slice(&ausgabe.stdout).ok()?;

    let mut stand = Vec::new();

    for konto in liste {
        // Nur claude: Codex führt kein `.claude.json`, und `claude_json` ist
        // dort `null`.
        if konto.get("tool").and_then(|t| t.as_str()) != Some("claude") {
            continue;
        }

        let name = konto
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("(ohne Namen)")
            .to_string();

        let drin = konto
            .get("claude_json")
            .and_then(|p| p.as_str())
            .is_some_and(datei_hat_eintrag);

        stand.push((name, drin));
    }

    Some(stand)
}

/// Trägt diese `.claude.json` unseren MCP-Eintrag, und zeigt er auf uns?
fn datei_hat_eintrag(pfad: &str) -> bool {
    let Ok(roh) = fs::read_to_string(pfad) else {
        return false;
    };

    let Ok(wert) = serde_json::from_str::<serde_json::Value>(&roh) else {
        return false;
    };

    wert
        .get("mcpServers")
        .and_then(|s| s.get("ai-brain"))
        .and_then(|e| e.get("command"))
        .and_then(|c| c.as_str())
        .is_some_and(ist_unser_programm)
}

/// Liegt dieses Programm im PATH?
fn which(name: &str) -> Option<PathBuf> {
    let pfad = std::env::var_os("PATH")?;

    std::env::split_paths(&pfad)
        .map(|verzeichnis| verzeichnis.join(name))
        .find(|kandidat| kandidat.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Der Riegel aus #8949 — an Pfaden, nicht am laufenden Prozess.
    ///
    /// Geprüft wird die Erkennung, nicht `current_exe`: Die hängt davon ab,
    /// wie der Test selbst gebaut wurde, und ein Test, dessen Ergebnis vom
    /// Bauort abhängt, prüft den Bauort.
    #[test]
    fn das_bauverzeichnis_wird_an_target_erkannt() {
        let faelle: [(&str, bool); 7] = [
            ("/pfad/target/debug/ai-brain-harness", true),
            ("/pfad/target/release/ai-brain-harness", true),
            // Abweichendes CARGO_TARGET_DIR: zählt das Elternpaar, nicht der
            // Anfang des Pfades.
            ("/woanders/ganz/target/debug/ai-brain-harness", true),
            ("/home/x/.local/bin/ai-brain-harness", false),
            ("/usr/local/bin/ai-brain-harness", false),
            // „release" allein genügt NICHT — sonst wäre jedes Verzeichnis
            // mit diesem Namen betroffen.
            ("/opt/release/ai-brain-harness", false),
            ("/opt/debug/ai-brain-harness", false),
        ];

        for (pfad, erwartet) in faelle {
            assert_eq!(ist_bauverzeichnis(std::path::Path::new(pfad)), erwartet, "{pfad}");
        }
    }

    /// Bewusst EIN Test statt mehrerer: Umgebungsvariablen gehören dem ganzen
    /// Prozess, und Rust lässt Tests nebenläufig laufen. Getrennte Tests würden
    /// sich gegenseitig die Variablen unter den Füßen wegziehen und je nach
    /// Laune bestehen oder scheitern.
    #[test]
    fn pfade_finden_auch_ohne_home() {
        let sicherung: Vec<(&str, Option<std::ffi::OsString>)> =
            ["HOME", "USERPROFILE", "XDG_CONFIG_HOME", "APPDATA"]
                .iter()
                .map(|k| (*k, std::env::var_os(k)))
                .collect();

        for (k, _) in &sicherung {
            std::env::remove_var(k);
        }

        // Ohne jede Angabe darf nicht geraten werden.
        assert!(
            heimat().is_err(),
            "ohne HOME und USERPROFILE darf es kein Ergebnis geben"
        );

        // Der Windows-Fall: nur USERPROFILE gesetzt. Genau hier brach das
        // Programm vorher ab, bevor es überhaupt etwas tun konnte.
        std::env::set_var("USERPROFILE", "/tmp/nutzer");
        assert_eq!(heimat().unwrap(), PathBuf::from("/tmp/nutzer"));

        // HOME hat Vorrang, damit sich unter Unix nichts ändert.
        std::env::set_var("HOME", "/tmp/zuhause");
        assert_eq!(heimat().unwrap(), PathBuf::from("/tmp/zuhause"));

        // XDG schlägt alles — das galt vorher schon und muss weiter gelten.
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg");
        assert_eq!(
            config_dir().unwrap(),
            PathBuf::from("/tmp/xdg/ai-brain-cli")
        );

        std::env::remove_var("XDG_CONFIG_HOME");
        assert_eq!(
            config_dir().unwrap(),
            PathBuf::from("/tmp/zuhause/.config/ai-brain-cli")
        );

        // Die Übernahme vom alten Namen (#7664). Steht in DIESEM Test, nicht in
        // einem eigenen — aus demselben Grund wie alles andere hier: der Umzug
        // haengt an XDG_CONFIG_HOME, und zwei Tests wuerden sich die Variable
        // gegenseitig wegziehen.
        let spielwiese =
            std::env::temp_dir().join(format!("ai-brain-cli-uebernahme-{}", std::process::id()));
        let _ = fs::remove_dir_all(&spielwiese);
        std::env::set_var("XDG_CONFIG_HOME", &spielwiese);

        let alt = spielwiese.join("ai-brain-companion");
        let neu = spielwiese.join("ai-brain-cli");

        let alt_anlegen = || {
            fs::create_dir_all(&alt).unwrap();
            fs::write(alt.join("connection.json"), "{}").unwrap();
            // Der Abgleichstand MUSS mitkommen — bleibt er zurueck, meldet
            // `sync` danach jeden Eintrag als geaendert.
            fs::write(alt.join("sync-state.json"), "ABGLEICH").unwrap();
        };

        // Fall 1: Der Rechner hat nur die alte Ablage.
        alt_anlegen();
        uebernimm_alte_ablage();

        assert!(!alt.exists(), "der leere alte Ordner muss verschwinden");
        assert_eq!(
            fs::read_to_string(neu.join("connection.json")).unwrap(),
            "{}"
        );
        assert_eq!(
            fs::read_to_string(neu.join("sync-state.json")).unwrap(),
            "ABGLEICH",
            "der Abgleichstand muss mit umziehen"
        );

        // Fall 2 — der Fall, der fast durchgerutscht waere: `install.sh` legt
        // die neue Ablage selbst an und schreibt `instanz` hinein, BEVOR dieses
        // Programm das erste Mal laeuft. Eine Pruefung auf „Ordner existiert
        // nicht" wuerde hier schweigen und die Zugangsdaten zurueck lassen.
        let _ = fs::remove_dir_all(&neu);
        fs::create_dir_all(&neu).unwrap();
        fs::write(neu.join("instanz"), "https://neue-instanz.example").unwrap();
        alt_anlegen();
        fs::write(alt.join("instanz"), "https://alte-instanz.example").unwrap();

        uebernimm_alte_ablage();

        assert_eq!(
            fs::read_to_string(neu.join("connection.json")).unwrap(),
            "{}",
            "die Verbindung muss trotz vorhandenem Ordner herueberkommen"
        );
        assert_eq!(
            fs::read_to_string(neu.join("instanz")).unwrap(),
            "https://neue-instanz.example",
            "die vom Installer geschriebene Instanz ist die aktuellere und bleibt"
        );
        assert!(
            alt.exists(),
            "blieb etwas stehen, bleibt auch der alte Ordner — kein Loeschweg"
        );

        // Fall 3: Ist die Verbindung schon drueben, wird nichts mehr angefasst.
        let _ = fs::remove_dir_all(&alt);
        alt_anlegen();
        fs::write(alt.join("connection.json"), "FREMD").unwrap();
        uebernimm_alte_ablage();
        assert_eq!(
            fs::read_to_string(neu.join("connection.json")).unwrap(),
            "{}",
            "eine vorhandene Verbindung darf nicht ueberschrieben werden"
        );

        let _ = fs::remove_dir_all(&spielwiese);

        for (k, wert) in sicherung {
            match wert {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}

#[cfg(test)]
mod umbenennung_tests {
    use super::*;

    fn server(json: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        json.as_object().unwrap().clone()
    }

    #[test]
    fn erkennt_alle_eigenen_namen_auch_mit_pfad_und_endung() {
        assert!(ist_unser_programm("/home/x/.local/bin/ai-brain-harness"));
        assert!(ist_unser_programm("/usr/local/bin/ai-brain-cli"));
        assert!(ist_unser_programm("~/.local/bin/ai-brain-companion"));
        // Windows: mit Endung.
        assert!(ist_unser_programm("C:\\Programs\\ai-brain-cli.exe"));
        // Ohne Pfad.
        assert!(ist_unser_programm("ai-brain-harness"));
    }

    #[test]
    fn haelt_fremde_programme_fuer_fremd() {
        assert!(!ist_unser_programm("/usr/bin/npx"));
        assert!(!ist_unser_programm("/home/x/.hermes/hermes-agent"));
        // Scharf: ein Name, der unseren ENTHAELT, ist noch nicht unserer.
        assert!(!ist_unser_programm("/usr/bin/ai-brain-cli-wrapper"));
    }

    #[test]
    fn raeumt_eintraege_frueherer_fassungen_weg() {
        let mut s = server(serde_json::json!({
            "ai-brain-companion": { "command": "/home/x/.local/bin/ai-brain-companion", "args": ["mcp"] },
            "ai-brain-cli": { "command": "/usr/local/bin/ai-brain-cli", "args": ["mcp"] },
        }));

        let weg = alte_eintraege_raeumen(&mut s);

        assert_eq!(weg.len(), 2);
        assert!(s.is_empty());
    }

    #[test]
    fn laesst_den_eigenen_schluessel_stehen() {
        // Der wird beim Schreiben ohnehin ersetzt — ihn hier zu loeschen waere
        // doppelt und wuerde faelschlich als „aufgeraeumt" gemeldet.
        let mut s = server(serde_json::json!({
            "ai-brain": { "command": "/alt/ai-brain-cli", "args": ["mcp"] },
        }));

        assert!(alte_eintraege_raeumen(&mut s).is_empty());
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn laesst_fremde_server_unangetastet() {
        let mut s = server(serde_json::json!({
            "hermes": { "command": "/home/x/.hermes/hermes-agent", "args": ["mcp"] },
            "playwright": { "command": "npx", "args": ["-y", "@playwright/mcp"] },
        }));

        assert!(alte_eintraege_raeumen(&mut s).is_empty());
        assert_eq!(s.len(), 2, "fremde Server gehoeren jemand anderem");
    }

    #[test]
    fn uebersteht_einen_eintrag_ohne_befehl() {
        // Eine Konfiguration von Hand kann alles enthalten. Ein Eintrag ohne
        // `command` darf nicht zum Absturz fuehren — und auch nicht geloescht
        // werden, denn wir wissen nicht, wem er gehoert.
        let mut s = server(serde_json::json!({
            "kaputt": { "args": ["mcp"] },
            "auch-kaputt": "gar kein Objekt",
        }));

        assert!(alte_eintraege_raeumen(&mut s).is_empty());
        assert_eq!(s.len(), 2);
    }
}

/// Die Pruefungen um {@link Eintragsstand} — ohne echte Werkzeuge (#1123).
///
/// Bewusst an den reinen Funktionen und einer Attrappen-Datei: Was geprueft
/// werden muss, ist die ENTSCHEIDUNG („schon drin" gegen „angelegt"), nicht
/// dass `codex` oder `pi` laufen. Ein Test, der die echten Werkzeuge aufruft,
/// schreibt in die Konfiguration des Rechners, auf dem er laeuft — genau der
/// Fehler aus dem Wiki-Artikel „Die Isolation fallen lassen, um besser zu
/// messen".
#[cfg(test)]
mod eintraege {
    use super::*;

    #[test]
    fn nur_pfad_und_argumente_zusammen_zeigen_auf_uns() {
        let unser = "/home/x/.local/bin/ai-brain-harness";

        let richtig = serde_json::json!({"command": unser, "args": ["mcp"]});
        assert!(befehl_zeigt_auf(&richtig, unser));

        // Richtiges Programm, aber ohne `mcp`: startet keinen Server und
        // sieht in der Datei trotzdem richtig aus.
        let ohne = serde_json::json!({"command": unser, "args": []});
        assert!(!befehl_zeigt_auf(&ohne, unser));

        // Eine frueher installierte Fassung an einem anderen Ort gehoert uns
        // und ist trotzdem falsch — deshalb der ganze Pfad, nicht der Name.
        let woanders =
            serde_json::json!({"command": "/usr/local/bin/ai-brain-cli", "args": ["mcp"]});
        assert!(!befehl_zeigt_auf(&woanders, unser));

        let fremd = serde_json::json!({"command": "npx", "args": ["mcp"]});
        assert!(!befehl_zeigt_auf(&fremd, unser));
    }

    #[test]
    fn aus_vorher_und_nachher_wird_der_stand() {
        use Eintragsstand::*;

        // Es gab keinen → angelegt.
        assert_eq!(nachgelesen(None, Some(true)), Angelegt);
        // Es gab einen, der woanders hinzeigte → erneuert.
        assert_eq!(nachgelesen(Some(false), Some(true)), Erneuert);
        // Geschrieben und nicht wiedergefunden ist kein Erfolg. Das ist der
        // Kern von #1123: lieber „bitte nachsehen" als „angelegt".
        assert_eq!(nachgelesen(None, None), Verschwunden);
        assert_eq!(nachgelesen(Some(false), Some(false)), Verschwunden);
    }

    /// `None` und `Some(false)` sind zwei verschiedene Auskuenfte: „es gibt
    /// keinen" gegen „es gibt einen, der woanders hinzeigt". Ohne diesen
    /// Unterschied gaebe es kein „erneuert".
    #[test]
    fn eine_mcp_json_wird_dreifach_unterschieden() {
        let unser = "/home/x/.local/bin/ai-brain-harness";
        let ordner = std::env::temp_dir().join(format!(
            "ai-brain-harness-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));

        fs::create_dir_all(&ordner).unwrap();

        let datei = ordner.join("mcp.json");

        // Gar keine Datei.
        assert_eq!(mcp_json_zeigt_auf(&datei, unser), None);

        // Datei ohne unseren Eintrag.
        fs::write(&datei, r#"{"mcpServers":{"playwright":{"command":"npx"}}}"#).unwrap();
        assert_eq!(mcp_json_zeigt_auf(&datei, unser), None);

        // Unser Eintrag, aber auf eine andere Fassung.
        fs::write(
            &datei,
            r#"{"mcpServers":{"ai-brain":{"command":"/alt/ai-brain-cli","args":["mcp"]}}}"#,
        )
        .unwrap();
        assert_eq!(mcp_json_zeigt_auf(&datei, unser), Some(false));

        // Genau so, wie wir ihn schreiben — das ist die Form, die `pi` am
        // 27.10.2026 in einem Schein-HOME angelegt hat.
        fs::write(
            &datei,
            format!(r#"{{"mcpServers":{{"ai-brain":{{"command":"{unser}","args":["mcp"]}}}}}}"#),
        )
        .unwrap();
        assert_eq!(mcp_json_zeigt_auf(&datei, unser), Some(true));

        let _ = fs::remove_dir_all(&ordner);
    }
}
