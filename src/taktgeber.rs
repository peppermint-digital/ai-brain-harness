//! Der Abholer als Taktgeber (#7814).
//!
//! **Ohne Abholer ist ein Dauerauftrag dasselbe wie ein pausierter Wächter.**
//! AI Brain kennt Takt und Frist, aber niemand holt ab — und das sieht genauso
//! aus wie Ruhe. Genau dieser Fall liess Hermes' Backup-Wächter dreieinhalb
//! Monate untätig in der Liste stehen.
//!
//! ## Nutzer-Units, kein Systemdienst
//!
//! Der Abholer arbeitet mit den Rechten des angemeldeten Menschen und holt
//! seine Aufträge mit dessen Zugang. Ein Systemdienst müsste als `root` laufen
//! und sich die Identität borgen — mehr Rechte für weniger Klarheit. Nutzer-Units
//! brauchen kein `sudo`, und der Rechner hier hat `Linger` schon an (sonst
//! liefen sie nur, solange jemand angemeldet ist; davor wird gewarnt).
//!
//! ## Dass der Taktgeber läuft, prüft die Gegenseite
//!
//! Dieses Modul richtet ihn ein, überwacht sich aber nicht selbst — das wäre
//! derselbe Zirkel wie bei Hermes. Bleibt der Abholer stehen, kommen keine
//! Meldungen, und die Stillfrist in AI Brain schlägt Alarm. Der Wächter über
//! den Wächter sitzt also auf der anderen Seite.

use std::path::PathBuf;
use std::process::Command;

const DIENST: &str = "ai-brain-daueraufgaben.service";
const TIMER: &str = "ai-brain-daueraufgaben.timer";

/// Der zweite Taktgeber: Auftraege aus der AI-Brain-Oberflaeche.
///
/// **Warum zwei.** Dauerauftraege sind wiederkehrend und laufen meist ohne
/// Modell; Dispatches sind einmalig und tragen eine Anweisung FUER ein Modell.
/// Verschiedene Takte, verschiedene Kosten, verschiedene Grenzen — in einer
/// Unit zusammengelegt waere die eine nicht ohne die andere abschaltbar.
const AUFTRAG_DIENST: &str = "ai-brain-auftraege.service";
const AUFTRAG_TIMER: &str = "ai-brain-auftraege.timer";

fn unit_verzeichnis() -> Result<PathBuf, String> {
    let heimat = std::env::var("HOME").map_err(|_| "HOME ist nicht gesetzt.".to_string())?;

    Ok(PathBuf::from(heimat).join(".config/systemd/user"))
}

/// Die Unit-Dateien, wie sie geschrieben werden.
///
/// `Persistent=true` ist wichtig: War der Rechner aus, soll der Abholer nach
/// dem Hochfahren nachholen statt den Takt zu überspringen. Ohne das fiele ein
/// Lauf aus, ohne dass es auffällt — und das ist genau die Sorte Lücke, gegen
/// die Dauerauftraege gebaut sind.
pub fn units(programm: &str, takt: &str) -> (String, String) {
    let dienst = format!(
        "[Unit]\n\
         Description=AI Brain Harness — Dauerauftraege abholen und fahren\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={programm} daueraufgaben --fahren --still\n"
    );

    let timer = format!(
        "[Unit]\n\
         Description=AI Brain Harness — Taktgeber fuer Dauerauftraege\n\
         \n\
         [Timer]\n\
         OnCalendar={takt}\n\
         Persistent=true\n\
         RandomizedDelaySec=30\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n"
    );

    (dienst, timer)
}

/// Die Units fuer den Auftrags-Abholer.
pub fn auftrag_units(programm: &str, takt: &str) -> (String, String) {
    let dienst = format!(
        "[Unit]\n\
         Description=AI Brain Harness — Auftraege aus AI Brain abholen und fahren\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={programm} auftraege --fahren --still\n"
    );

    let timer = format!(
        "[Unit]\n\
         Description=AI Brain Harness — Taktgeber fuer Auftraege\n\
         \n\
         [Timer]\n\
         OnCalendar={takt}\n\
         Persistent=true\n\
         RandomizedDelaySec=30\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n"
    );

    (dienst, timer)
}

/// `taktgeber [--einrichten|--entfernen|--zustand]`
pub fn verwalten(args: &[String]) -> Result<(), String> {
    let hat = |n: &str| args.iter().any(|a| a == n);

    if hat("--entfernen") {
        return entfernen();
    }

    if hat("--einrichten") {
        let takt = args
            .iter()
            .position(|a| a == "--takt")
            .and_then(|i| args.get(i + 1))
            .filter(|w| !w.starts_with("--"))
            .cloned()
            // Alle fünf Minuten: feiner als jeder sinnvolle Takt eines
            // Dauerauftrags, und ein Lauf ohne Fälliges kostet eine Abfrage.
            .unwrap_or_else(|| "*-*-* *:00/5:00".to_string());

        return einrichten(&takt);
    }

    zustand()
}

fn einrichten(takt: &str) -> Result<(), String> {
    let verzeichnis = unit_verzeichnis()?;

    std::fs::create_dir_all(&verzeichnis)
        .map_err(|e| format!("{} lässt sich nicht anlegen: {e}", verzeichnis.display()))?;

    let programm = std::env::current_exe()
        .map_err(|e| format!("Eigener Pfad nicht ermittelbar: {e}"))?
        .display()
        .to_string();

    let (dienst, timer) = units(&programm, takt);

    std::fs::write(verzeichnis.join(DIENST), &dienst)
        .map_err(|e| format!("{DIENST} nicht schreibbar: {e}"))?;
    std::fs::write(verzeichnis.join(TIMER), &timer)
        .map_err(|e| format!("{TIMER} nicht schreibbar: {e}"))?;

    let (a_dienst, a_timer) = auftrag_units(&programm, takt);

    std::fs::write(verzeichnis.join(AUFTRAG_DIENST), &a_dienst)
        .map_err(|e| format!("{AUFTRAG_DIENST} nicht schreibbar: {e}"))?;
    std::fs::write(verzeichnis.join(AUFTRAG_TIMER), &a_timer)
        .map_err(|e| format!("{AUFTRAG_TIMER} nicht schreibbar: {e}"))?;

    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", TIMER])?;
    systemctl(&["enable", "--now", AUFTRAG_TIMER])?;

    println!("Taktgeber eingerichtet.");
    println!();
    println!("  Dauerauftraege  {}", verzeichnis.join(DIENST).display());
    println!("  Auftraege       {}", verzeichnis.join(AUFTRAG_DIENST).display());
    println!("  Takt            {takt}");
    println!();
    println!("  Bis jetzt wartete ein Auftrag aus der Oberflaeche UNBEGRENZT —");
    println!("  bis jemand `auftraege` tippte. Jetzt hoechstens einen Takt.");
    println!();

    // Ohne Lingering läuft der Timer nur, solange jemand angemeldet ist — und
    // ein Abholer, der mit der Abmeldung stehenbleibt, ist genau die stille
    // Lücke, gegen die Dauerauftraege gebaut sind.
    match lingering() {
        Some(true) => println!("  Lingering ist an — der Taktgeber läuft auch ohne angemeldete Sitzung."),
        Some(false) => {
            println!("  \x1b[33mLingering ist AUS.\x1b[0m Der Taktgeber läuft dann nur, solange");
            println!("  jemand angemeldet ist. Einschalten:");
            println!("    sudo loginctl enable-linger {}", std::env::var("USER").unwrap_or_default());
        }
        None => println!("  Lingering liess sich nicht feststellen — bitte nachsehen."),
    }

    println!();
    println!("Zustand ansehen:  ai-brain-harness taktgeber");

    Ok(())
}

fn entfernen() -> Result<(), String> {
    let verzeichnis = unit_verzeichnis()?;

    // Erst abschalten, dann löschen. Andersherum bleibt eine Unit aktiviert,
    // deren Datei fehlt, und systemd meckert bei jedem `daemon-reload`.
    let _ = systemctl(&["disable", "--now", TIMER]);
    let _ = systemctl(&["disable", "--now", AUFTRAG_TIMER]);

    for datei in [DIENST, TIMER, AUFTRAG_DIENST, AUFTRAG_TIMER] {
        let pfad = verzeichnis.join(datei);

        if pfad.exists() {
            std::fs::remove_file(&pfad)
                .map_err(|e| format!("{} nicht löschbar: {e}", pfad.display()))?;
        }
    }

    systemctl(&["daemon-reload"])?;

    println!("Taktgeber entfernt.");
    println!();
    println!("Achtung: Ohne Abholer melden die Daueraufträge nicht mehr. AI Brain");
    println!("schlägt nach ihrer Stillfrist Alarm — das ist gewollt, aber es kommt.");
    println!("Wer sie wirklich nicht mehr braucht, schaltet sie dort ab.");

    Ok(())
}

fn zustand() -> Result<(), String> {
    let verzeichnis = unit_verzeichnis()?;

    if !verzeichnis.join(TIMER).exists() {
        println!("Kein Taktgeber eingerichtet.");
        println!();
        println!("Ohne ihn holt niemand die Daueraufträge dieses Rechners ab — und");
        println!("das sieht aus wie Ruhe, bis die Stillfrist zuschlägt.");
        println!();
        println!("Einrichten:  ai-brain-harness taktgeber --einrichten");

        return Ok(());
    }

    let aktiv = ausgabe(&["is-active", TIMER]).unwrap_or_else(|| "unbekannt".into());

    println!("Taktgeber: {aktiv}");
    println!();

    if let Some(naechster) = ausgabe(&["list-timers", "--no-legend", "--no-pager", TIMER]) {
        if !naechster.trim().is_empty() {
            println!("  {}", naechster.trim());
            println!();
        }
    }

    // **`ExecMainExitTimestamp`, nicht `ActiveEnterTimestamp`.** Bei
    // `Type=oneshot` ist letzterer nach dem Beenden LEER — die Zeile faellt
    // dann aus und alle folgenden Werte rutschen eine Stelle nach vorne. Hier
    // stand deshalb „Letzter Lauf 0, Ergebnis (leer)", obwohl der Lauf fuenf
    // Sekunden zuvor erfolgreich war. Derselbe Fehler, den die Restic-Pruefung
    // am selben Tag hatte; siehe Wiki „systemd oneshot:
    // ActiveEnterTimestamp ist leer, nachdem der Lauf durch ist".
    //
    // Einzeln abfragen statt in einer Liste: dann kann keine fehlende Zeile
    // die Zuordnung verschieben.
    let feld = |name: &str| -> String {
        ausgabe(&["show", DIENST, "-p", name, "--value"])
            .map(|w| w.trim().to_string())
            .filter(|w| !w.is_empty() && w != "n/a")
            .unwrap_or_else(|| "—".to_string())
    };

    println!("  Letzter Lauf   {}", feld("ExecMainExitTimestamp"));
    println!("  Ergebnis       {}", feld("Result"));
    println!("  Rückgabewert   {}", feld("ExecMainStatus"));

    if lingering() == Some(false) {
        println!();
        println!("  \x1b[33mLingering ist AUS\x1b[0m — der Taktgeber steht, sobald niemand angemeldet ist.");
    }

    Ok(())
}

/// Was `status` ueber den Taktgeber in seine Antwort nimmt (#9059).
#[derive(serde::Serialize)]
pub struct TaktgeberKurz {
    /// Liegen die Unit-Dateien?
    pub eingerichtet: bool,
    /// Was `systemctl --user is-active` sagt — `active`, `inactive`, oder
    /// `None`, wenn systemd nicht antwortet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aktiv: Option<String>,
}

/// Der Stand beider Taktgeber in einem Wort.
///
/// Gefragt wird nur der Dauerauftrags-Timer. Beide einzeln wären zwei
/// `systemctl`-Aufrufe in einer Anzeige, die alle 30 Sekunden gelesen wird —
/// und sie werden vom Einrichter immer gemeinsam gesetzt. Wer es getrennt
/// braucht, ruft `taktgeber`.
pub fn kurzstand() -> TaktgeberKurz {
    let eingerichtet = unit_verzeichnis()
        .map(|v| v.join(TIMER).exists())
        .unwrap_or(false);

    if !eingerichtet {
        return TaktgeberKurz {
            eingerichtet: false,
            aktiv: None,
        };
    }

    TaktgeberKurz {
        eingerichtet: true,
        aktiv: ausgabe(&["is-active", TIMER])
            .map(|w| w.trim().to_string())
            .filter(|w| !w.is_empty()),
    }
}

fn lingering() -> Option<bool> {
    let nutzer = std::env::var("USER").ok()?;

    let aus = Command::new("loginctl")
        .args(["show-user", &nutzer, "-p", "Linger", "--value"])
        .output()
        .ok()?;

    if !aus.status.success() {
        return None;
    }

    match String::from_utf8_lossy(&aus.stdout).trim() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

fn systemctl(args: &[&str]) -> Result<(), String> {
    let mut alle = vec!["--user"];
    alle.extend_from_slice(args);

    let aus = Command::new("systemctl")
        .args(&alle)
        .output()
        .map_err(|e| format!("systemctl liess sich nicht starten: {e}"))?;

    if aus.status.success() {
        return Ok(());
    }

    Err(format!(
        "`systemctl {}` scheiterte: {}",
        alle.join(" "),
        String::from_utf8_lossy(&aus.stderr).trim()
    ))
}

fn ausgabe(args: &[&str]) -> Option<String> {
    let mut alle = vec!["--user"];
    alle.extend_from_slice(args);

    let aus = Command::new("systemctl").args(&alle).output().ok()?;

    Some(String::from_utf8_lossy(&aus.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_auftrags_dienst_ruft_den_richtigen_befehl() {
        let (dienst, _) = auftrag_units("/usr/bin/ai-brain-harness", "*-*-* *:00/5:00");

        assert!(dienst.contains("auftraege --fahren --still"), "{dienst}");
        assert!(dienst.contains("Type=oneshot"));
        // Scharf: NICHT der Dauerauftrags-Befehl.
        assert!(!dienst.contains("daueraufgaben"), "{dienst}");
    }

    #[test]
    fn beide_taktgeber_sind_getrennte_einheiten() {
        // In einer Unit zusammengelegt waere die eine nicht ohne die andere
        // abschaltbar — und sie haben verschiedene Kosten.
        assert_ne!(DIENST, AUFTRAG_DIENST);
        assert_ne!(TIMER, AUFTRAG_TIMER);

        let (d1, _) = units("/x", "*-*-* *:00/5:00");
        let (d2, _) = auftrag_units("/x", "*-*-* *:00/5:00");

        assert_ne!(d1, d2);
    }

    #[test]
    fn der_auftrags_timer_holt_versaeumte_laeufe_nach() {
        let (_, timer) = auftrag_units("/x", "*-*-* *:00/5:00");

        assert!(timer.contains("Persistent=true"));
        assert!(timer.contains("WantedBy=timers.target"));
    }

    #[test]
    fn der_dienst_ruft_den_abholer_still() {
        let (dienst, _) = units("/usr/bin/ai-brain-harness", "*-*-* *:00/5:00");

        assert!(dienst.contains("daueraufgaben --fahren --still"));
        assert!(dienst.contains("Type=oneshot"));
    }

    #[test]
    fn der_timer_holt_versaeumte_laeufe_nach() {
        // Ohne `Persistent=true` faellt der Lauf nach einem Neustart aus —
        // und ein ausgefallener Lauf ist genau die stille Luecke.
        let (_, timer) = units("/x", "*-*-* *:00/5:00");

        assert!(timer.contains("Persistent=true"));
        assert!(timer.contains("OnCalendar=*-*-* *:00/5:00"));
        assert!(timer.contains("WantedBy=timers.target"));
    }

    #[test]
    fn der_takt_landet_im_timer_und_nicht_im_dienst() {
        let (dienst, timer) = units("/x", "*-*-* 04:00:00");

        assert!(timer.contains("04:00:00"));
        assert!(!dienst.contains("04:00:00"));
    }
}
