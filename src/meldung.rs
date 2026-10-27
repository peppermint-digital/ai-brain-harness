//! imkr-Meldungen im gemeinsamen Posteingang (imkr-Idee #536).
//!
//! **Was hier durchgeht.** Wartet ein Agent auf eine Antwort, ist er fertig,
//! abgebrochen, ist sein Konto erschöpft oder hat ein Kontakt geschrieben,
//! meldet imkr das hierher, und die Zeile steht unter der Art `imkr` im
//! Posteingang von AI Brain — zusätzlich zu Klingel und ntfy, nicht statt.
//!
//! **Warum über die Harness und nicht direkt.** Wie bei den Nachrichten
//! zwischen imkrs (#495): imkr hält keine Anmeldung bei AI Brain, die Harness
//! tut es. imkr ruft deshalb dieses Programm auf und kennt nur diesen Vertrag.
//!
//! **Der Rückweg ist das eigentlich Neue.** imkr führt sein eigenes ✦ („da
//! war was, du hast es noch nicht gesehen"). Hakt jemand die Zeile im
//! Posteingang ab, ist sie gesehen — ein ✦, das danach noch im Terminal
//! steht, wäre eine zweite Wahrheit. `gelesen` holt deshalb, was abgehakt
//! wurde, und `bestaetigen` schliesst es ab.
//!
//! **Mindestens einmal, nicht genau einmal.** `gelesen` gibt eine Zeile
//! heraus, ohne sie abzuschliessen. Stirbt imkr dazwischen, kommt sie
//! wieder — ein ✦ zweimal wegzunehmen kostet nichts, es einmal stehen zu
//! lassen schon.
//!
//! **`gelesen --json` ist ein Maschinenvertrag.** Die Antwort wird von imkr
//! geparst und deshalb **unverändert durchgereicht**, nicht hier neu
//! zusammengesetzt: Ein zweiter Ort, der dieselbe Form beschreibt, ist ein
//! zweiter Ort, der von ihr abweichen kann. Dieselbe Entscheidung wie in
//! [`crate::nachricht`].

use std::io::Read;

use serde::Deserialize;

use crate::config;
use crate::mcp;
use crate::nachricht::flagge;

/// Die Arten, die AI Brain annimmt. Hier nur, um früh und verständlich
/// abzulehnen — die verbindliche Liste steht dort.
const ARTEN: [&str; 5] = ["blocked", "done", "failed", "account", "message"];

#[derive(Debug, Deserialize)]
struct Gemeldet {
    id: u64,
}

#[derive(Debug, Deserialize)]
struct Gelesen {
    id: u64,
    #[serde(default)]
    sitzung: Option<String>,
    #[serde(default)]
    pane: Option<u64>,
    #[serde(default)]
    art: Option<String>,
    #[serde(default)]
    abholungen: u64,
}

#[derive(Debug, Deserialize)]
struct Rueckweg {
    #[serde(default)]
    gelesen: Vec<Gelesen>,
}

/// `meldung <senden|gelesen|bestaetigen> …`
pub fn fuehren(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("senden") => senden(&args[1..]),
        Some("gelesen") => gelesen(&args[1..]),
        Some("bestaetigen") => bestaetigen(&args[1..]),
        Some(anderes) => Err(format!(
            "`{anderes}` kenne ich nicht. Möglich: senden, gelesen, bestaetigen."
        )),
        None => Err("`ai-brain-harness meldung <senden|gelesen|bestaetigen>`".to_string()),
    }
}

/// Alle Flaggen auf einmal herausziehen; zurück bleibt der Titel.
///
/// Eine Schleife und keine neun Einzelaufrufe mit Hand-gepflegter Liste
/// belegter Stellen: Genau dort landete sonst irgendwann „--agent" im Titel,
/// weil jemand eine Flagge ergänzt und das Mitzählen vergisst.
fn flaggen<'a>(args: &[String], namen: &[&'a str]) -> (Vec<(&'a str, String)>, String) {
    let mut gefunden = Vec::new();
    let mut belegt: Vec<usize> = Vec::new();

    for name in namen {
        if let Some((wert, pos)) = flagge(args, name) {
            gefunden.push((*name, wert));
            belegt.push(pos);
            belegt.push(pos + 1);
        }
    }

    let titel = args
        .iter()
        .enumerate()
        .filter(|(i, _)| !belegt.contains(i))
        .map(|(_, a)| a.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    (gefunden, titel)
}

fn wert<'a>(gefunden: &'a [(&str, String)], name: &str) -> Option<&'a str> {
    gefunden
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, w)| w.as_str())
}

fn zahl(gefunden: &[(&str, String)], name: &str) -> Result<Option<u64>, String> {
    match wert(gefunden, name) {
        None => Ok(None),
        Some(roh) => roh
            .parse()
            .map(Some)
            .map_err(|_| format!("`{name} {roh}` ist keine Zahl.")),
    }
}

fn senden(args: &[String]) -> Result<(), String> {
    let (gefunden, titel) = flaggen(
        args,
        &[
            "--art",
            "--rechner",
            "--sitzung",
            "--pane",
            "--fenster",
            "--agent",
            "--link",
        ],
    );

    let art = wert(&gefunden, "--art")
        .ok_or_else(|| format!("`--art` fehlt. Möglich: {}.", ARTEN.join(", ")))?;

    if !ARTEN.contains(&art) {
        return Err(format!(
            "`{art}` ist keine Meldungsart. Möglich: {}.",
            ARTEN.join(", ")
        ));
    }

    // Rechner, Sitzung und Pane sind die Rückadresse. Ohne sie findet das
    // Abhaken nicht zurück, und das ✦ bliebe im Terminal stehen — eine Zeile,
    // deren Haken nichts tut, ist schlimmer als keine Zeile.
    let rechner = wert(&gefunden, "--rechner").ok_or("`--rechner` fehlt.")?;
    let sitzung = wert(&gefunden, "--sitzung").ok_or("`--sitzung` fehlt.")?;
    let pane = zahl(&gefunden, "--pane")?.ok_or("`--pane` fehlt.")?;

    if titel.trim().is_empty() {
        return Err(
            "Ohne Titel steht im Posteingang eine leere Zeile. `meldung senden --art … <titel>`"
                .to_string(),
        );
    }

    // Der Begleittext kommt auf stdin: Er ist der Boden eines
    // Agentenbildschirms, also mehrzeilig und voll mit allem, was ein
    // Terminal so ausgibt. Auf der Befehlszeile wäre das ein Zitierproblem
    // ohne Gewinn.
    let mut text = String::new();
    let _ = std::io::stdin().read_to_string(&mut text);

    let mut nutzlast = serde_json::json!({
        "art": art,
        "rechner": rechner,
        "sitzung": sitzung,
        "pane": pane,
        "titel": titel.trim(),
    });

    if let Some(fenster) = zahl(&gefunden, "--fenster")? {
        nutzlast["fenster"] = serde_json::json!(fenster);
    }

    for (name, feld) in [("--agent", "agent"), ("--link", "link")] {
        if let Some(v) = wert(&gefunden, name) {
            nutzlast[feld] = serde_json::json!(v);
        }
    }

    if !text.trim().is_empty() {
        nutzlast["text"] = serde_json::json!(text.trim_end());
    }

    let connection = config::load()?;
    let token = mcp::access_token(&connection)?;
    let roh = mcp::call_tool(&connection, &token, "report-imkr-event-tool", nutzlast)?;

    if args.iter().any(|a| a == "--json") {
        println!(
            "{}",
            serde_json::to_string(&roh).map_err(|e| format!("{e}"))?
        );

        return Ok(());
    }

    let gemeldet: Gemeldet = serde_json::from_value(roh)
        .map_err(|e| format!("Antwort auf report-imkr-event-tool ist unlesbar: {e}"))?;

    println!("Meldung #{} liegt im Posteingang.", gemeldet.id);

    Ok(())
}

fn gelesen(args: &[String]) -> Result<(), String> {
    let connection = config::load()?;
    let token = mcp::access_token(&connection)?;
    let roh = mcp::call_tool(
        &connection,
        &token,
        "fetch-imkr-read-tool",
        serde_json::json!({}),
    )?;

    // Unverändert durchreichen — das ist die Form, auf die sich imkr verlässt.
    if args.iter().any(|a| a == "--json") {
        println!(
            "{}",
            serde_json::to_string(&roh).map_err(|e| format!("{e}"))?
        );

        return Ok(());
    }

    let rueckweg: Rueckweg = serde_json::from_value(roh)
        .map_err(|e| format!("Antwort auf fetch-imkr-read-tool ist unlesbar: {e}"))?;

    if rueckweg.gelesen.is_empty() {
        println!("Nichts abgehakt.");

        return Ok(());
    }

    for g in &rueckweg.gelesen {
        println!(
            "#{} {} {}/{}{}",
            g.id,
            g.art.as_deref().unwrap_or("?"),
            g.sitzung.as_deref().unwrap_or("?"),
            g.pane.map(|p| p.to_string()).unwrap_or("?".into()),
            // Grösser als eins heisst: schon einmal geholt, nie bestätigt.
            if g.abholungen > 1 {
                format!(" (zum {}. Mal geholt)", g.abholungen)
            } else {
                String::new()
            }
        );
    }

    println!();
    println!("Erledigt melden:  ai-brain-harness meldung bestaetigen <nummer>");

    Ok(())
}

fn bestaetigen(args: &[String]) -> Result<(), String> {
    let Some(roh) = args.first() else {
        return Err("`ai-brain-harness meldung bestaetigen <nummer>`".to_string());
    };

    let nummer: u64 = roh
        .trim_start_matches('#')
        .parse()
        .map_err(|_| format!("`{roh}` ist keine Meldungsnummer."))?;

    let connection = config::load()?;
    let token = mcp::access_token(&connection)?;

    let antwort = mcp::call_tool_text(
        &connection,
        &token,
        "confirm-imkr-read-tool",
        serde_json::json!({ "meldung_id": nummer }),
    )?;

    println!("{}", antwort.trim());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{flaggen, wert, zahl};

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flaggen_lassen_nur_den_titel_stehen() {
        let a = args(&[
            "--art",
            "blocked",
            "claude",
            "needs",
            "you",
            "--pane",
            "3",
            "--sitzung",
            "imkr",
        ]);
        let (gefunden, titel) = flaggen(&a, &["--art", "--pane", "--sitzung", "--agent"]);

        assert_eq!(titel, "claude needs you");
        assert_eq!(wert(&gefunden, "--art"), Some("blocked"));
        assert_eq!(wert(&gefunden, "--agent"), None);
        assert_eq!(zahl(&gefunden, "--pane"), Ok(Some(3)));
        assert_eq!(zahl(&gefunden, "--fenster"), Ok(None));
    }

    #[test]
    fn eine_zahl_die_keine_ist_wird_gesagt() {
        let a = args(&["--pane", "drei"]);
        let (gefunden, _) = flaggen(&a, &["--pane"]);

        assert!(zahl(&gefunden, "--pane").is_err());
    }
}
