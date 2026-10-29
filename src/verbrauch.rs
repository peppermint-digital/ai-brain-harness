//! Verbrauch der Sitzungen dieses Rechners melden (#4179).
//!
//! Claude Code und Codex schreiben, was sie verbraucht haben, in ihre
//! Protokolldateien. Der Harness liest sie, bildet je Sitzung eine Summe und
//! meldet den **Endstand** an AI Brain. Die Differenz bildet der Server, nicht
//! dieser Code — zwei Zähler laufen irgendwann auseinander, und der Server ist
//! der einzige, der alle Rechner sieht.
//!
//! **Nur Zahlen.** Kein Gesprächsinhalt verlässt diesen Rechner. Gelesen werden
//! ausschliesslich `usage`-Objekte, Sitzungskennungen, Arbeitsverzeichnisse und
//! Modellnamen.
//!
//! ## Zwei Messungen, ohne die das hier falsch wäre
//!
//! **Die Konten teilen sich ein Verzeichnis.** Unter
//! `~/.local/state/imkr/accounts/claude/<konto>/` sind fast alle Einträge
//! Symlinks auf `~/.claude` — `projects/` eingeschlossen. Wer über „alle
//! Konfigurationen" läuft, liest dieselben Dateien mehrfach. Deshalb werden die
//! Verzeichnisse über ihren aufgelösten Pfad entdoppelt. Aus demselben Grund
//! steht in einem Transcript auch **kein Konto**: es ist aus der Datei nicht
//! ableitbar, nur ein Hook im laufenden Gespräch kennt es.
//!
//! **Codex nennt seinen Verbrauch anders als gedacht.** `turn.completed` gibt
//! es nicht (0 von 20 Dateien am 15.10.2026); der Satz heisst
//! `token_usage_record`. Dort steckt `input_tokens` **inklusive** der
//! gecachten, anders als bei Claude.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::config;

/// Was eine Sitzung verbraucht hat — der Endstand, nicht ein Zuwachs.
#[derive(Debug, Clone, PartialEq)]
pub struct Sitzung {
    pub werkzeug: &'static str,
    pub kennung: String,
    pub ein: u64,
    pub aus: u64,
    pub cache: u64,
    pub ein_frisch: Option<u64>,
    pub cache_schreiben: Option<u64>,
    pub modell: Option<String>,
    pub verzeichnis: Option<String>,
    pub dauer_ms: Option<u64>,
}

impl Sitzung {
    /// Ein Fingerabdruck des Endstands.
    ///
    /// Danach entscheidet sich, ob gemeldet wird. Nur Zähler gehen ein — ein
    /// geänderter Modellname allein ist kein Verbrauch und soll keine Meldung
    /// auslösen.
    fn fingerabdruck(&self) -> String {
        format!(
            "{}/{}/{}/{}",
            self.ein,
            self.aus,
            self.cache,
            self.dauer_ms.unwrap_or(0)
        )
    }

    fn schluessel(&self) -> String {
        format!("{}:{}", self.werkzeug, self.kennung)
    }

    fn leer(&self) -> bool {
        self.ein == 0 && self.aus == 0 && self.cache == 0
    }
}

// ---------------------------------------------------------------------------
// Einwilligung
// ---------------------------------------------------------------------------

fn schalter_pfad() -> Result<PathBuf, String> {
    Ok(config::config_dir()?.join("verbrauch-aus"))
}

/// Ist die Erfassung auf diesem Rechner eingeschaltet?
///
/// Standardmässig ja (Entscheidung 15.10.2026). Abgeschaltet wird durch eine
/// Datei, nicht durch einen Eintrag in einer Konfiguration: so sieht man beim
/// Nachsehen sofort, woran es liegt, und `verbrauch aus` lässt sich ohne
/// JSON-Bearbeitung rückgängig machen.
pub fn eingeschaltet() -> bool {
    schalter_pfad().map(|p| !p.exists()).unwrap_or(true)
}

fn hinweis_pfad() -> Result<PathBuf, String> {
    Ok(config::config_dir()?.join("verbrauch-angesagt"))
}

/// Beim ersten Lauf einmal sichtbar sagen, dass gemessen wird.
///
/// Eine Erfassung, die standardmässig läuft, muss sich wenigstens einmal
/// zeigen. Danach schweigt sie — ein Hinweis bei jedem Lauf würde überlesen
/// und wäre damit auch keine Ansage mehr.
fn ansagen_falls_neu() {
    let Ok(pfad) = hinweis_pfad() else {
        return;
    };

    if pfad.exists() {
        return;
    }

    eprintln!();
    eprintln!("  ┌─ Verbrauch wird erfasst");
    eprintln!("  │");
    eprintln!("  │  Dieser Rechner meldet an AI Brain, wie viele Token und wie viel Zeit");
    eprintln!("  │  deine Claude- und Codex-Sitzungen verbraucht haben. Uebertragen werden");
    eprintln!("  │  ausschliesslich Zaehler — kein Gespraechsinhalt, keine Dateinamen.");
    eprintln!("  │");
    eprintln!("  │  Abschalten:  ai-brain-harness verbrauch aus");
    eprintln!("  │");
    eprintln!("  └─ Diese Ansage kommt nur einmal.");
    eprintln!();

    let _ = std::fs::write(&pfad, "angesagt\n");
}

// ---------------------------------------------------------------------------
// Abgleichstand
// ---------------------------------------------------------------------------

fn stand_pfad() -> Result<PathBuf, String> {
    Ok(config::config_dir()?.join("verbrauch-stand.json"))
}

fn stand_laden() -> BTreeMap<String, String> {
    let Ok(pfad) = stand_pfad() else {
        return BTreeMap::new();
    };

    let Ok(text) = std::fs::read_to_string(&pfad) else {
        return BTreeMap::new();
    };

    serde_json::from_str(&text).unwrap_or_default()
}

fn stand_schreiben(stand: &BTreeMap<String, String>) -> Result<(), String> {
    let pfad = stand_pfad()?;

    if let Some(ordner) = pfad.parent() {
        std::fs::create_dir_all(ordner)
            .map_err(|e| format!("{} ist nicht anlegbar: {e}", ordner.display()))?;
    }

    let text = serde_json::to_string_pretty(stand)
        .map_err(|e| format!("Abgleichstand ist nicht schreibbar: {e}"))?;

    std::fs::write(&pfad, format!("{text}\n"))
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", pfad.display()))
}

// ---------------------------------------------------------------------------
// Claude
// ---------------------------------------------------------------------------

/// Alle Transcript-Verzeichnisse — **entdoppelt**.
///
/// `~/.claude/projects` und `…/accounts/claude/<konto>/projects` sind auf
/// diesem Rechner dieselbe Stelle. Ohne die Auflösung läse der Lauf jede Datei
/// so oft, wie es Konten gibt.
fn claude_verzeichnisse() -> Vec<PathBuf> {
    let mut kandidaten: Vec<PathBuf> = Vec::new();

    if let Ok(heim) = config::heimat() {
        kandidaten.push(heim.join(".claude/projects"));

        let konten = heim.join(".local/state/imkr/accounts/claude");

        if let Ok(eintraege) = std::fs::read_dir(&konten) {
            for eintrag in eintraege.flatten() {
                kandidaten.push(eintrag.path().join("projects"));
            }
        }
    }

    if let Ok(eigen) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !eigen.trim().is_empty() {
            kandidaten.push(PathBuf::from(eigen).join("projects"));
        }
    }

    let mut gesehen = BTreeSet::new();
    let mut ergebnis = Vec::new();

    for pfad in kandidaten {
        let Ok(echt) = std::fs::canonicalize(&pfad) else {
            continue;
        };

        if gesehen.insert(echt.clone()) {
            ergebnis.push(echt);
        }
    }

    ergebnis
}

fn jsonl_dateien(wurzel: &Path, tiefe: usize, sammlung: &mut Vec<PathBuf>) {
    if tiefe == 0 {
        return;
    }

    let Ok(eintraege) = std::fs::read_dir(wurzel) else {
        return;
    };

    for eintrag in eintraege.flatten() {
        let pfad = eintrag.path();

        if pfad.is_dir() {
            jsonl_dateien(&pfad, tiefe - 1, sammlung);
        } else if pfad.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            sammlung.push(pfad);
        }
    }
}

/// Eine Claude-Transcriptdatei zu einer Sitzung zusammenfassen.
///
/// Die `usage`-Objekte stehen **je Nachricht** und werden summiert. Die
/// Feldbedeutung folgt den Channels: `ein` ist frische Eingabe plus
/// Cache-Schreiben, `cache` ist der gelesene Cache.
fn claude_datei(pfad: &Path) -> Option<Sitzung> {
    let text = std::fs::read_to_string(pfad).ok()?;

    // Welches Arbeitsverzeichnis die Sitzung praegt, entscheidet die HAEUFIGKEIT,
    // nicht die erste Nennung. Gemessen am 16.10.2026: eine Sitzung nannte als
    // erstes die Heimat und arbeitete dann 337 Saetze lang in einem anderen
    // Ordner. Bei einer normalen Projektsitzung stimmen beide ueberein (742×
    // derselbe Pfad), der Unterschied kostet also nichts und gewinnt die
    // wandernden Faelle.
    let mut verzeichnisse: BTreeMap<String, usize> = BTreeMap::new();

    let mut s = Sitzung {
        werkzeug: "claude",
        kennung: String::new(),
        ein: 0,
        aus: 0,
        cache: 0,
        ein_frisch: Some(0),
        cache_schreiben: Some(0),
        modell: None,
        verzeichnis: None,
        dauer_ms: Some(0),
    };

    for zeile in text.lines() {
        let Ok(satz) = serde_json::from_str::<serde_json::Value>(zeile) else {
            continue;
        };

        if s.kennung.is_empty() {
            if let Some(k) = satz.get("sessionId").and_then(|v| v.as_str()) {
                s.kennung = k.to_string();
            }
        }

        if let Some(v) = satz.get("cwd").and_then(|v| v.as_str()) {
            *verzeichnisse.entry(v.to_string()).or_insert(0) += 1;
        }

        // Die Zugdauer steht in eigenen Saetzen, nicht am `usage`.
        if satz.get("subtype").and_then(|v| v.as_str()) == Some("turn_duration") {
            if let Some(ms) = satz.get("durationMs").and_then(|v| v.as_u64()) {
                s.dauer_ms = Some(s.dauer_ms.unwrap_or(0) + ms);
            }
        }

        let Some(nachricht) = satz.get("message") else {
            continue;
        };

        if let Some(m) = nachricht.get("model").and_then(|v| v.as_str()) {
            s.modell = Some(m.to_string());
        }

        let Some(u) = nachricht.get("usage") else {
            continue;
        };

        let zahl = |name: &str| u.get(name).and_then(|v| v.as_u64()).unwrap_or(0);

        let frisch = zahl("input_tokens");
        let schreiben = zahl("cache_creation_input_tokens");

        s.ein += frisch + schreiben;
        s.aus += zahl("output_tokens");
        s.cache += zahl("cache_read_input_tokens");
        s.ein_frisch = Some(s.ein_frisch.unwrap_or(0) + frisch);
        s.cache_schreiben = Some(s.cache_schreiben.unwrap_or(0) + schreiben);
    }

    s.verzeichnis = verzeichnisse
        .into_iter()
        .max_by_key(|(pfad, anzahl)| (*anzahl, pfad.clone()))
        .map(|(pfad, _)| pfad);

    if s.kennung.is_empty() {
        // Ohne Kennung kann der Server keinen Zuwachs bilden. Der Dateiname ist
        // bei Claude die Sitzungskennung — das ist der Rueckfall, aber ein
        // belegter: so heissen die Dateien.
        s.kennung = pfad.file_stem()?.to_str()?.to_string();
    }

    if s.leer() {
        return None;
    }

    Some(s)
}

pub fn claude_sitzungen() -> Vec<Sitzung> {
    let mut dateien = Vec::new();

    for verzeichnis in claude_verzeichnisse() {
        jsonl_dateien(&verzeichnis, 3, &mut dateien);
    }

    dateien.iter().filter_map(|p| claude_datei(p)).collect()
}

// ---------------------------------------------------------------------------
// Codex
// ---------------------------------------------------------------------------

/// Eine Codex-Sitzungsdatei zusammenfassen.
///
/// **Eine offene Frage, die ehrlich benannt gehört.** Ein `token_usage_record`
/// führt `usage` UND `turn_token_usage`. Ob `usage` die Summe der Sitzung ist
/// oder nur dieser Zug, liess sich am 15.10.2026 nicht entscheiden: auf diesem
/// Rechner hat genau **eine** Datei einen solchen Satz, und die hat genau
/// einen — beide Felder sind dort gleich.
///
/// Darum wird das Grössere von beidem genommen: die Summe der Zugwerte und der
/// letzte `usage`-Stand. Ist `usage` kumulativ, gewinnt es; ist es ein
/// Zugwert, gewinnt die Summe. Beide Lesarten führen damit zum richtigen
/// Ergebnis, statt dass eine davon still daneben liegt.
///
/// Sobald hier eine mehrzügige Codex-Sitzung liegt, gehört das nachgemessen und
/// diese Stelle vereinfacht.
fn codex_datei(pfad: &Path) -> Option<Sitzung> {
    let text = std::fs::read_to_string(pfad).ok()?;

    let mut kennung = String::new();
    let mut verzeichnis = None;
    let mut modell = None;

    let (mut summe_ein, mut summe_aus, mut summe_cache, mut summe_schreiben) = (0, 0, 0, 0);
    let (mut letzt_ein, mut letzt_aus, mut letzt_cache, mut letzt_schreiben) = (0, 0, 0, 0);

    for zeile in text.lines() {
        let Ok(satz) = serde_json::from_str::<serde_json::Value>(zeile) else {
            continue;
        };

        match satz.get("type").and_then(|v| v.as_str()) {
            Some("session_meta") => {
                let p = satz.get("payload").cloned().unwrap_or_default();

                if let Some(k) = p.get("id").and_then(|v| v.as_str()) {
                    kennung = k.to_string();
                }
                if let Some(v) = p.get("cwd").and_then(|v| v.as_str()) {
                    verzeichnis = Some(v.to_string());
                }
                if let Some(m) = p.get("model_provider").and_then(|v| v.as_str()) {
                    modell = Some(m.to_string());
                }
            }
            Some("token_usage_record") => {
                let p = satz.get("payload").cloned().unwrap_or_default();

                if kennung.is_empty() {
                    if let Some(k) = p.get("session_id").and_then(|v| v.as_str()) {
                        kennung = k.to_string();
                    }
                }

                let lies = |wo: &serde_json::Value, name: &str| -> u64 {
                    wo.get(name).and_then(|v| v.as_u64()).unwrap_or(0)
                };

                if let Some(zug) = p.get("turn_token_usage") {
                    // `input_tokens` enthaelt bei Codex die gecachten mit —
                    // anders als bei Claude. Also herausrechnen, sonst zaehlt
                    // der Cache doppelt.
                    let roh = lies(zug, "input_tokens");
                    let gecacht = lies(zug, "cached_input_tokens");

                    summe_ein += roh.saturating_sub(gecacht);
                    summe_cache += gecacht;
                    summe_schreiben += lies(zug, "cache_write_input_tokens");
                    summe_aus += lies(zug, "output_tokens");
                }

                if let Some(ges) = p.get("usage") {
                    let roh = lies(ges, "input_tokens");
                    let gecacht = lies(ges, "cached_input_tokens");

                    letzt_ein = roh.saturating_sub(gecacht);
                    letzt_cache = gecacht;
                    letzt_schreiben = lies(ges, "cache_write_input_tokens");
                    letzt_aus = lies(ges, "output_tokens");
                }
            }
            _ => {}
        }
    }

    if kennung.is_empty() {
        return None;
    }

    let frisch = summe_ein.max(letzt_ein);
    let schreiben = summe_schreiben.max(letzt_schreiben);

    let s = Sitzung {
        werkzeug: "codex",
        kennung,
        ein: frisch + schreiben,
        aus: summe_aus.max(letzt_aus),
        cache: summe_cache.max(letzt_cache),
        ein_frisch: Some(frisch),
        cache_schreiben: Some(schreiben),
        modell,
        verzeichnis,
        // Codex fuehrt ein `duration_ms`, aber nicht je Zug und nicht
        // verlaesslich — bis das gemessen ist, bleibt das Feld leer statt
        // gefuellt mit etwas Ungeprueftem.
        dauer_ms: None,
    };

    if s.leer() {
        return None;
    }

    Some(s)
}

pub fn codex_sitzungen() -> Vec<Sitzung> {
    let Ok(heim) = config::heimat() else {
        return Vec::new();
    };

    let mut dateien = Vec::new();
    // `~/.codex/sessions/JAHR/MONAT/TAG/datei.jsonl` — vier Ebenen.
    jsonl_dateien(&heim.join(".codex/sessions"), 4, &mut dateien);

    dateien.iter().filter_map(|p| codex_datei(p)).collect()
}

// ---------------------------------------------------------------------------
// Konto-Merkzettel
// ---------------------------------------------------------------------------

fn konten_pfad() -> Result<PathBuf, String> {
    Ok(config::config_dir()?.join("verbrauch-konten.json"))
}

/// Welches Konto gehoert zu welcher Sitzung?
///
/// ## Warum es diesen Merkzettel braucht
///
/// Alle imkr-Konten teilen dasselbe Transcript-Verzeichnis
/// (`~/.claude/projects` ist in jeden Kontoordner hineingelinkt). Aus einer
/// Protokolldatei allein ist deshalb nicht ableitbar, unter welchem Konto die
/// Sitzung lief — und das aktuelle Konto auf alle zu stempeln war am
/// 16.10.2026 schon einmal falsch: 2066 historische Sitzungen bekamen
/// „henneberg", obwohl ein Teil unter `department` lief. Siehe {@link konto}.
///
/// Der Merkzettel loest das, ohne zu raten: Jede Zeile stammt aus einer
/// MESSUNG — entweder aus dem Hook (dort ist `CLAUDE_CONFIG_DIR` die Wahrheit
/// ueber DIESE Sitzung) oder aus den Kontoordnern selbst
/// ({@link konten_aus_kontoordnern}).
///
/// Dass der Merkzettel bleibt, ist der halbe Zweck: `lastSessionId` kennt je
/// Projekt nur die JUENGSTE Sitzung. Was heute dort steht, ist morgen
/// ueberschrieben. Jeder Lauf legt den heutigen Stand dazu, und damit waechst
/// die Abdeckung, statt mit jedem Lauf von vorn zu beginnen.
fn konten_laden() -> BTreeMap<String, String> {
    let Ok(pfad) = konten_pfad() else {
        return BTreeMap::new();
    };

    let Ok(text) = std::fs::read_to_string(&pfad) else {
        return BTreeMap::new();
    };

    serde_json::from_str(&text).unwrap_or_default()
}

fn konten_schreiben(konten: &BTreeMap<String, String>) -> Result<(), String> {
    let pfad = konten_pfad()?;

    if let Some(ordner) = pfad.parent() {
        std::fs::create_dir_all(ordner)
            .map_err(|e| format!("{} ist nicht anlegbar: {e}", ordner.display()))?;
    }

    let text = serde_json::to_string_pretty(konten)
        .map_err(|e| format!("Konto-Merkzettel ist nicht schreibbar: {e}"))?;

    std::fs::write(&pfad, format!("{text}\n"))
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", pfad.display()))
}

/// Die Kontoordner von Claude Code auf diesem Rechner.
///
/// Der Standardordner gehoert ausdruecklich dazu. Auf der codingmachine ist
/// `~/.claude` nicht „kein Konto", sondern ein angemeldetes — und es war der
/// groesste blinde Fleck der Auswertung: 57 Sitzungen mit 28 M Ausgabe-Token
/// in sieben Tagen, keiner Person und keinem Konto zugeordnet.
fn kontoordner(heim: &Path) -> Vec<PathBuf> {
    let mut ordner = vec![heim.join(".claude")];

    let imkr = heim.join(".local/state/imkr/accounts/claude");

    if let Ok(eintraege) = std::fs::read_dir(&imkr) {
        let mut weitere: Vec<PathBuf> = eintraege
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        weitere.sort();
        ordner.extend(weitere);
    }

    ordner
}

/// Wie das Konto eines Ordners heisst — aus der Anmeldung, nicht aus dem Pfad.
///
/// `oauthAccount.emailAddress` in `.claude.json` nennt, wer angemeldet ist.
/// Der Teil vor dem `@` ist genau der Name, den `imkr` fuehrt
/// (`henneberg@…` → `henneberg`) — und er stimmt auch fuer den
/// Standardordner, der keinen Kontonamen im Pfad hat.
///
/// Bewusst nicht der Ordnername: Der stimmt bei imkr-Ordnern zufaellig und beim
/// Standardordner gar nicht (`.claude` ist kein Konto).
fn konto_des_ordners(ordner: &Path) -> Option<String> {
    let daten = kontodatei(ordner)?;

    let adresse = daten
        .get("oauthAccount")?
        .get("emailAddress")?
        .as_str()?
        .trim()
        .to_lowercase();

    let name = adresse.split('@').next()?.trim().to_string();

    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Die `.claude.json` eines Kontoordners — und sie liegt an ZWEI Orten.
///
/// Bei einem imkr-Kontoordner (`CLAUDE_CONFIG_DIR` gesetzt) liegt sie IM Ordner:
/// `…/accounts/claude/henneberg/.claude.json`. Beim Standardordner `~/.claude`
/// liegt sie DANEBEN: `~/.claude.json`.
///
/// Am 29.10.2026 genau daran gescheitert: Der Merkzettel suchte nur im Ordner
/// und fand das Standardkonto nicht — also ausgerechnet das, das den groessten
/// blinden Fleck ausmacht (auf der codingmachine ist `~/.claude` bei stelljes
/// angemeldet, mit 2798 Verbrauchszeilen ohne Zuordnung). Die Funktion sah
/// richtig aus, weil sie fuer die imkr-Ordner stimmte.
fn kontodatei(ordner: &Path) -> Option<serde_json::Value> {
    let mut orte = vec![ordner.join(".claude.json")];

    // `~/.claude` → `~/.claude.json`. Aus dem Ordner selbst abgeleitet, nicht
    // aus HOME: so stimmt es auch fuer einen Testordner.
    if let (Some(name), Some(eltern)) = (
        ordner.file_name().and_then(|n| n.to_str()),
        ordner.parent(),
    ) {
        orte.push(eltern.join(format!("{name}.json")));
    }

    for ort in orte {
        if let Ok(text) = std::fs::read_to_string(&ort) {
            if let Ok(daten) = serde_json::from_str::<serde_json::Value>(&text) {
                return Some(daten);
            }
        }
    }

    None
}

/// Sitzung → Konto, gelesen aus den Kontoordnern.
///
/// Claude Code haelt je Projekt die juengste Sitzung in
/// `projects.<pfad>.lastSessionId` — und zwar im Ordner des Kontos, unter dem
/// sie lief. Das ist der einzige Ort, an dem die Zuordnung nach dem Lauf noch
/// steht.
///
/// **Taucht eine Sitzung unter ZWEI Konten auf, wird sie weggelassen.** Das
/// sollte nicht vorkommen; wenn doch, ist die Zuordnung unklar, und eine
/// unklare gehoert nicht in eine Auswertung. Eine Luecke sieht man, eine
/// falsche Zuordnung nicht.
fn konten_aus_kontoordnern(heim: &Path) -> BTreeMap<String, String> {
    let mut gefunden: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for ordner in kontoordner(heim) {
        let Some(konto) = konto_des_ordners(&ordner) else {
            continue;
        };

        let Some(daten) = kontodatei(&ordner) else {
            continue;
        };

        let Some(projekte) = daten.get("projects").and_then(|p| p.as_object()) else {
            continue;
        };

        for (_pfad, eintrag) in projekte {
            if let Some(sitzung) = eintrag.get("lastSessionId").and_then(|s| s.as_str()) {
                if !sitzung.trim().is_empty() {
                    gefunden
                        .entry(format!("claude:{}", sitzung.trim()))
                        .or_default()
                        .insert(konto.clone());
                }
            }
        }
    }

    gefunden
        .into_iter()
        .filter_map(|(schluessel, konten)| {
            if konten.len() == 1 {
                konten.into_iter().next().map(|k| (schluessel, k))
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Melden
// ---------------------------------------------------------------------------

pub fn alle_sitzungen() -> Vec<Sitzung> {
    let mut alle = claude_sitzungen();
    alle.extend(codex_sitzungen());
    alle
}

/// Welche Sitzungen haben sich seit der letzten Meldung geändert?
pub fn offene(alle: &[Sitzung], stand: &BTreeMap<String, String>) -> Vec<Sitzung> {
    alle.iter()
        .filter(|s| stand.get(&s.schluessel()) != Some(&s.fingerabdruck()))
        .cloned()
        .collect()
}

fn nutzlast(s: &Sitzung, konto: Option<&str>) -> serde_json::Value {
    let mut argumente = serde_json::json!({
        "werkzeug": s.werkzeug,
        "sitzung": s.kennung,
        "ein": s.ein,
        "aus": s.aus,
        "cache": s.cache,
    });

    let mut setzen = |name: &str, wert: serde_json::Value| {
        if !wert.is_null() {
            argumente[name] = wert;
        }
    };

    setzen("ein_frisch", s.ein_frisch.into());
    setzen("cache_schreiben", s.cache_schreiben.into());
    setzen("dauer_ms", s.dauer_ms.filter(|d| *d > 0).into());
    setzen("modell", s.modell.clone().into());
    setzen("verzeichnis", s.verzeichnis.clone().into());
    setzen("konto", konto.map(str::to_string).into());

    argumente
}

/// Marke, an der unser Hook wiedererkannt wird.
///
/// Ohne eine Marke liesse sich beim Entfernen nicht sagen, welcher Eintrag
/// unserer ist — und ein Entferner, der alles wegraeumt, nimmt fremde Hooks
/// mit. Auf diesem Rechner haengt an `SessionEnd` bereits imkr.
const HOOK_MARKE: &str = "verbrauch melden --vom-hook";

fn hook_befehl() -> String {
    format!("ai-brain-harness {HOOK_MARKE} --still --grenze 5")
}

fn einstellungen_pfad() -> Result<PathBuf, String> {
    Ok(config::heimat()?.join(".claude/settings.json"))
}

/// Trägt den SessionEnd-Hook ein — **neben** die vorhandenen.
///
/// **Warum ausdrücklich und nicht über `sync`.** Ein Hook läuft bei jedem
/// Sitzungsende mit; so etwas gehört nicht still eingerichtet. Deshalb ein
/// eigener Befehl, den man aufruft, und eine Ansage, was er tut.
fn hook_einrichten() -> Result<(), String> {
    let pfad = einstellungen_pfad()?;

    let text = std::fs::read_to_string(&pfad)
        .map_err(|e| format!("{} ist nicht lesbar: {e}", pfad.display()))?;

    let mut wert: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} ist kein JSON: {e}", pfad.display()))?;

    let liste = wert
        .as_object_mut()
        .ok_or_else(|| "Die Einstellungen sind kein Objekt.".to_string())?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "`hooks` ist kein Objekt.".to_string())?
        .entry("SessionEnd")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| "`SessionEnd` ist keine Liste.".to_string())?;

    if text.contains(HOOK_MARKE) {
        println!("Der Hook ist schon eingetragen.");
        return Ok(());
    }

    let vorhanden = liste.len();

    liste.push(serde_json::json!({
        "matcher": "*",
        "hooks": [{
            "type": "command",
            "command": hook_befehl(),
            // Knapp halten: ein Hook, der am Sitzungsende haengt, haelt den
            // Menschen auf. Was er nicht schafft, holt der naechste
            // nachholende Lauf.
            "timeout": 20
        }]
    }));

    // Vorher sichern. Diese Datei traegt fremde Hooks; ein kaputter Schreibzug
    // legte imkrs Zustandsmeldung lahm, und das faellt erst viel spaeter auf.
    let sicherung = pfad.with_extension("json.vor-verbrauch");
    std::fs::write(&sicherung, &text)
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", sicherung.display()))?;

    let neu = serde_json::to_string_pretty(&wert)
        .map_err(|e| format!("Einstellungen sind nicht schreibbar: {e}"))?;

    std::fs::write(&pfad, format!("{neu}\n"))
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", pfad.display()))?;

    println!("SessionEnd-Hook eingetragen.");
    println!("  Befehl   : {}", hook_befehl());
    println!("  Daneben  : {vorhanden} vorhandene Eintraege blieben stehen");
    println!("  Sicherung: {}", sicherung.display());
    println!();
    println!("Der Hook meldet am Sitzungsende NUR die endende Sitzung — und als");
    println!("einziger Weg kennt er das Konto, unter dem sie lief.");

    Ok(())
}

fn hook_entfernen() -> Result<(), String> {
    let pfad = einstellungen_pfad()?;

    let text = std::fs::read_to_string(&pfad)
        .map_err(|e| format!("{} ist nicht lesbar: {e}", pfad.display()))?;

    let mut wert: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} ist kein JSON: {e}", pfad.display()))?;

    let Some(liste) = wert
        .pointer_mut("/hooks/SessionEnd")
        .and_then(|v| v.as_array_mut())
    else {
        println!("Kein SessionEnd-Hook eingetragen.");
        return Ok(());
    };

    let vorher = liste.len();

    // Nur unseren. Erkennungszeichen ist die Marke, nicht die Position.
    liste.retain(|eintrag| !eintrag.to_string().contains(HOOK_MARKE));

    if liste.len() == vorher {
        println!("Unser Hook war nicht eingetragen — nichts geaendert.");
        return Ok(());
    }

    let geblieben = liste.len();

    let neu = serde_json::to_string_pretty(&wert)
        .map_err(|e| format!("Einstellungen sind nicht schreibbar: {e}"))?;

    std::fs::write(&pfad, format!("{neu}\n"))
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", pfad.display()))?;

    println!("Hook entfernt. {geblieben} fremde Eintraege blieben stehen.");

    Ok(())
}

const HILFE: &str = "\
ai-brain-harness verbrauch — Token und Zeit der Sitzungen dieses Rechners melden

  verbrauch melden [--zeigen]   Noch nicht gemeldete Sitzungen an AI Brain geben
  verbrauch zeigen              Nur anzeigen, was gemeldet wuerde
  verbrauch aus                 Erfassung auf diesem Rechner abschalten
  verbrauch an                  Wieder einschalten
  verbrauch stand               Was zuletzt gemeldet wurde
  verbrauch hook ein|aus        SessionEnd-Hook eintragen oder entfernen

Zusaetze:
  --konto <name>   Konto, unter dem die Sitzungen liefen. Aus den Protokollen
                   ist es NICHT ableitbar (alle Konten teilen ein Verzeichnis),
                   darum setzt es der SessionEnd-Hook.
  --grenze <n>     Hoechstens n Sitzungen in einem Lauf melden (Vorgabe 50).
  --sitzung <id>   Nur diese eine Sitzung melden.
  --vom-hook       Die Sitzungskennung aus der Hook-Eingabe auf stdin lesen.
  --still          Nur melden, wenn etwas schiefgeht.
";

pub fn verwalten(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("melden") => melden(args),
        Some("zeigen") => zeigen(args),
        Some("aus") => abschalten(),
        Some("an") => einschalten(),
        Some("stand") => stand_zeigen(),
        Some("hook") => match args.get(1).map(String::as_str) {
            Some("ein") | Some("einrichten") => hook_einrichten(),
            Some("aus") | Some("entfernen") => hook_entfernen(),
            _ => {
                println!("verbrauch hook ein   — SessionEnd-Hook eintragen");
                println!("verbrauch hook aus   — wieder entfernen");
                Ok(())
            }
        },
        _ => {
            print!("{HILFE}");
            Ok(())
        }
    }
}

fn abschalten() -> Result<(), String> {
    let pfad = schalter_pfad()?;

    if let Some(ordner) = pfad.parent() {
        let _ = std::fs::create_dir_all(ordner);
    }

    std::fs::write(&pfad, "Verbrauch wird auf diesem Rechner nicht erfasst.\n")
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", pfad.display()))?;

    println!("Verbrauchserfassung ist auf diesem Rechner AUS.");
    println!("Es wird nichts mehr gelesen und nichts mehr gemeldet.");
    println!("Wieder an: ai-brain-harness verbrauch an");

    Ok(())
}

fn einschalten() -> Result<(), String> {
    let pfad = schalter_pfad()?;

    if pfad.exists() {
        std::fs::remove_file(&pfad)
            .map_err(|e| format!("{} ist nicht entfernbar: {e}", pfad.display()))?;
    }

    println!("Verbrauchserfassung ist auf diesem Rechner AN.");

    Ok(())
}

fn stand_zeigen() -> Result<(), String> {
    let stand = stand_laden();

    if stand.is_empty() {
        println!("Noch nichts gemeldet.");
        return Ok(());
    }

    println!("Zuletzt gemeldet ({} Sitzungen):", stand.len());
    println!();

    for (schluessel, abdruck) in stand.iter().take(30) {
        println!("  {schluessel}  →  {abdruck}");
    }

    if stand.len() > 30 {
        println!();
        println!("… und {} weitere.", stand.len() - 30);
    }

    Ok(())
}

fn grenze(args: &[String]) -> usize {
    args.iter()
        .position(|a| a == "--grenze")
        .and_then(|i| args.get(i + 1))
        .and_then(|w| w.parse().ok())
        .unwrap_or(50)
}

/// Das Konto, unter dem die gemeldete Sitzung lief.
///
/// **Nur fuer die Sitzung, um die es geht.** `CLAUDE_CONFIG_DIR` sagt, in
/// welchem Konto DIESER Lauf steckt — nicht, in welchem eine Sitzung von
/// vorgestern lief. Ein nachholender Lauf meldet viele Sitzungen, und sie
/// stammen aus verschiedenen Konten; das aktuelle auf alle zu stempeln ist
/// schlimmer als die Luecke, weil es wie eine Messung aussieht.
///
/// Am 16.10.2026 genau so passiert: der erste Nachhol-Lauf schrieb „henneberg"
/// auf 2066 historische Sitzungen, obwohl ein Teil davon unter `department`
/// lief. Darum haengt das Konto jetzt daran, dass eine BESTIMMTE Sitzung
/// gemeint ist — und das ist nur im Hook der Fall.
fn konto(args: &[String], gemeinte: Option<&str>) -> Option<String> {
    if let Some(genannt) = args
        .iter()
        .position(|a| a == "--konto")
        .and_then(|i| args.get(i + 1))
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
    {
        return Some(genannt);
    }

    // Ohne benannte Sitzung ist das hier ein nachholender Lauf — dann bleibt
    // das Konto leer. Es ist aus den Protokollen nicht ableitbar, weil alle
    // Konten dasselbe Transcript-Verzeichnis benutzen.
    gemeinte?;

    let name = crate::konto::aktuelles(&BTreeMap::new());

    if name == "Standard" {
        // „Standard" heisst nur, dass kein Kontoordner im Pfad steht — nicht,
        // dass niemand angemeldet ist. Auf der codingmachine ist `~/.claude`
        // ein angemeldetes Konto (stelljes), und es war der groesste blinde
        // Fleck der Auswertung: 57 Sitzungen mit 28 M Ausgabe-Token in sieben
        // Tagen ohne jede Zuordnung. Die Anmeldung selbst sagt, wer es ist.
        let ordner = crate::konto::konfigurationsordner(&BTreeMap::new())?;

        return konto_des_ordners(Path::new(&ordner));
    }

    // `claude:henneberg` → `henneberg`. Das Werkzeug steht schon in einem
    // eigenen Feld; es hier zu wiederholen machte die Auswertung unleserlich.
    Some(name.rsplit(':').next().unwrap_or(&name).to_string())
}

/// Die Sitzung, um die es geht — genannt oder aus der Hook-Eingabe.
///
/// Claude reicht einem Hook sein JSON auf stdin (so macht es auch
/// `no-host-dev-server.sh` mit `jq`). Darin steht die Kennung der Sitzung, die
/// gerade endet. Ohne sie wuerde der Hook bei jedem Sitzungsende ALLE offenen
/// Sitzungen melden — auf diesem Rechner waren das beim ersten Lauf 2047.
///
/// Fehlt das Feld, wird NICHT geraten: der Lauf meldet dann wie ein
/// nachholender, nur mit kleiner Grenze.
fn gemeinte_sitzung(args: &[String]) -> Option<String> {
    if let Some(genannt) = args
        .iter()
        .position(|a| a == "--sitzung")
        .and_then(|i| args.get(i + 1))
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
    {
        return Some(genannt);
    }

    if !args.iter().any(|a| a == "--vom-hook") {
        return None;
    }

    use std::io::Read;

    let mut roh = String::new();

    if std::io::stdin().read_to_string(&mut roh).is_err() || roh.trim().is_empty() {
        return None;
    }

    let wert: serde_json::Value = serde_json::from_str(&roh).ok()?;

    // Beide Schreibweisen: das Hook-JSON fuehrt `session_id`, die Transcripts
    // `sessionId`. Welche hier ankommt, haengt an der Fassung von Claude Code.
    for name in ["session_id", "sessionId"] {
        if let Some(k) = wert.get(name).and_then(|v| v.as_str()) {
            if !k.trim().is_empty() {
                return Some(k.to_string());
            }
        }
    }

    None
}

fn zeigen(args: &[String]) -> Result<(), String> {
    if !eingeschaltet() {
        println!("Verbrauchserfassung ist auf diesem Rechner AUS.");
        return Ok(());
    }

    let alle = alle_sitzungen();
    let stand = stand_laden();
    let offen = offene(&alle, &stand);

    println!(
        "{} Sitzungen gefunden, {} davon neu oder gewachsen.",
        alle.len(),
        offen.len()
    );
    println!();

    for s in offen.iter().take(grenze(args)) {
        println!(
            "  {:<6} {:<38} ein {:>10}  aus {:>9}  cache {:>12}{}",
            s.werkzeug,
            kurz(&s.kennung, 36),
            s.ein,
            s.aus,
            s.cache,
            s.dauer_ms
                .filter(|d| *d > 0)
                .map(|d| format!("  {:.1} h", d as f64 / 3_600_000.0))
                .unwrap_or_default()
        );
    }

    Ok(())
}

fn kurz(text: &str, grenze: usize) -> String {
    if text.chars().count() <= grenze {
        return text.to_string();
    }

    text.chars().take(grenze - 1).collect::<String>() + "…"
}

fn melden(args: &[String]) -> Result<(), String> {
    if !eingeschaltet() {
        // Abgeschaltet heisst: es wird nichts gelesen. Ein Lauf, der erst liest
        // und dann verwirft, waere etwas anderes als das Versprochene.
        if !args.iter().any(|a| a == "--still") {
            println!("Verbrauchserfassung ist auf diesem Rechner AUS — nichts gelesen.");
        }

        return Ok(());
    }

    if args.iter().any(|a| a == "--zeigen") {
        return zeigen(args);
    }

    ansagen_falls_neu();

    let verbindung = config::load()?;
    let token = crate::mcp::access_token(&verbindung)?;

    let gemeint = gemeinte_sitzung(args);

    let alle: Vec<Sitzung> = match &gemeint {
        // Aus dem Hook heraus zaehlt nur die Sitzung, die gerade endet.
        Some(k) => alle_sitzungen().into_iter().filter(|s| &s.kennung == k).collect(),
        None => alle_sitzungen(),
    };

    let mut stand = stand_laden();
    let offen = offene(&alle, &stand);
    let deckel = grenze(args);
    let konto = konto(args, gemeint.as_deref());

    // Der Merkzettel wird bei JEDEM Lauf aus den Kontoordnern ergaenzt — auch
    // wenn gerade nichts zu melden ist. `lastSessionId` kennt je Projekt nur
    // die juengste Sitzung; was heute dort steht, ist morgen ueberschrieben.
    // Wer erst beim Melden nachsieht, verliert genau die Zuordnungen, die
    // zwischen zwei Meldungen entstanden sind.
    let mut konten = konten_laden();
    let vorher = konten.len();

    if let Ok(heim) = std::env::var("HOME") {
        for (schluessel, name) in konten_aus_kontoordnern(Path::new(&heim)) {
            // Ein aus dem Hook gemerkter Eintrag bleibt stehen: Dort war
            // `CLAUDE_CONFIG_DIR` die Wahrheit ueber DIESE Sitzung, waehrend
            // `lastSessionId` nur sagt, welche Sitzung zuletzt lief.
            konten.entry(schluessel).or_insert(name);
        }
    }

    // Was der Hook weiss, gehoert in den Merkzettel — sonst ist es nach dem
    // Lauf weg, und ein spaeterer Nachtrag derselben Sitzung stuende wieder
    // ohne Konto da.
    if let (Some(kennung), Some(name)) = (gemeint.as_deref(), konto.as_deref()) {
        for s in alle.iter().filter(|s| s.kennung == kennung) {
            konten.insert(s.schluessel(), name.to_string());
        }
    }

    if konten.len() != vorher {
        let _ = konten_schreiben(&konten);
    }

    if offen.is_empty() {
        if !args.iter().any(|a| a == "--still") {
            println!("Nichts Neues — {} Sitzungen, alle schon gemeldet.", alle.len());
        }

        return Ok(());
    }

    let mut gemeldet = 0_usize;
    let mut unveraendert = 0_usize;
    let mut fehler: Vec<String> = Vec::new();

    for s in offen.iter().take(deckel) {
        // Das Konto DIESER Sitzung, nicht das des Laufs. Ein nachholender Lauf
        // meldet viele Sitzungen aus verschiedenen Konten; `konto` ist nur
        // gesetzt, wenn eine bestimmte gemeint ist (Hook). Sonst zaehlt der
        // Merkzettel — und wenn der sie nicht kennt, bleibt es leer. Eine
        // Luecke sieht man, eine falsche Zuordnung nicht.
        let kennung = konto
            .clone()
            .or_else(|| konten.get(&s.schluessel()).cloned());

        let antwort = crate::mcp::call_tool_text(
            &verbindung,
            &token,
            "report-usage-tool",
            nutzlast(s, kennung.as_deref()),
        );

        match antwort {
            Ok(text) => {
                // Erst nach der Bestaetigung merken. Wer vorher schreibt,
                // verliert bei einem Fehlschlag den Verbrauch dauerhaft — der
                // naechste Lauf haelt ihn dann fuer gemeldet.
                stand.insert(s.schluessel(), s.fingerabdruck());

                if text.contains("unveraendert") {
                    unveraendert += 1;
                } else {
                    gemeldet += 1;
                }
            }
            Err(grund) => fehler.push(format!("{}: {grund}", kurz(&s.kennung, 20))),
        }
    }

    stand_schreiben(&stand)?;

    let still = args.iter().any(|a| a == "--still");

    if !still || !fehler.is_empty() {
        println!(
            "{gemeldet} gemeldet, {unveraendert} unveraendert, {} offen geblieben.",
            offen.len().saturating_sub(deckel.min(offen.len()))
        );
    }

    if !fehler.is_empty() {
        println!();
        println!("Nicht gemeldet:");

        for f in fehler.iter().take(5) {
            println!("  {f}");
        }

        // Ein Fehlschlag ist kein Abbruch: der naechste Lauf holt nach. Aber er
        // darf auch nicht als Erfolg durchgehen.
        return Err(format!("{} Sitzungen konnten nicht gemeldet werden.", fehler.len()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wegwerf(name: &str, inhalt: &str) -> PathBuf {
        let ordner = std::env::temp_dir().join(format!("verbrauch-test-{}", std::process::id()));
        std::fs::create_dir_all(&ordner).expect("Testordner");

        let pfad = ordner.join(name);
        std::fs::write(&pfad, inhalt).expect("Testdatei");

        pfad
    }

    /// Ein Kontoordner, wie Claude Code ihn hinterlaesst.
    fn kontoordner_bauen(heim: &Path, pfad: &str, adresse: &str, sitzungen: &[(&str, &str)]) {
        let ordner = heim.join(pfad);
        std::fs::create_dir_all(&ordner).expect("Kontoordner");

        let projekte: serde_json::Map<String, serde_json::Value> = sitzungen
            .iter()
            .map(|(projekt, sitzung)| {
                (
                    (*projekt).to_string(),
                    serde_json::json!({ "lastSessionId": sitzung }),
                )
            })
            .collect();

        let inhalt = serde_json::json!({
            "oauthAccount": { "emailAddress": adresse },
            "projects": projekte,
        });

        // Beim Standardordner liegt die Datei DANEBEN (`~/.claude.json`), bei
        // einem imkr-Kontoordner DRIN. Der Helfer macht es genauso, sonst
        // prueft der Test eine Welt, die es nicht gibt.
        let ziel = if pfad == ".claude" {
            heim.join(".claude.json")
        } else {
            ordner.join(".claude.json")
        };

        std::fs::write(ziel, serde_json::to_string(&inhalt).expect("JSON")).expect("claude.json");
    }

    fn wegwerf_heim(name: &str) -> PathBuf {
        let heim = std::env::temp_dir().join(format!(
            "verbrauch-heim-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&heim);
        std::fs::create_dir_all(&heim).expect("Heimverzeichnis");

        heim
    }

    // -- Konto-Merkzettel ----------------------------------------------------

    #[test]
    fn das_konto_kommt_aus_der_anmeldung_und_nicht_aus_dem_ordnernamen() {
        // Der Ordner heisst `.claude` und ist trotzdem ein Konto. Genau das war
        // der groesste blinde Fleck: Auf der codingmachine ist der
        // Standardordner bei stelljes angemeldet, und 57 Sitzungen mit 28 M
        // Ausgabe-Token lagen deshalb ohne jede Zuordnung in der Auswertung.
        let heim = wegwerf_heim("anmeldung");
        kontoordner_bauen(&heim, ".claude", "stelljes@peppermint-digital.de", &[]);

        assert_eq!(
            konto_des_ordners(&heim.join(".claude")).as_deref(),
            Some("stelljes")
        );
    }

    #[test]
    fn die_datei_des_standardkontos_liegt_neben_dem_ordner() {
        // DER FEHLER vom 29.10.2026. Der Merkzettel suchte nur
        // `~/.claude/.claude.json` — die gibt es nicht. Die Datei des
        // Standardkontos liegt in `~/.claude.json`. Damit fehlte ausgerechnet
        // das Konto mit den meisten unzugeordneten Zeilen (2798).
        let heim = wegwerf_heim("daneben");
        std::fs::create_dir_all(heim.join(".claude")).expect("Ordner");
        std::fs::write(
            heim.join(".claude.json"),
            serde_json::to_string(&serde_json::json!({
                "oauthAccount": { "emailAddress": "stelljes@peppermint-digital.de" },
                "projects": { "-home-x-a": { "lastSessionId": "sitzung-standard" } },
            }))
            .expect("JSON"),
        )
        .expect("Datei");

        assert_eq!(
            konto_des_ordners(&heim.join(".claude")).as_deref(),
            Some("stelljes"),
            "die Datei neben dem Ordner wird nicht gelesen"
        );
        assert_eq!(
            konten_aus_kontoordnern(&heim)
                .get("claude:sitzung-standard")
                .map(String::as_str),
            Some("stelljes")
        );
    }

    #[test]
    fn der_standardordner_zaehlt_mit() {
        let heim = wegwerf_heim("standardordner");
        kontoordner_bauen(&heim, ".claude", "stelljes@peppermint-digital.de", &[]);
        kontoordner_bauen(
            &heim,
            ".local/state/imkr/accounts/claude/department",
            "department@peppermint-digital.de",
            &[],
        );

        let ordner = kontoordner(&heim);

        assert!(ordner.contains(&heim.join(".claude")), "Standardordner fehlt");
        assert!(ordner
            .contains(&heim.join(".local/state/imkr/accounts/claude/department")));
    }

    #[test]
    fn sitzungen_werden_ihrem_konto_zugeordnet() {
        let heim = wegwerf_heim("zuordnung");
        kontoordner_bauen(
            &heim,
            ".local/state/imkr/accounts/claude/department",
            "department@peppermint-digital.de",
            &[("-home-x-verwaltung", "sitzung-dep")],
        );
        kontoordner_bauen(
            &heim,
            ".local/state/imkr/accounts/claude/henneberg",
            "henneberg@peppermint-digital.de",
            &[("-home-x-ai-brain", "sitzung-hen")],
        );

        let konten = konten_aus_kontoordnern(&heim);

        assert_eq!(konten.get("claude:sitzung-dep").map(String::as_str), Some("department"));
        assert_eq!(konten.get("claude:sitzung-hen").map(String::as_str), Some("henneberg"));
    }

    #[test]
    fn eine_sitzung_unter_zwei_konten_wird_weggelassen() {
        // DER KERN der Vorsicht. Eine unklare Zuordnung gehoert nicht in eine
        // Auswertung: Eine Luecke sieht man, eine falsche Zuordnung nicht. Am
        // 16.10.2026 bekamen 2066 Sitzungen ein geratenes Konto, und niemand
        // konnte es hinterher erkennen.
        let heim = wegwerf_heim("mehrdeutig");
        kontoordner_bauen(
            &heim,
            ".local/state/imkr/accounts/claude/department",
            "department@peppermint-digital.de",
            &[("-home-x-a", "dieselbe")],
        );
        kontoordner_bauen(
            &heim,
            ".local/state/imkr/accounts/claude/henneberg",
            "henneberg@peppermint-digital.de",
            &[("-home-x-b", "dieselbe")],
        );

        assert!(
            !konten_aus_kontoordnern(&heim).contains_key("claude:dieselbe"),
            "eine mehrdeutige Sitzung darf nicht zugeordnet werden"
        );
    }

    #[test]
    fn ein_ordner_ohne_anmeldung_liefert_nichts() {
        let heim = wegwerf_heim("ohne-anmeldung");
        let ordner = heim.join(".claude");
        std::fs::create_dir_all(&ordner).expect("Ordner");
        std::fs::write(ordner.join(".claude.json"), r#"{"projects":{}}"#).expect("Datei");

        assert_eq!(konto_des_ordners(&ordner), None);
        assert!(konten_aus_kontoordnern(&heim).is_empty());
    }

    // -- Claude --------------------------------------------------------------

    /// Die Feldnamen stammen aus einem echten Transcript (15.10.2026).
    const CLAUDE: &str = r#"{"sessionId":"abc-123","cwd":"/home/x/Development/ai-brain","type":"assistant","message":{"model":"claude-opus-5","usage":{"input_tokens":2,"cache_creation_input_tokens":500,"cache_read_input_tokens":30000,"output_tokens":196}}}
{"sessionId":"abc-123","type":"system","subtype":"turn_duration","durationMs":145908}
{"sessionId":"abc-123","type":"assistant","message":{"model":"claude-opus-5","usage":{"input_tokens":3,"cache_creation_input_tokens":100,"cache_read_input_tokens":40000,"output_tokens":50}}}
{"sessionId":"abc-123","type":"system","subtype":"turn_duration","durationMs":54092}
"#;

    #[test]
    fn claude_summiert_je_nachricht_und_folgt_der_feldbedeutung() {
        let pfad = wegwerf("claude.jsonl", CLAUDE);
        let s = claude_datei(&pfad).expect("lesbar");

        assert_eq!(s.werkzeug, "claude");
        assert_eq!(s.kennung, "abc-123");
        // `ein` ist frische Eingabe PLUS Cache-Schreiben — wie bei den Channels.
        assert_eq!(s.ein, 2 + 500 + 3 + 100);
        assert_eq!(s.aus, 196 + 50);
        assert_eq!(s.cache, 30000 + 40000);
        assert_eq!(s.ein_frisch, Some(5));
        assert_eq!(s.cache_schreiben, Some(600));
        assert_eq!(s.modell.as_deref(), Some("claude-opus-5"));
        assert_eq!(s.verzeichnis.as_deref(), Some("/home/x/Development/ai-brain"));
    }

    #[test]
    fn das_haeufigste_arbeitsverzeichnis_gewinnt_nicht_das_erste() {
        // Der gemessene Fall: erst die Heimat, dann die eigentliche Arbeit
        // woanders. Wer die erste Nennung nimmt, ordnet die Sitzung dem
        // falschen Projekt zu — oder gar keinem.
        let inhalt = concat!(
            r#"{"sessionId":"wandert","cwd":"/home/x","type":"assistant","message":{"usage":{"output_tokens":1}}}"#, "\n",
            r#"{"sessionId":"wandert","cwd":"/home/x/Development/ai-brain","type":"assistant","message":{"usage":{"output_tokens":1}}}"#, "\n",
            r#"{"sessionId":"wandert","cwd":"/home/x/Development/ai-brain","type":"assistant","message":{"usage":{"output_tokens":1}}}"#, "\n",
        );

        let pfad = wegwerf("claude-wandert.jsonl", inhalt);
        let s = claude_datei(&pfad).expect("lesbar");

        assert_eq!(s.verzeichnis.as_deref(), Some("/home/x/Development/ai-brain"));
    }

    #[test]
    fn claude_summiert_die_zugdauer_aus_eigenen_saetzen() {
        let pfad = wegwerf("claude-dauer.jsonl", CLAUDE);
        let s = claude_datei(&pfad).expect("lesbar");

        // Die Dauer steht NICHT am usage, sondern in `subtype=turn_duration`.
        assert_eq!(s.dauer_ms, Some(145908 + 54092));
    }

    #[test]
    fn claude_ohne_verbrauch_ergibt_keine_sitzung() {
        let pfad = wegwerf(
            "claude-leer.jsonl",
            "{\"sessionId\":\"leer\",\"type\":\"user\"}\n",
        );

        assert!(claude_datei(&pfad).is_none());
    }

    #[test]
    fn claude_nimmt_den_dateinamen_als_kennung_wenn_keine_dasteht() {
        let pfad = wegwerf(
            "0f9f4642-aus-dem-namen.jsonl",
            "{\"type\":\"assistant\",\"message\":{\"usage\":{\"output_tokens\":5}}}\n",
        );

        let s = claude_datei(&pfad).expect("lesbar");
        assert_eq!(s.kennung, "0f9f4642-aus-dem-namen");
    }

    #[test]
    fn kaputte_zeilen_werfen_die_datei_nicht_weg() {
        let mut inhalt = String::from("das ist kein JSON\n");
        inhalt.push_str(CLAUDE);

        let pfad = wegwerf("claude-kaputt.jsonl", &inhalt);
        let s = claude_datei(&pfad).expect("trotz kaputter Zeile lesbar");

        assert_eq!(s.aus, 246);
    }

    // -- Codex ---------------------------------------------------------------

    /// Feldnamen aus einer echten Codex-Sitzung (15.10.2026). `turn.completed`
    /// gibt es nicht — der Satz heisst `token_usage_record`.
    const CODEX: &str = r#"{"type":"session_meta","payload":{"id":"01a1-codex","cwd":"/home/x/Development/ai-brain-harness","model_provider":"openai"}}
{"type":"token_usage_record","payload":{"session_id":"01a1-codex","usage":{"input_tokens":16051,"cached_input_tokens":12160,"cache_write_input_tokens":0,"output_tokens":5},"turn_token_usage":{"input_tokens":16051,"cached_input_tokens":12160,"cache_write_input_tokens":0,"output_tokens":5}}}
"#;

    #[test]
    fn codex_rechnet_die_gecachten_aus_der_eingabe_heraus() {
        let pfad = wegwerf("codex.jsonl", CODEX);
        let s = codex_datei(&pfad).expect("lesbar");

        assert_eq!(s.werkzeug, "codex");
        assert_eq!(s.kennung, "01a1-codex");
        // `input_tokens` ENTHAELT bei Codex die gecachten — anders als bei
        // Claude. Ohne das Herausrechnen zaehlte der Cache doppelt.
        assert_eq!(s.ein_frisch, Some(16051 - 12160));
        assert_eq!(s.cache, 12160);
        assert_eq!(s.aus, 5);
        assert_eq!(s.verzeichnis.as_deref(), Some("/home/x/Development/ai-brain-harness"));
    }

    #[test]
    fn codex_nimmt_das_groessere_von_summe_und_letztem_stand() {
        // Zwei Zuege. Ist `usage` kumulativ, steht dort am Ende 300; ist es ein
        // Zugwert, ist die Summe der Zuege 300. Beide Lesarten muessen auf
        // dieselbe Zahl kommen — genau dafuer ist das Maximum da.
        let inhalt = concat!(
            r#"{"type":"session_meta","payload":{"id":"zwei-zuege","cwd":"/tmp"}}"#, "\n",
            r#"{"type":"token_usage_record","payload":{"usage":{"input_tokens":100,"output_tokens":10},"turn_token_usage":{"input_tokens":100,"output_tokens":10}}}"#, "\n",
            r#"{"type":"token_usage_record","payload":{"usage":{"input_tokens":300,"output_tokens":30},"turn_token_usage":{"input_tokens":200,"output_tokens":20}}}"#, "\n",
        );

        let pfad = wegwerf("codex-zwei.jsonl", inhalt);
        let s = codex_datei(&pfad).expect("lesbar");

        assert_eq!(s.ein, 300, "Summe der Zuege und letzter Stand sind beide 300");
        assert_eq!(s.aus, 30);
    }

    #[test]
    fn codex_ohne_kennung_wird_verworfen() {
        let pfad = wegwerf(
            "codex-ohne.jsonl",
            "{\"type\":\"token_usage_record\",\"payload\":{\"usage\":{\"output_tokens\":5}}}\n",
        );

        // Ohne Kennung kann der Server keinen Zuwachs bilden — eine Meldung
        // waere bei jedem Lauf eine neue Zeile.
        assert!(codex_datei(&pfad).is_none());
    }

    // -- Abgleichstand -------------------------------------------------------

    fn sitzung(kennung: &str, ein: u64, aus: u64) -> Sitzung {
        Sitzung {
            werkzeug: "claude",
            kennung: kennung.to_string(),
            ein,
            aus,
            cache: 0,
            ein_frisch: None,
            cache_schreiben: None,
            modell: None,
            verzeichnis: None,
            dauer_ms: None,
        }
    }

    #[test]
    fn was_schon_gemeldet_ist_bleibt_liegen() {
        let a = sitzung("s1", 100, 10);
        let mut stand = BTreeMap::new();
        stand.insert(a.schluessel(), a.fingerabdruck());

        assert!(offene(&[a], &stand).is_empty());
    }

    #[test]
    fn eine_gewachsene_sitzung_wird_wieder_faellig() {
        let alt = sitzung("s1", 100, 10);
        let mut stand = BTreeMap::new();
        stand.insert(alt.schluessel(), alt.fingerabdruck());

        let neu = sitzung("s1", 150, 20);

        assert_eq!(offene(&[neu], &stand).len(), 1);
    }

    #[test]
    fn ein_neuer_modellname_allein_loest_keine_meldung_aus() {
        let mut alt = sitzung("s1", 100, 10);
        let mut stand = BTreeMap::new();
        stand.insert(alt.schluessel(), alt.fingerabdruck());

        alt.modell = Some("ein-anderes-modell".to_string());

        // Der Fingerabdruck fuehrt nur Zaehler. Sonst meldete jeder Lauf
        // dieselbe Sitzung erneut, sobald irgendein Begleitfeld wechselt.
        assert!(offene(&[alt], &stand).is_empty());
    }

    #[test]
    fn gleiche_kennung_bei_verschiedenen_werkzeugen_ist_nicht_dasselbe() {
        let claude = sitzung("gleich", 100, 10);
        let mut codex = sitzung("gleich", 100, 10);
        codex.werkzeug = "codex";

        assert_ne!(claude.schluessel(), codex.schluessel());
    }

    // -- Nutzlast ------------------------------------------------------------

    #[test]
    fn leere_felder_stehen_nicht_in_der_nutzlast() {
        let n = nutzlast(&sitzung("s1", 100, 10), None);

        // Ein `null` im Aufruf waere etwas anderes als „weggelassen": das
        // Werkzeug prueft auf Vorhandensein.
        assert!(n.get("modell").is_none());
        assert!(n.get("konto").is_none());
        assert!(n.get("dauer_ms").is_none());
        assert_eq!(n["ein"], 100);
    }

    #[test]
    fn eine_dauer_von_null_wird_weggelassen() {
        let mut s = sitzung("s1", 100, 10);
        s.dauer_ms = Some(0);

        assert!(nutzlast(&s, None).get("dauer_ms").is_none());
    }

    #[test]
    fn ohne_benannte_sitzung_bleibt_das_konto_leer() {
        // Der Fehler vom 16.10.2026: ein nachholender Lauf stempelte das
        // AKTUELLE Konto auf 2066 historische Sitzungen aus verschiedenen
        // Konten. Eine Luecke ist besser als eine Zahl, die wie eine Messung
        // aussieht.
        std::env::set_var("CLAUDE_CONFIG_DIR", "/home/x/.local/state/imkr/accounts/claude/henneberg");

        assert_eq!(konto(&[], None), None, "nachholend: kein Konto");
        assert_eq!(
            konto(&[], Some("eine-sitzung")).as_deref(),
            Some("henneberg"),
            "benannte Sitzung: das Konto dieses Laufs"
        );

        std::env::remove_var("CLAUDE_CONFIG_DIR");
    }

    #[test]
    fn ein_genanntes_konto_gilt_auch_nachholend() {
        let args = vec!["--konto".to_string(), "department".to_string()];

        assert_eq!(konto(&args, None).as_deref(), Some("department"));
    }

    #[test]
    fn das_konto_kommt_von_aussen_in_die_nutzlast() {
        // Aus den Protokollen ist es nicht ableitbar — nur der Hook kennt es.
        let n = nutzlast(&sitzung("s1", 100, 10), Some("henneberg"));

        assert_eq!(n["konto"], "henneberg");
    }

    // -- Kleinkram -----------------------------------------------------------

    #[test]
    fn kuerzen_schneidet_nicht_mitten_in_ein_zeichen() {
        assert_eq!(kurz(&"ä".repeat(50), 10).chars().count(), 10);
    }

    #[test]
    fn kurzes_bleibt_unangetastet() {
        assert_eq!(kurz("abc", 10), "abc");
    }
}
