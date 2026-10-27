//! Der Einrichtungsassistent: ein Befehl, danach läuft alles (#8949).
//!
//! ## Warum der Ablauf hier steht und nicht im Installationsskript
//!
//! Die Frage ist nicht theoretisch — sie hat eine Zahl. Am 27.10.2026 kamen
//! zwei Fragen dazu (Tray, Leitungsdienst), und sie stehen seitdem in **drei**
//! Dateien: dem `install.sh` dieses Repositories und den beiden Skripten, die
//! AI Brain erzeugt (`install.sh` und `install.ps1`, als Heredoc in PHP). Das
//! sind sechs Frageblöcke für zwei Fragen.
//!
//! Zwei davon stecken in PHP-Zeichenketten, die kein Werkzeug prüft: kein
//! `shellcheck`, kein Compiler, kein Test. Ein Tippfehler dort fällt erst dem
//! auf, der installiert — und der installiert gerade.
//!
//! Hier ist es eine Fassung, auf allen drei Plattformen dieselbe, mit Tests.
//! Die Skripte holen das Programm und rufen diesen Befehl.
//!
//! ## Was abbricht und was weiterläuft
//!
//! Nur **Schritt 1 ist tödlich**: Ohne Verbindung hat kein weiterer Schritt
//! einen Sinn. Alles danach meldet seinen Fehlschlag und macht weiter.
//!
//! Das ist Absicht und nicht Nachlässigkeit. Ein Rechner, auf dem der Abgleich
//! am Netz scheiterte, der MCP-Eintrag aber sitzt, ist brauchbar — und die
//! Zusammenfassung sagt, was fehlt. Ein Assistent, der bei Schritt 2 anhält,
//! lässt jemanden mit der Frage zurück, was von den fünf Schritten eigentlich
//! schon passiert ist.
//!
//! ## Die Zusammenfassung MISST
//!
//! Jede Zeile am Ende kommt aus dem Nachlesen, nicht aus „wir haben es
//! versucht". Der Unterschied ist derselbe, der heute zweimal wehgetan hat:
//! Eine Anzeige, die ihren eigenen Vorsatz wiedergibt, ist technisch richtig
//! und beantwortet nichts.

use crate::{config, erinnerungen, leitung, pairing, select, sync};

const HILFE: &str = "\
Richtet diesen Rechner vollstaendig fuer AI Brain ein — in einem Befehl.

Aufruf:
  ai-brain-harness einrichten [Optionen]

Schritte:
  1 Verbinden        Geraetecode holen, Browser oeffnen, bestaetigen lassen
  2 Abgleichen       Skills, Agents und gewaehlte MCP-Server holen
  3 Werkzeuge        MCP-Eintrag in Claude Code, Codex, pi und die imkr-Konten
  4 Erinnerungen     das Gedaechtnis einmal abgleichen
  5 Leitung, Leiste  Dienst und Tray anbieten

Optionen:
  --ja                Nicht fragen, sichere Vorgaben nehmen
  --kein-dienst       Den Leitungsdienst nicht einrichten
  --kein-tray         Den Tray-Autostart nicht einrichten
  --kein-browser      Beim Verbinden keinen Browser oeffnen
  --instanz <adresse> Instanz, mit der verbunden wird
  --geraet <name>     Name dieses Geraetezugangs

Schon verbunden? Dann wird Schritt 1 uebersprungen — ein zweiter Lauf legte
sonst einen ZWEITEN Geraetezugang an.
";

/// Was der Assistent am Ende berichtet. Jedes Feld ist eine Messung.
#[derive(Default)]
struct Bericht {
    verbindung: Option<String>,
    abgleich: String,
    claude: String,
    codex: String,
    pi: String,
    imkr: String,
    erinnerungen: String,
    leitung: String,
    tray: String,
}

struct Wunsch {
    fragen: bool,
    dienst: bool,
    tray: bool,
    browser: bool,
    instanz: Option<String>,
    geraet: Option<String>,
}

/// Die Befehlszeile lesen. Rein, damit die Vorgaben ohne Lauf prüfbar sind.
fn gelesen(args: &[String]) -> Result<Wunsch, String> {
    let mut w = Wunsch {
        fragen: true,
        dienst: true,
        tray: true,
        browser: true,
        instanz: None,
        geraet: None,
    };

    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--ja" => w.fragen = false,
            "--kein-dienst" => w.dienst = false,
            "--kein-tray" => w.tray = false,
            "--kein-browser" => w.browser = false,
            "--instanz" => {
                i += 1;
                w.instanz = Some(
                    args.get(i)
                        .ok_or("--instanz braucht eine Adresse.")?
                        .clone(),
                );
            }
            "--geraet" | "--gerät" => {
                i += 1;
                w.geraet = Some(args.get(i).ok_or("--geraet braucht einen Namen.")?.clone());
            }
            anders => return Err(format!("Unbekannt: {anders}\n\n{HILFE}")),
        }

        i += 1;
    }

    Ok(w)
}

pub fn verwalten(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") {
        print!("{HILFE}");
        return Ok(());
    }

    let wunsch = gelesen(args)?;
    let mut bericht = Bericht::default();

    schritt(1, "Verbindung");
    verbinden(&wunsch, &mut bericht)?;

    schritt(2, "Skills, Agents und MCP-Server abgleichen");
    abgleichen(&mut bericht);

    schritt(3, "MCP-Eintrag in den Werkzeugen");
    werkzeuge(&mut bericht);

    schritt(4, "Erinnerungen");
    bericht.erinnerungen = match erinnerungen::abgleichen(&[]) {
        Ok(()) => "abgeglichen".to_string(),
        Err(grund) => format!("nicht abgeglichen: {}", kurz(&grund)),
    };

    schritt(5, "Leitung und Leiste");
    leitung_und_leiste(&wunsch, &mut bericht);

    zusammenfassen(&bericht);

    Ok(())
}

fn schritt(nummer: u8, was: &str) {
    println!();
    println!("── Schritt {nummer}/5 · {was}");
    println!();
}

/// Schritt 1. Der einzige, der abbricht.
fn verbinden(wunsch: &Wunsch, bericht: &mut Bericht) -> Result<(), String> {
    // Schon verbunden ist der Normalfall bei einem Update — und ein zweiter
    // `connect` würde nicht erneuern, sondern einen ZWEITEN Gerätezugang
    // anlegen: derselbe Rechner zweimal unter „Meine Geräte", der neue mit
    // der Grundausstattung statt der gepflegten Auswahl, und die
    // Zeiterfassung auf zwei Namen.
    if let Ok(vorhanden) = config::load() {
        println!(
            "Schon verbunden mit {} als „{}“ — übersprungen.",
            vorhanden.base_url, vorhanden.device_name
        );
        println!("Neu koppeln wäre ein eigener Schritt: erst `disconnect`, dann `connect`.");

        bericht.verbindung = Some(format!(
            "{} als „{}“, zugerechnet an {}",
            vorhanden.base_url, vorhanden.device_name, vorhanden.owner
        ));

        return Ok(());
    }

    let mut args: Vec<String> = Vec::new();

    if let Some(adresse) = &wunsch.instanz {
        args.push(adresse.clone());
    }

    if let Some(name) = &wunsch.geraet {
        args.push("--name".to_string());
        args.push(name.clone());
    }

    if !wunsch.browser {
        args.push("--kein-browser".to_string());
    }

    pairing::connect(&args)?;

    let neu = config::load()?;

    bericht.verbindung = Some(format!(
        "{} als „{}“, zugerechnet an {}",
        neu.base_url, neu.device_name, neu.owner
    ));

    Ok(())
}

/// Schritt 2. Zeigen, was ausgewählt ist — und dann holen.
///
/// **Kein interaktives `select`.** Es wäre der naheliegende Schritt („lass ihn
/// auswählen, was er will"), und es wäre falsch: Ein Assistent, der in eine
/// Auswahlmaske springt, ist nicht mehr „ein Befehl, danach läuft alles".
/// Stattdessen `select --zeigen` — dieselbe Auskunft ohne Rückfrage — und der
/// Hinweis, wie man sie ändert.
///
/// `sync --apply --ja` heisst: anwenden ja, aber nichts überschreiben und
/// nichts entfernen. Ein Assistent, der beim Einrichten lokale Änderungen
/// überschreibt, ist einer, den man einmal benutzt.
fn abgleichen(bericht: &mut Bericht) {
    if let Err(grund) = select::run(&["--zeigen".to_string()]) {
        println!("Auswahl nicht lesbar: {grund}");
    }

    println!();

    let args = ["--apply".to_string(), "--ja".to_string()];

    bericht.abgleich = match sync::run(&args) {
        Ok(()) => "angewendet (nichts überschrieben, nichts entfernt)".to_string(),
        Err(grund) => format!("nicht abgeglichen: {}", kurz(&grund)),
    };

    println!();
    println!("Auswahl ändern später:  ai-brain-harness select");
}

/// Schritt 3. Die Reihenfolge ist nicht beliebig: `imkr account sync` verteilt
/// AUS `~/.claude.json`, muss also nach dem Schreiben kommen.
fn werkzeuge(bericht: &mut Bericht) {
    bericht.claude = match config::write_claude_config_mit_bericht() {
        Ok((pfad, geraeumt)) => {
            println!("Claude Code : {}", pfad.display());

            for schluessel in &geraeumt {
                println!("              alter Eintrag „{schluessel}“ entfernt");
            }

            // Nachgelesen, nicht behauptet.
            if config::claude_hat_eintrag() {
                format!("eingetragen in {}", pfad.display())
            } else {
                "geschrieben, aber nicht wiedergefunden — bitte nachsehen".to_string()
            }
        }
        Err(grund) => {
            println!("Claude Code : {grund}");
            format!("nicht eingetragen: {}", kurz(&grund))
        }
    };

    bericht.codex = melden("Codex       ", config::write_codex_config());
    bericht.pi = melden("pi          ", config::write_pi_config());

    // Eigene Meldung, nicht `melden`: Dort hiesse es „Eintrag angelegt", und
    // das ist hier falsch. `imkr account sync` legt keinen Eintrag an — es
    // VERTEILT den aus `~/.claude.json` in die Konten. Beim ersten Durchlauf
    // der Probe stand genau das da („Eintrag ai-brain angelegt"), obwohl es
    // gar keine Konten gab. Eine Zeile, die mehr behauptet als geschah, ist
    // schlimmer als keine.
    bericht.imkr = match config::imkr_konten_versorgen() {
        Ok(Some(_)) => {
            println!("imkr-Konten : über „imkr account sync“ verteilt");
            // Bewusst ohne Zusage je Konto: Dafür müsste man die Konten
            // aufzählen, und `imkr account ls` liefert nur Prosa. Siehe die
            // offene Frage in der Session-Notiz zu #8949.
            "verteilt (je Konto nicht nachgelesen)".to_string()
        }
        Ok(None) => {
            println!("imkr-Konten : imkr nicht installiert, übersprungen");
            "imkr nicht installiert".to_string()
        }
        Err(grund) => {
            println!("imkr-Konten : {grund}");
            format!("nicht verteilt: {}", kurz(&grund))
        }
    };
}

/// Lange Fehlertexte für die Zusammenfassung kürzen.
///
/// Eine Übersicht, deren Zeilen über drei Zeilen umbrechen, ist keine
/// Übersicht mehr — in der Probe nahm eine DNS-Meldung die halbe
/// Zusammenfassung ein. Der ganze Text steht oben beim Schritt; hier zählt,
/// DASS etwas fehlt und woran es grob lag.
fn kurz(grund: &str) -> String {
    let erste = grund.lines().next().unwrap_or(grund).trim();

    if erste.chars().count() <= 70 {
        return erste.to_string();
    }

    let gekuerzt: String = erste.chars().take(70).collect();

    format!("{gekuerzt}… (ganzer Text oben bei Schritt)")
}

/// Eine Zeile für ein Werkzeug, das es geben kann oder nicht.
fn melden(spalte: &str, ergebnis: Result<Option<String>, String>) -> String {
    match ergebnis {
        Ok(Some(_)) => {
            println!("{spalte}: Eintrag „ai-brain“ angelegt");
            "eingetragen".to_string()
        }
        Ok(None) => {
            println!("{spalte}: nicht installiert, übersprungen");
            "nicht installiert".to_string()
        }
        Err(grund) => {
            println!("{spalte}: {grund}");
            format!("nicht eingetragen: {}", kurz(&grund))
        }
    }
}

/// Schritt 5. Die zwei Fragen, die Bastian ausdrücklich will — und nur die.
fn leitung_und_leiste(wunsch: &Wunsch, bericht: &mut Bericht) {
    bericht.leitung = if !wunsch.dienst {
        println!("Leitungsdienst: auf Wunsch übersprungen.");
        "nicht eingerichtet (--kein-dienst)".to_string()
    } else {
        println!("Die Live-Leitung lässt AI Brain die Fenster dieses Rechners sehen —");
        println!("aber nur, solange ein Channel ihn dort freigegeben hat. Ohne Freigabe");
        println!("bleibt sie aus, und `leitung aus` schaltet sie jederzeit ab.");
        println!();

        if frage("Leitungsdienst beim Anmelden starten?", wunsch.fragen) {
            match leitung::autostart_ein() {
                Ok(()) => "eingerichtet".to_string(),
                Err(grund) => format!("nicht eingerichtet: {}", kurz(&grund)),
            }
        } else {
            println!("Später:  ai-brain-harness leitung autostart");
            "abgelehnt".to_string()
        }
    };

    println!();

    bericht.tray = if !wunsch.tray {
        println!("Tray: auf Wunsch übersprungen.");
        "nicht eingerichtet (--kein-tray)".to_string()
    } else {
        tray_anbieten(wunsch.fragen)
    };
}

/// Der Tray ist ein eigenes Programm — wir richten nur seinen Autostart ein,
/// und nur wenn es da ist. Geholt hat es das Installationsskript.
fn tray_anbieten(fragen: bool) -> String {
    let Some(tray) = tray_programm() else {
        println!("Tray: nicht installiert, übersprungen.");
        return "nicht installiert".to_string();
    };

    if !frage("Tray beim Anmelden starten?", fragen) {
        println!("Später:  ai-brain-tray --autostart");
        return "abgelehnt".to_string();
    }

    match std::process::Command::new(&tray).arg("--autostart").status() {
        Ok(s) if s.success() => "eingerichtet".to_string(),
        Ok(s) => format!("nicht eingerichtet (Ausgangscode {:?})", s.code()),
        Err(e) => format!("nicht eingerichtet: {e}"),
    }
}

/// Den Tray neben dem eigenen Programm suchen, dann im PATH.
///
/// Dieselbe Reihenfolge wie beim Tray selbst, der den Harness sucht, und aus
/// demselben Grund: Beide werden ins gleiche Verzeichnis installiert, und ein
/// Autostart-Eintrag startet ohne die Shell des Nutzers — `~/.local/bin` steht
/// dort oft nicht im PATH.
fn tray_programm() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) {
        "ai-brain-tray.exe"
    } else {
        "ai-brain-tray"
    };

    if let Some(ordner) = std::env::current_exe().ok().and_then(|p| {
        let o = p.parent()?.to_path_buf();
        Some(o)
    }) {
        let daneben = ordner.join(name);

        if daneben.is_file() {
            return Some(daneben);
        }
    }

    let pfad = std::env::var_os("PATH")?;

    std::env::split_paths(&pfad)
        .map(|v| v.join(name))
        .find(|k| k.is_file())
}

/// Eine Ja-/Nein-Frage — oder die Vorgabe, wenn nicht gefragt wird.
///
/// **Ohne Terminal wird nicht gefragt, sondern JA genommen**, und das ist der
/// Unterschied zu den Installationsskripten: Dort hiess „kein Terminal" nein,
/// weil ein Skript ungefragt nichts einrichten soll. Hier hat jemand
/// `einrichten` getippt — das IST die Zustimmung zum Einrichten. Wer einzelne
/// Schritte nicht will, sagt `--kein-dienst` bzw. `--kein-tray`.
fn frage(text: &str, fragen: bool) -> bool {
    use std::io::{BufRead, IsTerminal, Write};

    if !fragen {
        println!("{text} ja (--ja)");
        return true;
    }

    if !std::io::stdin().is_terminal() {
        println!("{text} ja (kein Terminal, Vorgabe)");
        return true;
    }

    print!("{text} [J/n] ");
    let _ = std::io::stdout().flush();

    let mut zeile = String::new();

    if std::io::stdin().lock().read_line(&mut zeile).is_err() {
        return true;
    }

    let antwort = zeile.trim().to_lowercase();

    antwort.is_empty() || antwort.starts_with('j') || antwort.starts_with('y')
}

fn zusammenfassen(b: &Bericht) {
    println!();
    println!("── Fertig");
    println!();
    println!(
        "  Verbindung    : {}",
        b.verbindung.as_deref().unwrap_or("keine")
    );
    println!("  Abgleich      : {}", b.abgleich);
    println!("  Claude Code   : {}", b.claude);
    println!("  Codex         : {}", b.codex);
    println!("  pi            : {}", b.pi);
    println!("  imkr-Konten   : {}", b.imkr);
    println!("  Erinnerungen  : {}", b.erinnerungen);
    println!("  Leitungsdienst: {}", b.leitung);
    println!("  Tray          : {}", b.tray);
    println!("  Leitung jetzt : {}", leitung::stand_wort());
    println!();
    println!("Starte deine Werkzeuge neu, damit die MCP-Einträge greifen.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lange_gruende_sprengen_die_uebersicht_nicht() {
        // Der echte Fall aus der Probe: eine DNS-Meldung, die in der
        // Zusammenfassung drei Zeilen nahm.
        let lang = "Token nicht erhältlich: https://attrappe.example.invalid/oauth/token: \
                    Dns Failed: resolve dns name 'attrappe.example.invalid:443': failed to \
                    lookup address information: Name or service not known";

        let k = kurz(lang);

        assert!(k.chars().count() < 120, "{k}");
        assert!(k.contains("ganzer Text oben"), "{k}");
        // Der Anfang bleibt lesbar — er sagt, WORAN es lag.
        assert!(k.starts_with("Token nicht erhältlich"), "{k}");
    }

    #[test]
    fn ein_kurzer_grund_bleibt_unangetastet() {
        assert_eq!(kurz("Netz weg"), "Netz weg");
        // Nur die erste Zeile: Ein mehrzeiliger Fehler würde die Tabelle
        // auseinanderreissen.
        assert_eq!(kurz("Erste Zeile\nzweite Zeile"), "Erste Zeile");
    }

    #[test]
    fn ohne_optionen_wird_alles_angeboten() {
        let w = gelesen(&[]).unwrap();

        assert!(w.fragen);
        assert!(w.dienst);
        assert!(w.tray);
        assert!(w.browser);
        assert!(w.instanz.is_none());
        assert!(w.geraet.is_none());
    }

    #[test]
    fn die_abwahl_gilt_einzeln() {
        let w = gelesen(&[
            "--kein-tray".to_string(),
            "--ja".to_string(),
            "--instanz".to_string(),
            "https://brain.example.com".to_string(),
        ])
        .unwrap();

        assert!(!w.fragen);
        assert!(!w.tray);
        // Der Dienst ist davon NICHT betroffen: Zwei Fragen, zwei Schalter.
        assert!(w.dienst);
        assert_eq!(w.instanz.as_deref(), Some("https://brain.example.com"));
    }

    #[test]
    fn eine_option_ohne_wert_bricht_ab_statt_zu_raten() {
        assert!(gelesen(&["--instanz".to_string()]).is_err());
        assert!(gelesen(&["--geraet".to_string()]).is_err());
        assert!(gelesen(&["--quatsch".to_string()]).is_err());
    }

    /// Die deutsche Schreibweise gilt auch — `select` nimmt sie ebenfalls.
    #[test]
    fn geraet_auch_mit_umlaut() {
        let w = gelesen(&["--gerät".to_string(), "Laptop".to_string()]).unwrap();

        assert_eq!(w.geraet.as_deref(), Some("Laptop"));
    }
}
