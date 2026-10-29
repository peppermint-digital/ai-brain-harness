//! AI Brain Harness
//!
//! Verbindet einen Rechner mit einer AI-Brain-Instanz, gibt ihm deren Werkzeuge
//! *und* die Hoheit über diesen Rechner — und hält das Gespräch in einer
//! eigenen Maske.
//!
//! **Der Befehl hiess bis zum 15.10.2026 `ai-brain-cli`.** Der alte Name bleibt
//! als Verweis, bis alle Rechner umgestellt sind. Die Ablage unter
//! `~/.config/ai-brain-cli` behält ihren Namen — siehe `config::alter_config_dir`.
//!
//! Zwei Aufgaben, zwei Unterbefehle:
//!
//! * `connect <domain>` — koppelt den Rechner. Liest den Instanz-Steckbrief
//!   unter `/.well-known/ai-brain`, startet den Device-Flow, zeigt einen kurzen
//!   Code an und holt nach der Bestätigung im Browser **eigene**
//!   Gerätezugangsdaten ab.
//! * `mcp` — läuft als MCP-Server über stdio. Claude Code startet diesen
//!   Prozess; er hält die Gerätezugangsdaten, besorgt sich damit Token und
//!   reicht die Aufrufe an die Instanz weiter.
//!
//! Warum der zweite Teil nötig ist: Ein Zugangstoken lebt nur eine Stunde,
//! Claude Codes Konfiguration kann aber nur einen festen Wert halten. Ohne
//! einen Prozess, der Token erneuert, wäre die Verbindung nach einer Stunde
//! tot. Genau deshalb ist die CLI nicht nur ein Einrichtungsprogramm.

mod agent;
mod anbieter;
mod anzeige;
mod aufgaben;
mod auftrag;
mod chat;
mod config;
mod dauerauftrag;
mod discovery;
mod erinnerungen;
mod freigabe;
mod imkr;
mod frage;
mod konto;
mod einrichten;
mod leitung;
mod maske;
mod mcp;
mod mcp_eintraege;
mod meldung;
mod nachricht;
mod neustart;
mod pairing;
mod pusher;
mod rechner;
mod schleife;
mod register;
mod taktgeber;
mod tunnel;
mod verbrauch;
mod select;
mod sitzung;
mod stand;
mod sync;

use std::process::ExitCode;

const USAGE: &str = "\
AI Brain Harness

Aufrufe:
  ai-brain-harness einrichten [...]   Alles in einem Befehl: verbinden, abgleichen,
                                  MCP eintragen, Leitung und Leiste anbieten
  ai-brain-harness connect [domain]   Rechner mit einer AI-Brain-Instanz verbinden
                                  (ohne Angabe: die Instanz, von der dieses
                                  Programm kam)
  ai-brain-harness mcp                Als MCP-Server laufen (startet Claude Code selbst)
  ai-brain-harness select             Waehlen, welche Eintraege dieser Rechner bekommt
  ai-brain-harness sync               Skills, Agents und gewaehlte MCP-Server abgleichen
  ai-brain-harness erinnerungen [..]  Das Gedaechtnis mit AI Brain abgleichen
  ai-brain-harness publish <name>     Einen lokalen Eintrag ins Repository geben
  ai-brain-harness chat [...]         Ein Gespraech hier fuehren (kurz: `brain`)
  ai-brain-harness agent [...]        Einen Agentenlauf hier starten
  ai-brain-harness agents             Welche Agentenlaeufe hier laufen
  ai-brain-harness schleife [...]     Der eigene Agentenloop, ohne fremde Befehlszeile
                                  (<frage> | --liste | --eintragen | --messen)
  ai-brain-harness aufgaben [...]     Offene Aufgaben eines Projekts (--projekt, --json)
  ai-brain-harness auftraege          Was AI Brain fuer diesen Rechner bereitlegt
  ai-brain-harness auftrag <nummer>   Einen Auftrag annehmen und abarbeiten
  ai-brain-harness auftrag-melden ..  Ergebnis eines Auftrags nachtragen
  ai-brain-harness daueraufgaben      Dauerauftraege dieses Rechners (--fahren, --json)
  ai-brain-harness taktgeber [...]    Taktgeber fuer Auftraege und Dauerauftraege
                                  (--einrichten [--takt <kalender>] |
                                  --entfernen | --zustand)
  ai-brain-harness nachricht ..       Nachrichten zwischen imkrs:
                                  kontakte | senden <person> <text> |
                                  abholen | bestaetigen <nummer>
  ai-brain-harness meldung ..         imkr-Ereignisse im gemeinsamen Posteingang:
                                  senden --art <blocked|done|failed|account|
                                  message> --rechner <r> --sitzung <s>
                                  --pane <n> [--fenster <n>] [--agent <name>]
                                  [--link <url>] <titel> (Text auf stdin) |
                                  gelesen [--json] | bestaetigen <nummer>
  ai-brain-harness freigabe [...]     Verzeichnisse fuer Laeufe mit mehr Rechten
  ai-brain-harness leitung [...]      Live-Leitung: ein imkr-Pane in AI Brain zeigen
                                  (an [--dauer 30m] | aus | status), Standard aus
  ai-brain-harness tunnel [...]       Den Browser deines Rechners an einen Agenten
                                  hier geben (start | status | stop)
  ai-brain-harness logs <kennung>     Was ein Lauf geschrieben hat
  ai-brain-harness stop <kennung>     Einen Lauf beenden
  ai-brain-harness status [--json]    Haengt dieser Rechner? Prueft die Instanz echt
                                  (Verbindung, Leitung, Tunnel, Taktgeber, Laeufe)
  ai-brain-harness verbrauch [...]    Token und Zeit der Sitzungen melden
                                  (melden | zeigen | stand | an | aus | hook)
  ai-brain-harness configure          Traegt den MCP-Server in Claude Code und Codex ein
  ai-brain-harness disconnect         Entfernt die lokale Verbindung

Beispiel:
  ai-brain-harness connect brain.proxy.peppermint-digital.com

Optionen für `sync`:
  --apply               Änderungen wirklich schreiben (ohne: erst fragen)
  --only <name>         Nur diesen einen Eintrag
  --force               Auch überschreiben, wo lokal geändert wurde
                        (die lokale Fassung wird gesichert)
  --aufraeumen          Nicht mehr ausgewählte Einträge entfernen — aber nur solche,
                        die die CLI selbst geschrieben hat und die seither
                        unverändert sind
  --ja                  Nicht fragen, sichere Vorgabe nehmen: anwenden ja, aber
                        nichts überschreiben und nichts entfernen. Dafür bleiben
                        --force und --aufraeumen nötig.

  Am Terminal fragt `sync` vor dem Anwenden und bei jedem strittigen Eintrag
  einzeln (behalten / überschreiben / Unterschied ansehen). Ohne Terminal wird
  NIE gefragt — dann gilt, was an Optionen dasteht.

  MCP-Server (Typ mcp) traegt `sync` nur ein, wenn sie auf diesem Rechner per
  `select` einzeln gewaehlt sind — nie ueber „alles“. Eingetragen wird in jede
  Claude-Konfiguration (~/.claude.json, $CLAUDE_CONFIG_DIR, imkr-Konten) und
  ueber `codex mcp add` in Codex. Die Konfiguration wird vorher gesichert.

Optionen für `agent`:
  --task <N>            Aufgabe aus AI Brain (die Beschreibung wird geholt)
  --projekt <slug>      Projekt, in dem gearbeitet wird
  --modell claude|codex Welche Schleife gemietet wird (Vorgabe: claude)
  --dir <Pfad>          Arbeitsverzeichnis (Vorgabe: das aktuelle)
  --fortsetzen          Die letzte Unterhaltung in diesem Verzeichnis weiterfuehren
  --sitzung <id>        Eine bestimmte Unterhaltung weiterfuehren (siehe `logs`)
  --imkr                In einem neuen imkr-Fenster statt als Kindprozess
  --zeitgrenze <Sek>    Nach dieser Zeit abbrechen (Vorgabe 3600, 0 = keine)
  --schreibend          Darf Dateien schreiben, ohne zu fragen
  --vollzugriff         Fragt nichts mehr („God-Mode“)
  --trocken             Nur zeigen, was gestartet wuerde, samt Vorlauf

  --schreibend und --vollzugriff gehen NUR in freigegebenen Verzeichnissen:
    ai-brain-harness freigabe add ~/Development

  Freie Woerter werden der Auftrag: `agent --projekt x \"Changelog schreiben\"`.

Optionen für `select`:
  --zeigen              Nur anzeigen, nichts fragen und nichts aendern
  --dazu a,b            Ohne Rueckfrage ergaenzen; der Modus bleibt (alles bleibt alles)
  --weg a,b             Ohne Rueckfrage entfernen; der Modus bleibt
                        Namen als typ/slug (mcp/playwright) oder eindeutiger Slug
  --setzen a,b,c        Ohne Rueckfrage genau diese Eintraege setzen. ACHTUNG: stellt
                        auf Modus auswahl und ERSETZT die ganze Liste — zum Ergaenzen
                        --dazu nehmen
  --alles               Ohne Rueckfrage auf alles stellen (auch kuenftige Eintraege;
                        MCP-Server nie — die bleiben, wie sie einzeln gewaehlt sind)
  --geraet <Name>       Ein anderes eigenes Geraet einstellen (Standard: dieser Rechner)

  Ohne Terminal wird nie gefragt — dann zeigt `select` nur an.

Optionen für `connect`:
  --name <Gerätename>   Abweichender Gerätename (Standard: Hostname)
  --no-claude-config    Claude-Code-Konfiguration nicht anfassen
  --kein-browser        Die Bestätigungsseite nicht öffnen. Der Code und die
                        Adresse stehen ohnehin im Terminal — über ssh ohne
                        Anzeige wird sie von sich aus nicht geöffnet.
  --neu                 Auch dann koppeln, wenn dieser Rechner schon verbunden ist.
                        Legt einen ZWEITEN Gerätezugang an — für ein Update ist das
                        nicht nötig, da genügt das Ersetzen des Programms.
";

/// Trägt den MCP-Server nachträglich in die vorhandenen Werkzeuge ein.
///
/// Gebraucht von allen, die mit `--no-claude-config` gekoppelt haben — und
/// beim Umstellen eines Rechners, der vorher auf einem anderen Weg an AI Brain
/// hing. Ohne diesen Befehl bliebe als Ausweg nur, ein zweites Mal zu koppeln,
/// und das legt einen überflüssigen zweiten Gerätezugang an.
///
/// `configure-claude` bleibt als Zweitname bestehen — der Befehl stand so in
/// der README, und wer ihn sich notiert hat, soll nicht ins Leere laufen.
fn configure_tools() -> Result<(), String> {
    // Hier bricht der Riegel ab, anders als im Assistenten: Einträge
    // schreiben IST der ganze Zweck dieses Befehls. Bliebe er stehen und täte
    // nichts, wäre das die schlechtere Antwort.
    config::riegel_bauverzeichnis()?;

    // Erst prüfen, ob überhaupt eine Verbindung existiert: sonst schreiben wir
    // einen MCP-Eintrag, der beim Start sofort scheitert.
    let connection = config::load()?;

    println!("Instanz : {}", connection.base_url);
    println!("Gerät   : {}", connection.device_name);
    println!();

    let (path, geraeumt) = config::write_claude_config_mit_bericht()?;
    println!("Claude Code : {}", path.display());

    // Was weggeraeumt wurde, gehoert gesagt. Ein Eintrag, der ohne ein Wort
    // verschwindet, ist spaeter nicht nachvollziehbar — und wer ihn vermisst,
    // sucht an der falschen Stelle.
    for schluessel in &geraeumt {
        println!("              alter Eintrag „{schluessel}“ entfernt (zeigte auf eine frühere Fassung)");
    }

    // Nachgelesen statt behauptet (#1123): Beide Werkzeuge ersetzen einen
    // vorhandenen Eintrag stillschweigend, und „angelegt" war beim zweiten
    // Lauf schlicht falsch.
    match config::write_codex_config()? {
        Some(stand) => println!("Codex       : {}", stand.wort()),
        None => println!("Codex       : nicht installiert, übersprungen"),
    }

    match config::write_pi_config()? {
        Some(stand) => println!("pi          : {}", stand.wort()),
        None => println!("pi          : nicht installiert, übersprungen"),
    }

    // Zuletzt, und das ist keine Reihenfolge nach Geschmack: `imkr account
    // sync` verteilt die mcpServers AUS `~/.claude.json` in die Konten. Vor
    // dem Schreiben aufgerufen, verteilte es den alten Stand.
    match config::imkr_konten_versorgen()? {
        Some(_) => println!("imkr-Konten : über „imkr account sync“ versorgt"),
        None => println!("imkr-Konten : imkr nicht installiert, übersprungen"),
    }

    println!();
    println!("Starte die Werkzeuge neu, damit die Einträge greifen.");

    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Vor allem anderen: die Ablage des alten Namens herüberholen (#7664).
    // Steht hier und nicht in `config_dir()`, weil das eine reine
    // Pfadfunktion ist — ein Seiteneffekt darin würde in jedem Test feuern.
    config::uebernimm_alte_ablage();

    let result = match args.first().map(String::as_str) {
        Some("connect") => pairing::connect(&args[1..]),
        Some("einrichten") => einrichten::verwalten(&args[1..]),
        Some("mcp") => mcp::serve(),
        Some("rechner-mcp") => rechner::serve(),
        Some("sync") => sync::run(&args[1..]),
        Some("select") => select::run(&args[1..]),
        Some("publish") => sync::publish(&args[1..]),
        Some("agent") => agent::starten(&args[1..]),
        Some("freigabe") => freigabe::verwalten(&args[1..]),
        // Die Maske ist der Normalfall; `--durchreichen` gibt das Terminal
        // wie fruehr direkt an das Werkzeug ab.
        Some("chat") | Some("maske") => {
            if args[1..].iter().any(|a| a == "--durchreichen") {
                chat::starten(&args[1..])
            } else {
                maske::starten(&args[1..])
            }
        }
        Some("agents") => register::anzeigen(&args[1..]),
        Some("aufgaben") => aufgaben::anzeigen(&args[1..]),
        Some("daueraufgaben") => dauerauftrag::anzeigen(&args[1..]),
        Some("taktgeber") => taktgeber::verwalten(&args[1..]),
        Some("schleife") => schleife::verwalten(&args[1..]),
        Some("verbrauch") => verbrauch::verwalten(&args[1..]),
        Some("tunnel") => tunnel::verwalten(&args[1..]),
        Some("leitung") => leitung::verwalten(&args[1..]),
        Some("erinnerungen") => erinnerungen::abgleichen(&args[1..]),
        Some("auftraege") => auftrag::anzeigen(&args[1..]),
        Some("auftrag") => auftrag::annehmen(&args[1..]),
        Some("auftrag-melden") => auftrag::nachmelden(&args[1..]),
        Some("nachricht") => nachricht::fuehren(&args[1..]),
        Some("meldung") => meldung::fuehren(&args[1..]),
        Some("logs") => register::protokoll_zeigen(&args[1..]),
        Some("stop") => register::stoppen(&args[1..]),
        Some("status") => stand::zeigen(&args[1..]),
        Some("configure") | Some("configure-claude") => configure_tools(),
        Some("disconnect") => config::disconnect(),
        Some("--help") | Some("-h") | Some("help") | None => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("Unbekannter Befehl: {other}\n");
            print!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // Fehler gehen immer nach stderr: im `mcp`-Modus ist stdout der
            // JSON-RPC-Kanal, dort würde eine Fehlermeldung das Protokoll
            // zerstören und Claude Code mit einem Parse-Fehler abbrechen.
            eprintln!("Fehler: {err}");
            ExitCode::FAILURE
        }
    }
}
