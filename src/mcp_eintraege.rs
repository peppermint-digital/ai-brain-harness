//! MCP-Server aus dem Repository — eingetragen in Claude Code und Codex.
//!
//! Ein Repository-Eintrag vom Typ `mcp` ist kein Dateibestand, sondern eine
//! Server-Definition: Befehl, Argumente, Umgebung. `sync` schreibt sie als
//! `mcpServers.<slug>` in jede Claude-Konfiguration des Rechners und über
//! `codex mcp add` in Codex.
//!
//! ## Warum strenger als Skills
//!
//! Ein MCP-Server ist — wie ein Hook — ein Befehl, der von selbst läuft:
//! `npx -y @playwright/mcp@latest` lädt Code aus npm und führt ihn aus, sobald
//! Claude Code startet. Deshalb gilt:
//!
//! * Eingetragen wird nur, was auf diesem Rechner **ausdrücklich** gewählt ist
//!   (`select`, Liste `slugs`). Der Modus „alles" nimmt MCP-Server nie mit —
//!   das prüft die Instanz beim Manifest, und hier wird es noch einmal geprüft.
//! * Ein gleichnamiger Server, den der Harness nicht selbst geschrieben hat,
//!   wird nicht still ersetzt — wie ein fremder Skill.
//! * `--aufraeumen` entfernt nur, was der Harness selbst geschrieben hat und
//!   was seither unverändert ist.
//!
//! ## Wo eingetragen wird
//!
//! Claude Code hat auf einem Rechner oft mehrere Konfigurationen: die im
//! Heimatordner, die unter `$CLAUDE_CONFIG_DIR` und je eine pro imkr-Konto
//! (`~/.local/state/imkr/accounts/claude/<konto>/.claude.json`). Ein Server,
//! der nur in einer davon steht, fehlt in allen Sitzungen der anderen — genau
//! das war bei `configure` der Fall, das nur `~/.claude.json` kannte.
//!
//! ## Wie Änderungen erkannt werden
//!
//! Wie bei Dateien: Je Konfigurationsdatei merkt sich der Harness einen
//! eigenen Hash über den Eintrag, so wie er ihn geschrieben hat. Verglichen
//! wird immer nur mit diesem eigenen Wert, nie mit der Server-Prüfsumme.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

/// Schlüssel im Abgleichstand für den Codex-Eintrag — neben den Pfaden der
/// Claude-Konfigurationen.
pub const CODEX: &str = "codex";

/// Was die Instanz als Inhalt eines `mcp`-Eintrags liefert.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Definition {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Vorgabe: beide. Ein Eintrag, der nur für Claude gedacht ist, sagt das.
    #[serde(default = "alle_ziele")]
    pub targets: Vec<String>,
    /// Steht hier z. B. `--executable-path`, wird der lokale Browser gesucht
    /// und mit diesem Schalter angehängt. Ohne Browser fällt der Schalter weg.
    #[serde(default)]
    pub browser_flag: Option<String>,
}

fn alle_ziele() -> Vec<String> {
    vec!["claude".to_string(), "codex".to_string()]
}

impl Definition {
    pub fn fuer_claude(&self) -> bool {
        self.targets.iter().any(|z| z == "claude")
    }

    pub fn fuer_codex(&self) -> bool {
        self.targets.iter().any(|z| z == "codex")
    }

    /// Die Argumente, wie sie auf DIESEM Rechner gelten.
    pub fn argumente(&self, browser: Option<&Path>) -> Vec<String> {
        let mut args = self.args.clone();

        if let (Some(schalter), Some(pfad)) = (&self.browser_flag, browser) {
            if !schalter.trim().is_empty() {
                args.push(schalter.clone());
                args.push(pfad.to_string_lossy().to_string());
            }
        }

        args
    }

    /// Der Eintrag für `mcpServers.<slug>` in einer Claude-Konfiguration.
    pub fn claude_eintrag(&self, browser: Option<&Path>) -> serde_json::Value {
        serde_json::json!({
            "type": "stdio",
            "command": self.command,
            "args": self.argumente(browser),
            "env": self.env,
        })
    }

    /// Dieselbe Angabe in der Form, in der `codex mcp get --json` sie liefert
    /// — nur die Teile, die der Harness selbst setzt.
    pub fn codex_eintrag(&self, browser: Option<&Path>) -> serde_json::Value {
        serde_json::json!({
            "command": self.command,
            "args": self.argumente(browser),
            "env": self.env,
        })
    }
}

/// Liest den Inhalt eines `mcp`-Eintrags.
///
/// Die Instanz liefert im Bündel den Schlüssel `mcp`: die schon geprüfte und
/// mit Vorgaben aufgefüllte Definition. Die gilt — die Regeln der Instanz
/// werden hier nicht nachgebaut. Nur wenn er fehlt (ältere Instanz), wird die
/// Datei `mcp.json` aus `files` gelesen. Fehlt `command`, ist das kein Server,
/// und es wird nichts eingetragen.
pub fn definition_lesen(bundle: &serde_json::Value) -> Result<Definition, String> {
    let roh = inhalt_finden(bundle)
        .ok_or_else(|| "der Eintrag enthält keine Server-Definition".to_string())?;

    let wert: serde_json::Value = match roh {
        serde_json::Value::String(text) => serde_json::from_str(text.trim())
            .map_err(|e| format!("die Server-Definition ist kein gültiges JSON: {e}"))?,
        anderes => anderes.clone(),
    };

    let definition: Definition = serde_json::from_value(wert)
        .map_err(|e| format!("die Server-Definition ist unvollständig: {e}"))?;

    if definition.command.trim().is_empty() {
        return Err("die Server-Definition nennt keinen Befehl".to_string());
    }

    Ok(definition)
}

fn inhalt_finden(bundle: &serde_json::Value) -> Option<&serde_json::Value> {
    for feld in ["mcp", "definition"] {
        if let Some(wert) = bundle.get(feld) {
            if !wert.is_null() {
                return Some(wert);
            }
        }
    }

    let dateien = bundle.get("files")?.as_array()?;

    dateien
        .iter()
        .find(|d| d.get("path").and_then(|p| p.as_str()) == Some("mcp.json"))
        .or_else(|| {
            dateien.iter().find(|d| {
                d.get("path")
                    .and_then(|p| p.as_str())
                    .map(|p| p.ends_with(".json"))
                    .unwrap_or(false)
            })
        })
        .or_else(|| {
            if dateien.len() == 1 {
                dateien.first()
            } else {
                None
            }
        })
        .and_then(|d| d.get("content"))
}

// --------------------------------------------------------------- Browser ----

/// Sucht den Browser dieses Rechners.
///
/// Reihenfolge wie im chezmoi-Skript, aus dem diese Funktion stammt: zuerst
/// Chromium, dann Chrome. Auf macOS und Windows liegen die Browser nicht im
/// PATH — dort helfen nur die üblichen Installationsorte.
pub fn browser_finden() -> Option<PathBuf> {
    browser_finden_in(std::env::var_os("PATH"), &feste_browserorte())
}

pub fn browser_finden_in(path: Option<OsString>, feste_orte: &[PathBuf]) -> Option<PathBuf> {
    const NAMEN: &[&str] = &[
        "chromium",
        "chromium-browser",
        "google-chrome-stable",
        "google-chrome",
        "chrome",
    ];

    if let Some(path) = path {
        for verzeichnis in std::env::split_paths(&path) {
            for name in NAMEN {
                let kandidat = verzeichnis.join(name);

                if kandidat.is_file() {
                    return Some(kandidat);
                }

                if cfg!(windows) {
                    let mit_endung = verzeichnis.join(format!("{name}.exe"));

                    if mit_endung.is_file() {
                        return Some(mit_endung);
                    }
                }
            }
        }
    }

    feste_orte.iter().find(|p| p.is_file()).cloned()
}

fn feste_browserorte() -> Vec<PathBuf> {
    let mut orte = vec![
        PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
    ];

    for variable in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
        if let Some(basis) = std::env::var_os(variable) {
            orte.push(
                PathBuf::from(basis)
                    .join("Google")
                    .join("Chrome")
                    .join("Application")
                    .join("chrome.exe"),
            );
        }
    }

    orte
}

// ------------------------------------------------- Claude-Konfigurationen ----

/// Alle Claude-Konfigurationen dieses Rechners.
///
/// Die im Heimatordner gehört immer dazu — auch wenn sie noch nicht existiert,
/// denn `configure` legt sie ebenso an. Die übrigen nur, wenn sie schon da
/// sind: Ein Konto, das es nicht gibt, bekommt keine Konfiguration erfunden.
pub fn claude_konfigurationen(heimat: &Path, config_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut pfade: Vec<PathBuf> = vec![heimat.join(".claude.json")];

    if let Some(dir) = config_dir {
        let pfad = dir.join(".claude.json");

        if pfad.is_file() {
            pfade.push(pfad);
        }
    }

    let konten = heimat
        .join(".local")
        .join("state")
        .join("imkr")
        .join("accounts")
        .join("claude");

    if let Ok(eintraege) = fs::read_dir(&konten) {
        let mut gefunden: Vec<PathBuf> = eintraege
            .flatten()
            .map(|e| e.path().join(".claude.json"))
            .filter(|p| p.is_file())
            .collect();

        gefunden.sort();
        pfade.extend(gefunden);
    }

    // Derselbe Ort darf nicht zweimal vorkommen — $CLAUDE_CONFIG_DIR zeigt in
    // einer imkr-Sitzung genau auf eines der Konten.
    let mut gesehen: BTreeSet<PathBuf> = BTreeSet::new();
    pfade.retain(|p| gesehen.insert(fs::canonicalize(p).unwrap_or_else(|_| p.clone())));

    pfade
}

/// Der Eintrag `mcpServers.<name>` einer Konfiguration, falls vorhanden.
pub fn claude_lesen(pfad: &Path, name: &str) -> Result<Option<serde_json::Value>, String> {
    if !pfad.exists() {
        return Ok(None);
    }

    let wurzel = json_lesen(pfad)?;

    Ok(wurzel.get("mcpServers").and_then(|s| s.get(name)).cloned())
}

/// Setzt oder entfernt genau `mcpServers.<name>` — alles andere bleibt.
///
/// `~/.claude.json` gehört Claude Code und enthält weit mehr als unsere
/// Einträge. Deshalb: einmal pro Lauf eine Sicherung daneben, nur den einen
/// Schlüssel anfassen, über eine Zwischendatei schreiben. Ein halb
/// geschriebenes JSON dort kostet den Nutzer seine ganze Konfiguration.
pub fn claude_schreiben(
    pfad: &Path,
    name: &str,
    eintrag: Option<&serde_json::Value>,
    gesichert: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let mut wurzel = if pfad.exists() {
        if gesichert.insert(pfad.to_path_buf()) {
            let sicherung = sicherungspfad(pfad);
            fs::copy(pfad, &sicherung)
                .map_err(|e| format!("Sicherung {} schlug fehl: {e}", sicherung.display()))?;
        }

        json_lesen(pfad)?
    } else {
        if eintrag.is_none() {
            return Ok(());
        }

        serde_json::json!({})
    };

    let objekt = wurzel
        .as_object_mut()
        .ok_or_else(|| format!("{} ist kein JSON-Objekt.", pfad.display()))?;

    match eintrag {
        Some(wert) => {
            objekt
                .entry("mcpServers")
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
                .ok_or_else(|| format!("mcpServers in {} ist kein JSON-Objekt.", pfad.display()))?
                .insert(name.to_string(), wert.clone());
        }
        None => {
            if let Some(server) = objekt.get_mut("mcpServers").and_then(|s| s.as_object_mut()) {
                server.remove(name);
            }
        }
    }

    let json = serde_json::to_string_pretty(&wurzel)
        .map_err(|e| format!("Konfiguration lässt sich nicht schreiben: {e}"))?;

    atomar_schreiben(pfad, json.as_bytes())
}

pub fn sicherungspfad(pfad: &Path) -> PathBuf {
    let mut name = pfad
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| OsString::from(".claude.json"));
    name.push(".vor-harness-sync");

    pfad.with_file_name(name)
}

fn json_lesen(pfad: &Path) -> Result<serde_json::Value, String> {
    let roh = fs::read_to_string(pfad)
        .map_err(|e| format!("{} lässt sich nicht lesen: {e}", pfad.display()))?;

    serde_json::from_str(&roh)
        .map_err(|e| format!("{} enthält kein gültiges JSON: {e}", pfad.display()))
}

fn atomar_schreiben(pfad: &Path, inhalt: &[u8]) -> Result<(), String> {
    let mut name = pfad
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(format!(".harness-{}", std::process::id()));
    let zwischen = pfad.with_file_name(name);

    if let Some(eltern) = pfad.parent() {
        fs::create_dir_all(eltern)
            .map_err(|e| format!("{} lässt sich nicht anlegen: {e}", eltern.display()))?;
    }

    fs::write(&zwischen, inhalt)
        .map_err(|e| format!("{} lässt sich nicht schreiben: {e}", zwischen.display()))?;

    // Rechte der alten Datei übernehmen — `.claude.json` kann Zugangsdaten
    // enthalten und ist oft nur für den Besitzer lesbar.
    if let Ok(alt) = fs::metadata(pfad) {
        let _ = fs::set_permissions(&zwischen, alt.permissions());
    }

    fs::rename(&zwischen, pfad).map_err(|e| {
        let _ = fs::remove_file(&zwischen);
        format!("{} lässt sich nicht ersetzen: {e}", pfad.display())
    })
}

// ------------------------------------------------------------------ Codex ----

pub fn codex_vorhanden() -> bool {
    let Some(pfad) = std::env::var_os("PATH") else {
        return false;
    };

    std::env::split_paths(&pfad).any(|v| v.join("codex").is_file() || v.join("codex.exe").is_file())
}

/// Der Codex-Eintrag in der Form von {@link Definition::codex_eintrag}.
///
/// `None`, wenn es keinen gibt. Ein Fehler nur, wenn Codex sich nicht
/// aufrufen lässt — ein unbekannter Name ist der Normalfall.
pub fn codex_lesen(name: &str) -> Result<Option<serde_json::Value>, String> {
    let ausgabe = std::process::Command::new("codex")
        .args(["mcp", "get", name, "--json"])
        .output()
        .map_err(|e| format!("codex liess sich nicht aufrufen: {e}"))?;

    if !ausgabe.status.success() {
        return Ok(None);
    }

    let wert = json_ab_erster_klammer(&ausgabe.stdout)
        .map_err(|e| format!("codex mcp get {name} lieferte kein JSON: {e}"))?;

    Ok(Some(codex_normalisieren(&wert)))
}

/// Liest das JSON ab der ersten Zeile, die mit `{` oder `[` beginnt.
///
/// `codex` ist auf Omarchy-Rechnern ein Wrapper, der vorher `mise use -g codex`
/// aufruft — und mise schreibt dabei eine Zeile wie
/// `mise ~/.config/mise/config.toml tools: codex@0.160.1` auf die
/// Standardausgabe, VOR das JSON. Am 15.10.2026 brach `sync` daran auf dem
/// Laptop ab. Was davor steht, ist nie Teil der Antwort.
///
/// **Auch eine Liste** (#1127): `codex mcp get --json` antwortet mit einem
/// Objekt, `codex mcp list --json` mit einem Array. Dieser Helfer kannte nur
/// `{`, und die Nachlese aus #1123 parste deshalb an derselben mise-Zeile
/// vorbei — gemeldet wurde dann „geschrieben, aber nicht wiedergefunden",
/// obwohl der Eintrag korrekt dastand. Eine Fassung, die nur die halbe Form
/// kennt, liest sich wie eine, die den Fall abdeckt.
pub(crate) fn json_ab_erster_klammer(
    stdout: &[u8],
) -> Result<serde_json::Value, serde_json::Error> {
    let text = String::from_utf8_lossy(stdout);
    let mut start = 0;

    for zeile in text.split_inclusive('\n') {
        let anfang = zeile.trim_start();

        if anfang.starts_with('{') || anfang.starts_with('[') {
            break;
        }

        start += zeile.len();
    }

    serde_json::from_str(&text[start..])
}

/// Bringt die Ausgabe von `codex mcp get --json` auf die Teile, die der
/// Harness setzt. `env: null` und `env: {}` sind dasselbe.
pub fn codex_normalisieren(wert: &serde_json::Value) -> serde_json::Value {
    let transport = wert.get("transport").unwrap_or(wert);

    let env = match transport.get("env") {
        Some(serde_json::Value::Object(o)) => serde_json::Value::Object(o.clone()),
        _ => serde_json::json!({}),
    };

    serde_json::json!({
        "command": transport.get("command").cloned().unwrap_or(serde_json::Value::Null),
        "args": transport.get("args").cloned().unwrap_or_else(|| serde_json::json!([])),
        "env": env,
    })
}

pub fn codex_schreiben(
    name: &str,
    definition: &Definition,
    browser: Option<&Path>,
) -> Result<(), String> {
    // Ersetzen heisst bei Codex: erst weg, dann neu. Aufgerufen wird das nur,
    // wenn die Entscheidung zum Überschreiben schon gefallen ist.
    let _ = std::process::Command::new("codex")
        .args(["mcp", "remove", name])
        .output();

    let mut befehl = std::process::Command::new("codex");
    befehl.args(["mcp", "add", name]);

    for (k, v) in &definition.env {
        befehl.arg("--env").arg(format!("{k}={v}"));
    }

    befehl.arg("--").arg(&definition.command);
    befehl.args(definition.argumente(browser));

    let ergebnis = befehl
        .output()
        .map_err(|e| format!("codex liess sich nicht aufrufen: {e}"))?;

    if !ergebnis.status.success() {
        return Err(format!(
            "codex mcp add {name} schlug fehl: {}",
            String::from_utf8_lossy(&ergebnis.stderr).trim()
        ));
    }

    Ok(())
}

pub fn codex_entfernen(name: &str) -> Result<(), String> {
    let ergebnis = std::process::Command::new("codex")
        .args(["mcp", "remove", name])
        .output()
        .map_err(|e| format!("codex liess sich nicht aufrufen: {e}"))?;

    if !ergebnis.status.success() {
        return Err(format!(
            "codex mcp remove {name} schlug fehl: {}",
            String::from_utf8_lossy(&ergebnis.stderr).trim()
        ));
    }

    Ok(())
}

// ------------------------------------------------------------------- Hash ----

/// Eigener Hash über einen Eintrag.
///
/// `serde_json` ordnet Objektschlüssel (ohne `preserve_order`) — dieselbe
/// Angabe ergibt also immer denselben Text, egal in welcher Reihenfolge sie in
/// der Datei stand.
pub fn eintrag_hash(wert: &serde_json::Value) -> String {
    let text = serde_json::to_string(wert).unwrap_or_default();
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

// -------------------------------------------------------------- Urteilen ----

/// Wie es um einen Server an EINEM Ort steht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lage {
    /// Steht dort noch nicht.
    Neu,
    /// Instanz hat eine neuere Fassung, hier wurde nichts angefasst.
    Aktualisiert,
    /// Hier geändert, Instanz unverändert — bleibt.
    LokalGeaendert,
    /// Beide Seiten haben sich bewegt.
    Konflikt,
    /// Steht dort, stammt nicht vom Harness und weicht ab.
    Fremd,
    /// Steht dort, stammt nicht vom Harness — ist aber genau das Gewünschte.
    Uebernommen,
    /// Gleich.
    Unveraendert,
}

/// Urteil für einen Ort.
///
/// * `vorhanden` — was dort jetzt steht
/// * `gewuenscht` — was der Harness heute schreiben würde
/// * `bekannt` — eigener Hash vom letzten Schreiben, falls es eines gab
/// * `fern_gleich` — hat sich die Instanz seit dem letzten Schreiben NICHT bewegt?
pub fn urteilen(
    vorhanden: Option<&serde_json::Value>,
    gewuenscht: &serde_json::Value,
    bekannt: Option<&str>,
    fern_gleich: bool,
) -> Lage {
    let Some(vorhanden) = vorhanden else {
        return Lage::Neu;
    };

    let genau_so = vorhanden == gewuenscht;

    match bekannt {
        None if genau_so => Lage::Uebernommen,
        None => Lage::Fremd,
        Some(hash) => {
            let lokal_gleich = eintrag_hash(vorhanden) == hash;

            match (fern_gleich, lokal_gleich) {
                (true, true) => Lage::Unveraendert,
                // Auch ohne Bewegung der Instanz kann sich das Gewünschte hier
                // geändert haben — etwa weil der Browser erst jetzt installiert
                // ist. Dann wie eine neue Fassung behandeln.
                (true, false) if genau_so => Lage::Uebernommen,
                (true, false) => Lage::LokalGeaendert,
                // Instanz bewegt, Ergebnis aber dasselbe: nur den Stand
                // nachziehen, sonst vergleicht jeder Lauf erneut.
                (false, true) if genau_so => Lage::Uebernommen,
                (false, true) => Lage::Aktualisiert,
                (false, false) if genau_so => Lage::Uebernommen,
                (false, false) => Lage::Konflikt,
            }
        }
    }
}

/// Darf ein nicht mehr gewählter Eintrag an diesem Ort weg?
///
/// Nur wenn er noch genau so dasteht, wie der Harness ihn geschrieben hat.
pub fn entfernbar(vorhanden: Option<&serde_json::Value>, bekannt: &str) -> bool {
    vorhanden
        .map(|v| eintrag_hash(v) == bekannt)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    #[test]
    fn codex_json_hinter_mise_zeile() {
        let stdout =
            b"mise ~/.config/mise/config.toml tools: codex@0.160.1\n{\n  \"name\": \"x\"\n}\n";
        let wert = json_ab_erster_klammer(stdout).unwrap();
        assert_eq!(wert["name"], "x");
    }

    #[test]
    fn codex_json_ohne_vorspann() {
        let wert = json_ab_erster_klammer(b"{\"name\": \"y\"}").unwrap();
        assert_eq!(wert["name"], "y");
    }

    /// #1127: `codex mcp list --json` antwortet mit einer LISTE. Dass der
    /// Helfer nur `{` kannte, hat auf Bastians Laptop „geschrieben, aber nicht
    /// wiedergefunden" gemeldet, obwohl der Eintrag korrekt dastand.
    #[test]
    fn codex_liste_hinter_mise_zeile() {
        let stdout = b"mise ~/.config/mise/config.toml tools: codex@0.160.1\n[\n  {\"name\": \"ai-brain\"}\n]\n";
        let wert = json_ab_erster_klammer(stdout).unwrap();

        assert_eq!(wert[0]["name"], "ai-brain");
    }

    #[test]
    fn codex_liste_ohne_vorspann() {
        let wert = json_ab_erster_klammer(b"[{\"name\": \"z\"}]").unwrap();

        assert_eq!(wert[0]["name"], "z");
    }

    use super::*;

    fn spielwiese(name: &str) -> PathBuf {
        let pfad = std::env::temp_dir().join(format!(
            "harness-mcp-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&pfad);
        fs::create_dir_all(&pfad).unwrap();
        pfad
    }

    fn playwright() -> Definition {
        Definition {
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@playwright/mcp@latest".to_string()],
            env: BTreeMap::new(),
            targets: alle_ziele(),
            browser_flag: Some("--executable-path".to_string()),
        }
    }

    #[test]
    fn haengt_den_browser_nur_an_wenn_es_einen_gibt() {
        let d = playwright();

        assert_eq!(
            d.argumente(Some(Path::new("/usr/bin/chromium"))),
            vec![
                "-y",
                "@playwright/mcp@latest",
                "--executable-path",
                "/usr/bin/chromium"
            ]
        );
        assert_eq!(d.argumente(None), vec!["-y", "@playwright/mcp@latest"]);

        // Ohne Schalter in der Definition bleibt der Browser draussen, auch
        // wenn einer da ist.
        let ohne = Definition {
            browser_flag: None,
            ..playwright()
        };
        assert_eq!(
            ohne.argumente(Some(Path::new("/usr/bin/chromium"))),
            vec!["-y", "@playwright/mcp@latest"]
        );
    }

    #[test]
    fn findet_den_browser_im_path_und_an_festen_orten() {
        let wiese = spielwiese("browser");
        let bin = wiese.join("bin");
        fs::create_dir_all(&bin).unwrap();

        // Leerer PATH, kein fester Ort: nichts.
        assert_eq!(
            browser_finden_in(Some(bin.clone().into_os_string()), &[]),
            None
        );

        // Chrome an einem festen Ort.
        let mac = wiese.join("Google Chrome");
        fs::write(&mac, "").unwrap();
        assert_eq!(
            browser_finden_in(
                Some(bin.clone().into_os_string()),
                std::slice::from_ref(&mac)
            ),
            Some(mac.clone())
        );

        // Chromium im PATH schlägt den festen Ort.
        fs::write(bin.join("chromium"), "").unwrap();
        assert_eq!(
            browser_finden_in(Some(bin.clone().into_os_string()), &[mac]),
            Some(bin.join("chromium"))
        );

        let _ = fs::remove_dir_all(&wiese);
    }

    #[test]
    fn liest_die_definition_aus_jeder_verpackung() {
        // So liefert die Instanz: `mcp` geprüft und aufgefüllt, daneben die Datei.
        let bundle = serde_json::json!({
            "checksum": "x",
            "files": [{ "path": "mcp.json", "content": "{\"command\":\"FALSCH\"}", "encoding": "utf-8" }],
            "mcp": {
                "command": "npx",
                "args": ["-y", "chrome-devtools-mcp@latest"],
                "env": {},
                "targets": ["claude", "codex"],
                "browser_flag": "--executablePath",
            },
        });
        let d = definition_lesen(&bundle).unwrap();
        assert_eq!(
            d.command, "npx",
            "der Schlüssel `mcp` hat Vorrang vor der Datei"
        );
        assert_eq!(d.browser_flag.as_deref(), Some("--executablePath"));

        // browser_flag: null von der Instanz heisst: kein Browser.
        let ohne = serde_json::json!({ "mcp": { "command": "npx", "args": [], "env": {}, "targets": ["claude"], "browser_flag": null } });
        assert_eq!(definition_lesen(&ohne).unwrap().browser_flag, None);

        // Ältere Instanz ohne `mcp`: die Datei, Vorgaben hier.
        let nur_datei = serde_json::json!({
            "files": [{ "path": "mcp.json", "content": "{\"command\":\"npx\"}" }],
        });
        let d = definition_lesen(&nur_datei).unwrap();
        assert_eq!(d.targets, alle_ziele(), "ohne Angabe gilt beides");

        let als_datei = serde_json::json!({
            "checksum": "x",
            "files": [{ "path": "server.json", "content": "{\"command\":\"npx\",\"targets\":[\"claude\"]}" }],
        });
        let d = definition_lesen(&als_datei).unwrap();
        assert!(d.fuer_claude());
        assert!(!d.fuer_codex());

        // Ohne Befehl kein Server.
        assert!(definition_lesen(&serde_json::json!({ "mcp": { "args": [] } })).is_err());
        assert!(definition_lesen(
            &serde_json::json!({ "files": [{ "path": "mcp.json", "content": "kein json" }] })
        )
        .is_err());
        assert!(definition_lesen(&serde_json::json!({ "files": [] })).is_err());
    }

    #[test]
    fn findet_alle_claude_konfigurationen() {
        let heimat = spielwiese("konfigurationen");
        let konten = heimat.join(".local/state/imkr/accounts/claude");

        for konto in ["henneberg", "department", "leer"] {
            fs::create_dir_all(konten.join(konto)).unwrap();
        }
        fs::write(konten.join("henneberg/.claude.json"), "{}").unwrap();
        fs::write(konten.join("department/.claude.json"), "{}").unwrap();

        // $CLAUDE_CONFIG_DIR zeigt auf ein Konto — darf nicht doppelt zählen.
        let pfade = claude_konfigurationen(&heimat, Some(&konten.join("henneberg")));

        // Heimat zuerst (auch wenn sie noch fehlt), dann $CLAUDE_CONFIG_DIR,
        // dann die übrigen Konten. Das leere Konto bekommt nichts erfunden.
        assert_eq!(
            pfade,
            vec![
                heimat.join(".claude.json"),
                konten.join("henneberg/.claude.json"),
                konten.join("department/.claude.json"),
            ]
        );

        let _ = fs::remove_dir_all(&heimat);
    }

    #[test]
    fn schreibt_in_mehrere_konfigurationen_ohne_fremdes_zu_veraendern() {
        let wiese = spielwiese("schreiben");
        let a = wiese.join("a.json");
        let b = wiese.join("b.json");

        let fremd_a = serde_json::json!({
            "numStartups": 42,
            "projects": { "/x": { "allowedTools": ["Bash"] } },
            "mcpServers": { "hermes": { "command": "/opt/hermes", "args": ["mcp"] } },
        });
        fs::write(&a, serde_json::to_string_pretty(&fremd_a).unwrap()).unwrap();
        fs::write(&b, "{\"oauthAccount\":{\"emailAddress\":\"x@y\"}}").unwrap();

        let eintrag = playwright().claude_eintrag(Some(Path::new("/usr/bin/chromium")));
        let mut gesichert = BTreeSet::new();

        for pfad in [&a, &b] {
            claude_schreiben(pfad, "playwright", Some(&eintrag), &mut gesichert).unwrap();
        }

        let nach_a: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&a).unwrap()).unwrap();
        assert_eq!(nach_a["numStartups"], 42);
        assert_eq!(nach_a["projects"], fremd_a["projects"]);
        assert_eq!(
            nach_a["mcpServers"]["hermes"],
            fremd_a["mcpServers"]["hermes"]
        );
        assert_eq!(nach_a["mcpServers"]["playwright"], eintrag);

        let nach_b: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&b).unwrap()).unwrap();
        assert_eq!(nach_b["oauthAccount"]["emailAddress"], "x@y");
        assert_eq!(nach_b["mcpServers"]["playwright"], eintrag);

        // Vorher gesichert — und zwar die Fassung VOR dem Lauf.
        let sicherung: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(sicherungspfad(&a)).unwrap()).unwrap();
        assert_eq!(sicherung, fremd_a);

        // Ein zweiter Schreibvorgang im selben Lauf überschreibt die Sicherung nicht.
        claude_schreiben(&a, "zweiter", Some(&eintrag), &mut gesichert).unwrap();
        let sicherung: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(sicherungspfad(&a)).unwrap()).unwrap();
        assert_eq!(sicherung, fremd_a);

        // Entfernen nimmt genau den einen Schlüssel.
        claude_schreiben(&a, "playwright", None, &mut gesichert).unwrap();
        assert_eq!(claude_lesen(&a, "playwright").unwrap(), None);
        assert_eq!(
            claude_lesen(&a, "hermes").unwrap(),
            Some(fremd_a["mcpServers"]["hermes"].clone())
        );

        let _ = fs::remove_dir_all(&wiese);
    }

    #[test]
    fn ueberschreibt_keinen_fremden_gleichnamigen_server() {
        let gewuenscht = playwright().claude_eintrag(None);
        let fremd = serde_json::json!({ "type": "stdio", "command": "npx", "args": ["-y", "@playwright/mcp", "--headless"] });

        // Nie selbst geschrieben, abweichend: fremd — wird nicht still ersetzt.
        assert_eq!(urteilen(Some(&fremd), &gewuenscht, None, true), Lage::Fremd);

        // Nie selbst geschrieben, aber genau das Gewünschte: übernehmen.
        assert_eq!(
            urteilen(Some(&gewuenscht), &gewuenscht, None, true),
            Lage::Uebernommen
        );

        // Selbst geschrieben, hier verändert, Instanz still: bleibt.
        let eigen = eintrag_hash(&gewuenscht);
        assert_eq!(
            urteilen(Some(&fremd), &gewuenscht, Some(&eigen), true),
            Lage::LokalGeaendert
        );

        // Selbst geschrieben, hier verändert, Instanz bewegt: Konflikt.
        let neu = playwright().claude_eintrag(Some(Path::new("/usr/bin/chromium")));
        assert_eq!(
            urteilen(Some(&fremd), &neu, Some(&eigen), false),
            Lage::Konflikt
        );

        // Selbst geschrieben, unverändert, Instanz bewegt: aktualisieren.
        assert_eq!(
            urteilen(Some(&gewuenscht), &neu, Some(&eigen), false),
            Lage::Aktualisiert
        );

        assert_eq!(urteilen(None, &gewuenscht, Some(&eigen), true), Lage::Neu);
        assert_eq!(
            urteilen(Some(&gewuenscht), &gewuenscht, Some(&eigen), true),
            Lage::Unveraendert
        );
    }

    #[test]
    fn raeumt_nur_eigene_unveraenderte_eintraege_weg() {
        let eigen = playwright().claude_eintrag(None);
        let hash = eintrag_hash(&eigen);

        assert!(entfernbar(Some(&eigen), &hash));

        let veraendert =
            serde_json::json!({ "type": "stdio", "command": "npx", "args": ["--headless"] });
        assert!(
            !entfernbar(Some(&veraendert), &hash),
            "hier geändert — bleibt"
        );
        assert!(!entfernbar(None, &hash), "schon weg — nichts zu tun");
    }

    #[test]
    fn codex_null_und_leer_sind_dasselbe() {
        let codex = serde_json::json!({
            "name": "playwright",
            "transport": {
                "type": "stdio",
                "command": "npx",
                "args": ["-y", "@playwright/mcp@latest"],
                "env": null,
                "env_vars": [],
                "cwd": null,
            },
        });

        assert_eq!(
            codex_normalisieren(&codex),
            playwright().codex_eintrag(None)
        );
    }
}
