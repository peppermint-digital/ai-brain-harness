//! `status` — eine Antwort auf die Frage „haengt dieser Rechner?" (#9059).
//!
//! ## Warum es diesen Befehl zusaetzlich gibt
//!
//! Bis zum 29.10.2026 zeigte `status` die hinterlegte Verbindung und sonst
//! nichts. Das ist eine Auskunft ueber eine **Datei**, nicht ueber eine
//! Verbindung: Ein Rechner, dessen Zugang laengst entzogen war, las sich dort
//! wie ein verbundener. Wer fragt, will aber wissen, ob es gerade traegt.
//!
//! Dazu kommt, dass der Stand bisher auf vier Befehle verteilt war — `status`,
//! `leitung status`, `tunnel status`, `taktgeber`. Eine Anzeige, die alle 30
//! Sekunden eine Zeile schreiben soll, kann nicht vier Programme starten.
//!
//! ## Was diesen Befehl billig haelt
//!
//! Genau **eine** Anfrage an die Instanz (`GET geraete/ich`, drei Sekunden
//! Frist), mit einem Token aus der Ablage — es wird keines nebenbei erneuert.
//! Alles andere liest Dateien dieses Rechners, dazu ein `systemctl is-active`.
//!
//! Das ist der Unterschied zu `leitung status --json`, das der Tray im
//! **Zwei**-Sekunden-Takt liest: Dort waere eine HTTP-Anfrage Dauerlast, und
//! deshalb steht dort bis heute nur der hinterlegte Zugang. Hier ist der Takt
//! 30 Sekunden, und damit ist die echte Messung bezahlbar.

use std::time::Duration;

use crate::config::{self, Connection};
use crate::leitung::{self, Erreichbar};
use crate::register::{self, Zustand};
use crate::taktgeber;
use crate::tunnel;

/// Wie lange auf die Instanz gewartet wird.
///
/// Drei Sekunden: Die Anzeige, die das liest, hat 30 Sekunden Takt — eine
/// Messung, die laenger braucht als ein Zehntel ihres Taktes, ist keine
/// Anzeige mehr, sondern eine Warteschleife. Und „nicht erreichbar" ist nach
/// drei Sekunden eine ehrliche Antwort: Eine Instanz, die laenger braucht,
/// traegt keine Leitung.
const FRIST: Duration = Duration::from_secs(3);

const HILFE: &str = "\
Zeigt, ob dieser Rechner an seiner AI-Brain-Instanz haengt.

  ai-brain-harness status            Verbindung, Erreichbarkeit, Leitung, Tunnel, Taktgeber
  ai-brain-harness status --json     Dasselbe als JSON, fuer Anzeigen

Geprueft wird echt: ein kurzer angemeldeter Aufruf gegen die Instanz
(3 s Frist). Ein Token wird dafuer nicht erneuert.
";

pub fn zeigen(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None => text(),
        Some("--json") => {
            println!("{}", json()?);
            Ok(())
        }
        Some("--help") | Some("-h") => {
            print!("{HILFE}");
            Ok(())
        }
        Some(anders) => Err(format!("Unbekannt: status {anders}\n\n{HILFE}")),
    }
}

/// Die drei Woerter, die `grund` kennt.
///
/// **Es sind Woerter, keine Saetze.** Eine Anzeige vergleicht sie; ein Satz
/// waere dort ein Textmuster, das bei jeder Umformulierung bricht. Der Satz
/// steht daneben in `meldung`.
fn wort(e: &Erreichbar) -> &'static str {
    match e {
        Erreichbar::Ja => "ok",
        Erreichbar::Abgemeldet(_) => "abgemeldet",
        Erreichbar::Nicht(_) => "nicht erreichbar",
    }
}

fn meldung(e: &Erreichbar) -> Option<&str> {
    match e {
        Erreichbar::Ja => None,
        Erreichbar::Abgemeldet(m) | Erreichbar::Nicht(m) => Some(m),
    }
}

/// Wie viele Laeufe laufen, wie viele warten.
///
/// Ein Fehler beim Lesen des Registers ist hier kein Abbruch: Die Frage dieses
/// Befehls ist die Verbindung, und ein Rechner ohne Register (noch nie ein Lauf
/// gestartet) ist der Normalfall, nicht ein Fehlerfall.
fn laeufe() -> (usize, usize) {
    let liste = register::liste().unwrap_or_default();

    (
        liste
            .iter()
            .filter(|l| l.zustand == Zustand::Laeuft)
            .count(),
        liste
            .iter()
            .filter(|l| l.zustand == Zustand::Wartet)
            .count(),
    )
}

/// Die Messung, einmal — fuer Text und JSON dieselbe.
///
/// Ohne hinterlegten Zugang gibt es nichts zu messen, und das ist „abgemeldet":
/// Der Rechner ist nicht gekoppelt. Kein Abbruch mit Fehler — „nicht
/// verbunden" ist eine Antwort auf die Frage, nicht ein Scheitern daran.
fn messen() -> (Option<Connection>, Erreichbar) {
    match config::load() {
        Ok(c) => {
            let e = leitung::erreichbarkeit(&c, FRIST);
            (Some(c), e)
        }
        Err(_) => (
            None,
            // Einzeilig, und das ist kein Zierrat: Diese Meldung landet in einer
            // Statuszeile von vielleicht dreissig Zeichen. Der Weg nach draussen
            // steht trotzdem drin — ohne ihn ist „nicht gekoppelt" eine
            // Feststellung ohne Ausweg.
            Erreichbar::Abgemeldet(
                "Dieser Rechner ist nicht gekoppelt — ai-brain-harness connect <domain>"
                    .to_string(),
            ),
        ),
    }
}

fn text() -> Result<(), String> {
    let (verbindung, erreichbar) = messen();

    match &verbindung {
        Some(c) => {
            println!("Instanz       : {} ({})", c.instance_name, c.base_url);
            println!("Gerät         : {}", c.device_name);
            println!("Zugerechnet an: {}", c.owner);
            println!("MCP-Endpunkt  : {}", c.mcp_url);
        }
        None => println!("Instanz       : nicht gekoppelt"),
    }

    match &erreichbar {
        Erreichbar::Ja => println!("Erreichbar    : ja"),
        _ => println!("Erreichbar    : nein — {}", wort(&erreichbar)),
    }

    // Der Grund unter die Zeile, eingerueckt, nicht in Klammern dahinter: Eine
    // Fehlermeldung der Instanz ist mehrzeilig, sobald es ernst wird — und in
    // Klammern gesetzt zerfaellt die Tabelle beim ersten Zeilenumbruch.
    if let Some(m) = meldung(&erreichbar) {
        for zeile in m.lines() {
            println!("                {zeile}");
        }
    }

    let l = leitung::kurzstand();
    println!(
        "Leitung       : {}{}",
        l.grund,
        match &l.lage {
            Some(lage) => format!(" — {}", lage.stand),
            None => String::new(),
        }
    );

    let t = tunnel::kurzstand();
    println!("Tunnel        : {} (Port {})", t.grund, t.port);

    let takt = taktgeber::kurzstand();
    println!(
        "Taktgeber     : {}",
        match (&takt.eingerichtet, &takt.aktiv) {
            (false, _) => "nicht eingerichtet".to_string(),
            (true, Some(a)) => a.clone(),
            (true, None) => "eingerichtet, keine Auskunft".to_string(),
        }
    );

    let (laeuft, wartet) = laeufe();
    println!("Läufe         : {laeuft} laufen, {wartet} warten");

    Ok(())
}

fn json() -> Result<String, String> {
    let (verbindung, erreichbar) = messen();
    let (laeuft, wartet) = laeufe();

    serde_json::to_string(&serde_json::json!({
        // Die eine Frage, als ein Wahrheitswert: Hat die Instanz gerade auf
        // einen angemeldeten Aufruf dieses Rechners geantwortet?
        "verbunden": matches!(erreichbar, Erreichbar::Ja),
        // `ok`, `abgemeldet` oder `nicht erreichbar`.
        "grund": wort(&erreichbar),
        // Der Satz zum Grund — fuer Menschen, nicht zum Vergleichen.
        "meldung": meldung(&erreichbar),
        "instanz": verbindung.as_ref().map(|c| c.instance_name.clone()),
        "basis": verbindung.as_ref().map(|c| c.base_url.clone()),
        "geraet": verbindung.as_ref().map(|c| c.device_name.clone()),
        "zugerechnet": verbindung.as_ref().map(|c| c.owner.clone()),
        "leitung": leitung::kurzstand(),
        "tunnel": tunnel::kurzstand(),
        "taktgeber": taktgeber::kurzstand(),
        // Die Laeufe stehen hier mit, damit eine Anzeige nicht ZWEI Programme
        // starten muss, um eine Zeile zu schreiben. Sie sind aber der Zusatz,
        // nicht die Auskunft: `ai-brain-harness agents` bleibt der Ort, an dem
        // Laeufe einzeln stehen.
        "laeufe": { "laeuft": laeuft, "wartet": wartet },
    }))
    .map_err(|e| format!("Status laesst sich nicht als JSON schreiben: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_drei_gruende_sind_woerter_und_bleiben_es() {
        // Eine Anzeige vergleicht genau diese drei. Wer sie umformuliert,
        // bricht die Anzeige still — deshalb stehen sie hier scharf.
        assert_eq!(wort(&Erreichbar::Ja), "ok");
        assert_eq!(wort(&Erreichbar::Abgemeldet(String::new())), "abgemeldet");
        assert_eq!(wort(&Erreichbar::Nicht(String::new())), "nicht erreichbar");
    }

    #[test]
    fn nur_ja_ist_verbunden() {
        for e in [
            Erreichbar::Abgemeldet("x".into()),
            Erreichbar::Nicht("y".into()),
        ] {
            assert!(!matches!(e, Erreichbar::Ja));
            // Und der Satz geht nicht verloren: Wer „nicht erreichbar" liest,
            // soll auch lesen koennen, woran es lag.
            assert!(meldung(&e).is_some());
        }

        assert!(meldung(&Erreichbar::Ja).is_none());
    }
}
