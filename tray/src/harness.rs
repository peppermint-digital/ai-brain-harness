//! Den Harness finden und fragen.
//!
//! Der Tray entscheidet nichts: Höchstdauer, Ablauf und Auflegen stehen im
//! Harness, und der Tray ruft sie auf. Damit gibt es jede Regel genau einmal.
//!
//! ## Warum das Suchen mehr als `PATH` ist (Stand 27.10.2026)
//!
//! Der Tray startet beim Anmelden — aus einem `.desktop`-Eintrag, einem
//! LaunchAgent oder dem Windows-Run-Key. Alle drei starten ihn **ohne**
//! die Shell des Nutzers: `~/.local/bin` steht dort oft nicht im `PATH`,
//! obwohl es im Terminal da ist. Der Tray wäre dann in der Leiste und fände
//! sein eigenes Programm nicht — und zu sehen wäre „Harness antwortet
//! nicht", also ein Fehler, der nach einem kaputten Harness aussieht.
//!
//! Deshalb wird neben dem eigenen Programm zuerst gesucht: Installiert
//! werden beide ins selbe Verzeichnis.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ansicht::{Status, Wissen};

#[cfg(windows)]
pub const BIN_NAME: &str = "ai-brain-harness.exe";
#[cfg(not(windows))]
pub const BIN_NAME: &str = "ai-brain-harness";

/// Wo nachgesehen wird, in dieser Reihenfolge.
///
/// Rein, damit die Reihenfolge ohne Dateisystem prüfbar ist.
pub fn kandidaten(
    gesetzt: Option<&str>,
    eigenes: Option<&Path>,
    heim: Option<&Path>,
) -> Vec<PathBuf> {
    let mut wege: Vec<PathBuf> = Vec::new();

    // Ausdrücklich gesetzt gewinnt immer — das ist der Ausweg für jeden Fall,
    // den die Suche unten nicht trifft.
    if let Some(p) = gesetzt.map(str::trim).filter(|p| !p.is_empty()) {
        wege.push(PathBuf::from(p));
    }

    if let Some(ordner) = eigenes.and_then(Path::parent) {
        wege.push(ordner.join(BIN_NAME));
    }

    if let Some(h) = heim {
        wege.push(h.join(".local").join("bin").join(BIN_NAME));

        if cfg!(windows) {
            wege.push(
                h.join("AppData")
                    .join("Local")
                    .join("Programs")
                    .join("ai-brain-harness")
                    .join(BIN_NAME),
            );
        }
    }

    // Zuletzt der blosse Name: Den löst das Betriebssystem über den PATH auf.
    wege.push(PathBuf::from(BIN_NAME));

    wege
}

fn gefunden() -> PathBuf {
    let gesetzt = std::env::var("AI_BRAIN_HARNESS").ok();
    let eigenes = std::env::current_exe().ok();
    let heim = heimat();

    let wege = kandidaten(gesetzt.as_deref(), eigenes.as_deref(), heim.as_deref());

    // Der letzte Weg ist der blosse Name und existiert als Datei nicht —
    // deshalb wird er genommen, wenn nichts anderes passt, und nicht geprüft.
    let letzter = wege.len() - 1;

    for (i, weg) in wege.iter().enumerate() {
        if i == letzter || weg.is_file() {
            return weg.clone();
        }
    }

    PathBuf::from(BIN_NAME)
}

fn heimat() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Ruft den Harness auf und gibt seine Ausgabe zurück.
pub fn rufen(args: &[String]) -> Result<String, String> {
    let pfad = gefunden();

    let ausgabe = Command::new(&pfad)
        .args(args)
        .output()
        .map_err(|e| format!("{} nicht startbar: {e}", pfad.display()))?;

    if ausgabe.status.success() {
        return Ok(String::from_utf8_lossy(&ausgabe.stdout).into_owned());
    }

    let text = String::from_utf8_lossy(&ausgabe.stderr);

    Err(text
        .lines()
        .last()
        .unwrap_or("unbekannter Fehler")
        .trim()
        .to_string())
}

/// Den Zustand lesen.
pub fn abfragen() -> Wissen {
    match rufen(&["leitung".into(), "status".into(), "--json".into()]) {
        Ok(text) => match serde_json::from_str::<Status>(text.trim()) {
            Ok(s) => Wissen::Da(s),
            Err(_) => Wissen::Fehler("Harness zu alt: kennt `leitung status --json` nicht".into()),
        },
        Err(grund) => Wissen::Fehler(grund),
    }
}

/// Eine Datei oder einen Ordner im Dateimanager bzw. Editor öffnen.
///
/// Die Plattform-Weiche stand bis zum 27.10.2026 hier. Sie liegt jetzt in
/// `ai-brain-plattform`, weil der Harness für die Anmeldung (#8949) dieselbe
/// braucht — und eine zweite Kopie wäre genau der Fall, gegen den es das
/// Crate gibt. Hier bleibt, dass dem Tray ein fehlgeschlagener Aufruf nichts
/// tut: Ein Protokoll, das sich nicht öffnet, ist ärgerlich und nicht mehr.
pub fn oeffnen(pfad: &str) {
    let _ = ai_brain_plattform::oeffnen::jetzt(pfad);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gesetzter_pfad_steht_vorn() {
        let wege = kandidaten(
            Some("/opt/brain/ai-brain-harness"),
            Some(Path::new("/home/x/.local/bin/ai-brain-tray")),
            Some(Path::new("/home/x")),
        );

        assert_eq!(wege[0], PathBuf::from("/opt/brain/ai-brain-harness"));
    }

    /// Der eigentliche Fix: Das Nachbarprogramm kommt VOR dem PATH.
    #[test]
    fn neben_dem_eigenen_programm_vor_dem_pfad() {
        let wege = kandidaten(
            None,
            Some(Path::new("/home/x/.local/bin/ai-brain-tray")),
            Some(Path::new("/home/x")),
        );

        assert_eq!(wege[0], PathBuf::from("/home/x/.local/bin").join(BIN_NAME));
        assert_eq!(wege.last().unwrap(), &PathBuf::from(BIN_NAME));
    }

    #[test]
    fn leerer_umgebungswert_zaehlt_nicht() {
        let wege = kandidaten(Some("  "), None, None);
        assert_eq!(wege, vec![PathBuf::from(BIN_NAME)]);
    }

    #[test]
    fn ohne_alles_bleibt_der_blosse_name() {
        assert_eq!(kandidaten(None, None, None), vec![PathBuf::from(BIN_NAME)]);
    }
}
