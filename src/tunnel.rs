//! Den Browser des Menschen an einen Agenten auf einem anderen Rechner geben.
//!
//! Ein Agent auf der `codingmachine` startet seinen Browser **dort** — also
//! ohne Bildschirm. Zum Prüfen und Testen genügt das. Soll der Mensch aber
//! zusehen, sich einloggen oder ein Captcha lösen, muss es der Browser des
//! Rechners sein, **an dem er gerade sitzt**.
//!
//! Dieser Befehl stellt das her: er findet den Rechner des Menschen, startet
//! dort einen eigenen Chromium mit Fernsteuerungs-Port und hält einen
//! SSH-Rückwärtstunnel, bis `127.0.0.1:<port>` hier antwortet. Danach zeigen
//! `agent-browser --cdp <port>` und die `-tunnel`-MCPs auf diesen Browser.
//!
//! ## Das Fenster ist geteilt
//!
//! Mehrere Sitzungen hängen am selben Port und damit am selben Browser. Jede
//! arbeitet deshalb in **ihrem eigenen Tab**, öffnet ihn selbst und schliesst
//! nur ihn: nie `close --all`, nie `open`/`navigate` auf einem Tab, den sie
//! nicht angelegt hat. Anlass: Eine Sitzung überschrieb einen vorhandenen
//! Tab eines anderen Vorgangs (eine liegengebliebene Peppermint-Connect-
//! Anmeldeseite). Die Meldung „Bereit" und die Beschreibung von
//! `tunnel-start` sagen das jedem Agenten, der den Tunnel benutzt.
//!
//! ## Herkunft und der eine Unterschied
//!
//! Gebaut nach `~/.local/bin/agent-tunnel` (chezmoi, Commit 365afa0) von der
//! Laptop-Sitzung. Im Harness, damit `install.sh` es überall mitbringt und es
//! ohne chezmoi und ohne imkr funktioniert — imkr wird benutzt, wenn es da
//! ist, ist aber keine Voraussetzung.
//!
//! **Abweichung:** Das Skript hält am Bildschirm Browser und Tunnel im
//! Vordergrund, Ctrl+C beendet beides. Hier läuft immer **abgekoppelt**, und
//! `tunnel stop` räumt auf. Begründung: dieser Befehl wird von einem Agenten
//! gerufen, der keine Taste drücken kann und auf eine Antwort wartet. Ein
//! Vordergrundprozess wäre dort ein hängender Aufruf. Für Menschen bleibt das
//! Skript, wie es ist.
//!
//! ## Warum ein eigenes Profil
//!
//! Der Fernsteuerungs-Port gibt **volle Kontrolle** über den Browser, der ihn
//! öffnet: jede Seite, jedes Cookie, jede angemeldete Sitzung. Deshalb nie das
//! Hauptprofil, sondern `~/.cache/agent-chrome` — und je Port ein eigenes, weil
//! zwei Tunnel zwei Browser sind.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::config;

const PORT_STANDARD: u16 = 9222;

/// Wie lange auf den Browser gewartet wird.
///
/// Das Skript wartet 15 s (75 × 0,2 s) auf den entfernten Start. Chromium
/// braucht beim ersten Lauf mit frischem Profil länger als beim zweiten,
/// deshalb hier etwas mehr Luft — ein zu kurzes Fenster meldet „geht nicht",
/// während der Browser noch hochkommt.
const WARTEN_SEKUNDEN: u64 = 25;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tat {
    Starten,
    Zustand,
    Beenden,
}

#[derive(Debug)]
struct Wunsch {
    tat: Tat,
    von: Option<String>,
    nach: Option<String>,
    port: u16,
    /// Hier am Bildschirm ausführen, nicht über den Rechner des Menschen.
    hier: bool,
}

// ---------------------------------------------------------------------------
// Umgebung
// ---------------------------------------------------------------------------

/// Sitzt hier jemand vor einem Bildschirm?
///
/// Zwei Bedingungen, und die zweite ist die wichtige: eine grafische Sitzung
/// **und** nicht über SSH angemeldet. Ohne die zweite hielte sich ein Agent,
/// der per SSH auf dem Rechner des Menschen landet, für den Menschen.
fn bildschirm_hier() -> bool {
    let grafisch = std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("DISPLAY").is_ok()
        || std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok();

    grafisch && std::env::var("SSH_CONNECTION").is_err()
}

/// Die grafische Sitzung des angemeldeten Menschen uebernehmen.
///
/// **Ohne das startet kein Browser.** Ueber SSH gibt es kein
/// `WAYLAND_DISPLAY`, und Chromium beendet sich dann sofort:
///
/// ```text
/// Failed to connect to Wayland display: No such file or directory
/// The platform failed to initialize.  Exiting.
/// ```
///
/// Am 16.10.2026 genau so gemessen, nachdem ich diesen Teil des Skripts beim
/// Portieren uebersehen hatte. Der Socket liegt in `XDG_RUNTIME_DIR` und heisst
/// `wayland-<n>`; die `.lock`-Datei daneben ist keiner.
///
/// Gibt `false` zurueck, wenn niemand grafisch angemeldet ist — dann gibt es
/// auch keinen Bildschirm, auf dem jemand zusehen koennte.
fn sitzung_uebernehmen() -> bool {
    let laufzeit = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", nutzer_nummer()));

    std::env::set_var("XDG_RUNTIME_DIR", &laufzeit);

    if let Ok(eintraege) = std::fs::read_dir(&laufzeit) {
        for eintrag in eintraege.flatten() {
            let name = eintrag.file_name().to_string_lossy().to_string();

            if !name.starts_with("wayland-") || name.ends_with(".lock") {
                continue;
            }

            std::env::set_var("WAYLAND_DISPLAY", &name);

            // `DISPLAY` fuer alles, was noch ueber X geht (XWayland).
            if std::env::var("DISPLAY").is_err() {
                std::env::set_var("DISPLAY", ":0");
            }

            return true;
        }
    }

    // Kein Wayland, aber vielleicht ein X-Server.
    if PathBuf::from("/tmp/.X11-unix/X0").exists() {
        std::env::set_var("DISPLAY", ":0");

        return true;
    }

    false
}

fn nutzer_nummer() -> u32 {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|a| String::from_utf8_lossy(&a.stdout).trim().parse().ok())
        .unwrap_or(1000)
}

/// Der SSH-Name eines Rechners zu seiner Adresse.
///
/// `SSH_CONNECTION` nennt eine IP. Ein `ssh <ip>` würde oft gehen, aber die
/// Einstellungen (Nutzername, Schlüssel, Port) hängen am **Namen** in
/// `~/.ssh/config` — deshalb wird er gesucht, statt die IP zu nehmen.
fn ssh_name_fuer(adresse: &str) -> String {
    let Ok(heim) = config::heimat() else {
        return adresse.to_string();
    };

    let Ok(text) = std::fs::read_to_string(heim.join(".ssh/config")) else {
        return adresse.to_string();
    };

    for zeile in text.lines() {
        let zeile = zeile.trim();

        if !zeile.to_lowercase().starts_with("host ") {
            continue;
        }

        for name in zeile[5..].split_whitespace() {
            // Muster wie `Host *` beschreiben keinen Rechner.
            if name.contains('*') || name.contains('?') {
                continue;
            }

            let Ok(ausgabe) = Command::new("ssh").arg("-G").arg(name).output() else {
                continue;
            };

            let aufgeloest = String::from_utf8_lossy(&ausgabe.stdout);

            for z in aufgeloest.lines() {
                if let Some(wert) = z.strip_prefix("hostname ") {
                    if wert.trim() == adresse {
                        return name.to_string();
                    }
                }
            }
        }
    }

    adresse.to_string()
}

/// Wo der Mensch sitzt, und unter welchem Namen jener Rechner diesen erreicht.
///
/// Die Reihenfolge ist Absicht und entspricht dem Skript:
///
/// 1. `--von` — was der Mensch sagt, gilt.
/// 2. `imkr operator` — weiss es, wenn er in imkr arbeitet. **Optional:** fehlt
///    imkr, geht es weiter, statt hier zu scheitern.
/// 3. `SSH_CONNECTION` — der Rechner, über den er gerade hier ist.
///
/// Findet sich nichts, wird **nicht geraten**. Ein falscher Rechner startet
/// einen Browser vor einem leeren Stuhl, und der Agent wartet auf etwas, das
/// niemand sieht.
fn betreiber(wunsch: &Wunsch) -> Result<(String, String), String> {
    let mut von = wunsch.von.clone();
    let mut nach = wunsch.nach.clone();

    if von.is_none() {
        if let Some((maschine, weg)) = aus_imkr() {
            von = Some(maschine);

            if nach.is_none() {
                nach = weg;
            }
        }
    }

    if von.is_none() {
        if let Ok(verbindung) = std::env::var("SSH_CONNECTION") {
            if let Some(adresse) = verbindung.split_whitespace().next() {
                von = Some(ssh_name_fuer(adresse));
            }
        }
    }

    let von = von.ok_or_else(|| {
        "Hier ist kein Bildschirm, und ich weiss nicht, an welchem Rechner du sitzt.\n\
         Sag mir den Rechner: ai-brain-harness tunnel start --von <rechner>  (z. B. laptop, desktop)"
            .to_string()
    })?;

    let nach = nach.unwrap_or_else(eigener_name);

    Ok((von, nach))
}

/// Unter welchem Namen ein anderer Rechner diesen hier erreicht.
///
/// Der rohe Hostname ist die naheliegende, aber nicht die beste Antwort: diese
/// Maschine heisst `omarchy` und ist im Netz als `codingmachine` bekannt. Ein
/// `ssh -R` mit dem falschen Namen scheitert an der Namensauflösung — am
/// 16.10.2026 genau so im Protokoll gelandet
/// (`Could not resolve hostname codingmachine`, weil der Name hier fehlt).
///
/// Deshalb zuerst der Name, unter dem sich dieser Rechner in imkr eingetragen
/// hat; das ist der Name, den die anderen benutzen. Erst danach der Hostname.
fn eigener_name() -> String {
    if let Ok(heim) = config::heimat() {
        let pfad = std::env::var("IMKR_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| heim.join(".config/imkr/config.toml"));

        if let Ok(text) = std::fs::read_to_string(pfad) {
            if let Some(name) = imkr_name(&text) {
                return name;
            }
        }
    }

    gethostname::gethostname().to_string_lossy().to_string()
}

/// `name = "..."` aus imkrs Konfiguration — nur auf oberster Ebene.
///
/// **Nicht der erste Treffer.** In dieser Konfiguration steht auf Zeile 18
/// `name = "codingmachine"` und auf Zeile 47, in einem `[[commands]]`-Abschnitt,
/// `name = "lazygit"`. Das Skript nimmt `head -n 1` und trifft damit heute das
/// Richtige — aber nur, solange die Reihenfolge so bleibt. Ein Tunnel nach
/// „lazygit" waere ein Fehler, den niemand sucht.
///
/// In TOML stehen Schluessel der obersten Ebene vor dem ersten Abschnitt. Also
/// wird bei der ersten `[`-Zeile abgebrochen.
fn imkr_name(text: &str) -> Option<String> {
    for zeile in text.lines() {
        let zeile = zeile.trim();

        if zeile.starts_with('[') {
            break;
        }

        let Some(rest) = zeile.strip_prefix("name") else {
            continue;
        };

        let Some(wert) = rest.trim_start().strip_prefix('=') else {
            continue;
        };

        let wert = wert.trim().trim_matches('"').trim();

        if !wert.is_empty() {
            return Some(wert.to_string());
        }
    }

    None
}

/// `imkr operator --json` lesen, falls imkr da ist.
fn aus_imkr() -> Option<(String, Option<String>)> {
    let ausgabe = Command::new("imkr")
        .args(["operator", "--json"])
        .output()
        .ok()?;

    if !ausgabe.status.success() {
        return None;
    }

    imkr_lesen(&String::from_utf8_lossy(&ausgabe.stdout))
}

/// Die Antwort von `imkr operator --json` auswerten.
///
/// Getrennt vom Aufruf, damit sie ohne imkr pruefbar ist — und weil imkr hier
/// ausdruecklich **optional** ist: fehlt es, geht die Suche weiter, statt zu
/// scheitern.
fn imkr_lesen(roh: &str) -> Option<(String, Option<String>)> {
    let wert: serde_json::Value = serde_json::from_str(roh).ok()?;

    // `here: true` heisst, der Mensch sitzt an DIESEM Rechner — dann ist nichts
    // zu tunneln, und der Aufrufer behandelt das ueber `bildschirm_hier`.
    let maschine = wert.get("machine")?.as_str()?.trim().to_string();

    if maschine.is_empty() {
        return None;
    }

    Some((
        maschine,
        wert.get("via")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string),
    ))
}

// ---------------------------------------------------------------------------
// Ablage je Port
// ---------------------------------------------------------------------------

fn cache() -> Result<PathBuf, String> {
    let pfad = config::heimat()?.join(".cache");

    std::fs::create_dir_all(&pfad)
        .map_err(|e| format!("{} ist nicht anlegbar: {e}", pfad.display()))?;

    Ok(pfad)
}

/// Der Profilname zu einem Port — reine Namensbildung, ohne Dateisystem.
///
/// Getrennt vom Pfad, damit der Test ihn pruefen kann, **ohne** `HOME` zu
/// lesen oder einen Ordner anzulegen. Rust testet in Threads, und
/// `std::env::set_var` ist prozessweit: ein anderer Test in dieser Suite setzt
/// `HOME` um, und ein Pfadtest daneben wurde dadurch unregelmaessig rot. Ein
/// Test, der die Umgebung nicht braucht, kann an ihr auch nicht scheitern.
fn profil_name(port: u16) -> String {
    if port == PORT_STANDARD {
        "agent-chrome".to_string()
    } else {
        format!("agent-chrome-{port}")
    }
}

fn merker_name(port: u16, was: &str) -> String {
    format!("agent-tunnel-{port}.{was}")
}

fn profil(port: u16) -> Result<PathBuf, String> {
    Ok(cache()?.join(profil_name(port)))
}

fn merker(port: u16, was: &str) -> Result<PathBuf, String> {
    Ok(cache()?.join(merker_name(port, was)))
}

fn pid_lesen(port: u16, was: &str) -> Option<u32> {
    let pfad = merker(port, was).ok()?;
    let text = std::fs::read_to_string(pfad).ok()?;

    text.trim().parse().ok()
}

/// Läuft der Prozess, dessen Nummer im Merker steht?
fn laeuft(port: u16, was: &str) -> bool {
    let Some(pid) = pid_lesen(port, was) else {
        return false;
    };

    PathBuf::from(format!("/proc/{pid}")).exists()
}

/// Was `status` ueber den Tunnel in seine Antwort nimmt (#9059).
#[derive(serde::Serialize)]
pub struct TunnelKurz {
    pub port: u16,
    pub browser: bool,
    pub ssh: bool,
    /// `laeuft`, `halb` (nur eine Haelfte) oder `aus`.
    pub grund: &'static str,
}

/// Der Stand des Tunnels — aus den Merkern, ohne den Port anzuklopfen.
///
/// **Bewusst ohne `browser_antwortet`.** Die HTTP-Probe ist die genauere
/// Auskunft (sie findet auch einen Browser aus einer fremden Sitzung), aber sie
/// kostet eine Verbindung mit zwei Sekunden Frist — in einer Anzeige, die alle
/// 30 Sekunden gelesen wird. Wer es genau wissen will, ruft `tunnel status`;
/// dort steht beides nebeneinander.
pub fn kurzstand() -> TunnelKurz {
    let port = PORT_STANDARD;
    let browser = laeuft(port, "browser");
    let ssh = laeuft(port, "ssh");

    TunnelKurz {
        port,
        browser,
        ssh,
        grund: match (browser, ssh) {
            (true, true) => "laeuft",
            (false, false) => "aus",
            _ => "halb",
        },
    }
}

fn beenden(port: u16, was: &str) -> bool {
    let Some(pid) = pid_lesen(port, was) else {
        return false;
    };

    // Die Ausgabe von `kill` wird verworfen: ein Prozess, der schon weg ist,
    // ist hier kein Fehler, und „No such process" neben der eigenen Meldung
    // liest sich wie einer.
    let getroffen = Command::new("kill")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let _ = merker(port, was).map(std::fs::remove_file);

    getroffen
}

// ---------------------------------------------------------------------------
// Der Port
// ---------------------------------------------------------------------------

/// Antwortet auf dem Port ein Browser?
///
/// Nicht „ist der Port offen": ein offener Port ohne Browser dahinter (ein
/// Tunnel, dessen Gegenstelle weg ist) sähe sonst wie Erfolg aus. Deshalb die
/// Frage, die nur ein Browser beantworten kann.
fn browser_antwortet(port: u16) -> bool {
    ureq::get(&format!("http://127.0.0.1:{port}/json/version"))
        .timeout(std::time::Duration::from_secs(2))
        .call()
        .is_ok()
}

fn warten_bis_antwortet(port: u16) -> bool {
    for _ in 0..(WARTEN_SEKUNDEN * 5) {
        if browser_antwortet(port) {
            return true;
        }

        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    false
}

// ---------------------------------------------------------------------------
// Hier am Bildschirm
// ---------------------------------------------------------------------------

fn chromium() -> Result<String, String> {
    for name in ["chromium", "google-chrome-stable", "google-chrome"] {
        if let Ok(ausgabe) = Command::new("command").args(["-v", name]).output() {
            if ausgabe.status.success() {
                return Ok(name.to_string());
            }
        }

        // `command -v` ist in manchen Umgebungen kein eigenes Programm.
        if let Ok(ausgabe) = Command::new("which").arg(name).output() {
            if ausgabe.status.success() {
                return Ok(name.to_string());
            }
        }
    }

    Err("Kein Chromium und kein Chrome gefunden.".to_string())
}

/// Ein liegengebliebenes Schloss im eigenen Profil wegnehmen.
///
/// Chromium fragt sonst „Unlock profile and relaunch" und startet nicht. Das
/// Profil gehört allein diesem Befehl — ein Schloss ohne den zugehörigen
/// Prozess ist also Abfall, kein fremdes Eigentum.
fn schloss_aufraeumen(port: u16) {
    let Ok(profil) = profil(port) else {
        return;
    };

    let schloss = profil.join("SingletonLock");

    let Ok(ziel) = std::fs::read_link(&schloss) else {
        return;
    };

    let ziel = ziel.to_string_lossy().to_string();

    // Die Marke lautet `<hostname>-<pid>`.
    let (rechner, pid) = match ziel.rsplit_once('-') {
        Some((r, p)) => (r.to_string(), p.to_string()),
        None => return,
    };

    let eigener = gethostname::gethostname().to_string_lossy().to_string();
    let lebt = pid
        .parse::<u32>()
        .map(|p| PathBuf::from(format!("/proc/{p}")).exists())
        .unwrap_or(false);

    if rechner != eigener || !lebt {
        for name in ["SingletonLock", "SingletonCookie", "SingletonSocket"] {
            let _ = std::fs::remove_file(profil.join(name));
        }
    }
}

/// Einen Prozess starten, der den Aufruf ueberlebt.
///
/// **`setsid`, nicht nur `&`.** Wird der Befehl ueber SSH gerufen, schickt der
/// SSH-Dienst beim Schliessen des Kanals ein SIGHUP an die ganze Prozessgruppe
/// — der Browser waere weg, sobald die Verbindung endet. Eine eigene Sitzung
/// nimmt ihn aus dieser Gruppe heraus. Das Skript macht es mit `setsid nohup`.
///
/// **Die Ausgabe geht in ein Protokoll, nicht nach /dev/null.** Beim ersten
/// Fernlauf am 16.10.2026 beendete sich Chromium sofort („Failed to initialize
/// Wayland platform") — und weil die Ausgabe verworfen wurde, war davon nichts
/// zu sehen. Die Meldung lautete nur „antwortet nach 25 s nicht", was auf eine
/// langsame Maschine hindeutet statt auf den echten Grund.
fn abgekoppelt(programm: &str, argumente: &[String], port: u16, was: &str) -> Result<u32, String> {
    let protokoll = merker(port, &format!("{was}.log"))?;

    let datei = std::fs::File::create(&protokoll)
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", protokoll.display()))?;

    let fehler = datei
        .try_clone()
        .map_err(|e| format!("Protokoll laesst sich nicht doppeln: {e}"))?;

    let kind = Command::new("setsid")
        .arg(programm)
        .args(argumente)
        .stdin(Stdio::null())
        .stdout(Stdio::from(datei))
        .stderr(Stdio::from(fehler))
        .spawn()
        .map_err(|e| format!("{was} liess sich nicht starten: {e}"))?;

    let pid = kind.id();

    let pfad = merker(port, was)?;
    std::fs::write(&pfad, format!("{pid}\n"))
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", pfad.display()))?;

    Ok(pid)
}

/// Was im Protokoll steht — fuer die Fehlermeldung.
fn protokoll_schwanz(port: u16, was: &str) -> String {
    let Ok(pfad) = merker(port, &format!("{was}.log")) else {
        return String::new();
    };

    let Ok(text) = std::fs::read_to_string(&pfad) else {
        return String::new();
    };

    let zeilen: Vec<&str> = text.lines().filter(|z| !z.trim().is_empty()).collect();
    let letzte: Vec<&str> = zeilen.iter().rev().take(3).rev().copied().collect();

    if letzte.is_empty() {
        return String::new();
    }

    format!("\n{}", letzte.join("\n"))
}

/// Die Regel für das geteilte Fenster — steht in jeder Meldung, nach der ein
/// Agent den Browser benutzt, denn mehrere Sitzungen hängen am selben.
fn geteilt(port: u16) -> String {
    format!(
        "Das Fenster ist geteilt — arbeite nur in einem EIGENEN Tab:\n\
         \x20 agent-browser --cdp {port} --pin-tab tab new <url> … am Ende tab close\n\
         \x20 Playwright: browser_tabs new/close · DevTools: new_page/select_page/close_page\n\
         Nie close --all, nie open/navigate auf einem Tab, den du nicht selbst geoeffnet hast."
    )
}

fn hier_starten(nach: &str, port: u16) -> Result<String, String> {
    if browser_antwortet(port) && laeuft(port, "ssh") {
        return Ok(format!(
            "Steht schon: 127.0.0.1:{port} zeigt auf einen Browser.\n{}",
            geteilt(port)
        ));
    }

    // **Ein fremder Browser auf dem Port ist kein Geschenk.**
    // Antwortet dort etwas, das nicht von uns stammt, wuerde der Tunnel den
    // Browser eines anderen durchreichen — und der Agent dahinter steuerte
    // fremde Tabs mit fremden angemeldeten Sitzungen. Auf diesem Rechner ist
    // das der Normalfall, nicht die Ausnahme: parallele Sitzungen halten
    // eigene headless-Chromiums. Am 16.10.2026 lag auf 9333 genau so einer.
    if browser_antwortet(port) && !laeuft(port, "browser") {
        return Err(format!(
            "Auf Port {port} antwortet schon ein Browser, der nicht von diesem Befehl stammt \
             (eine andere Sitzung?).\n\
             Einen anderen Port nehmen: ai-brain-harness tunnel start --port <n>"
        ));
    }

    let profil = profil(port)?;

    if !browser_antwortet(port) {
        // Ueber SSH gerufen gibt es keinen Bildschirm in der Umgebung — der
        // des angemeldeten Menschen muss uebernommen werden, sonst beendet
        // sich Chromium sofort.
        if !bildschirm_hier() && !sitzung_uebernehmen() {
            return Err(format!(
                "Auf {} ist niemand grafisch angemeldet — es gibt keinen Bildschirm, \
                 auf dem jemand zusehen koennte.",
                gethostname::gethostname().to_string_lossy()
            ));
        }

        schloss_aufraeumen(port);

        let programm = chromium()?;

        abgekoppelt(
            &programm,
            &[
                format!("--remote-debugging-port={port}"),
                format!("--user-data-dir={}", profil.display()),
                "--no-first-run".to_string(),
                "--no-default-browser-check".to_string(),
                "about:blank".to_string(),
            ],
            port,
            "browser",
        )?;

        if !warten_bis_antwortet(port) {
            let grund = protokoll_schwanz(port, "browser");
            beenden(port, "browser");

            return Err(format!(
                "Der Browser antwortet nach {WARTEN_SEKUNDEN} s nicht auf Port {port}.{grund}"
            ));
        }
    }

    // Eigene Verbindung statt des gemeinsamen Masters: eine Weiterleitung, die
    // an einem fremden Master haengt, verschwindet mit ihm.
    abgekoppelt(
        "ssh",
        &[
            "-N".to_string(),
            "-o".to_string(),
            "ControlMaster=no".to_string(),
            "-o".to_string(),
            "ControlPath=none".to_string(),
            "-o".to_string(),
            "ExitOnForwardFailure=yes".to_string(),
            "-o".to_string(),
            "ServerAliveInterval=30".to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-R".to_string(),
            format!("{port}:127.0.0.1:{port}"),
            nach.to_string(),
        ],
        port,
        "ssh",
    )?;

    // Kurz nachsehen, ob die Weiterleitung haelt. `ExitOnForwardFailure` laesst
    // ssh sonst still enden, und dann stuende hier „gestartet", waehrend auf
    // der anderen Seite nichts ankommt.
    std::thread::sleep(std::time::Duration::from_millis(1200));

    if !laeuft(port, "ssh") {
        let grund = protokoll_schwanz(port, "ssh");

        return Err(format!(
            "Der Rueckwaertstunnel nach {nach} hat nicht gehalten — \
             ist Port {port} dort belegt?{grund}"
        ));
    }

    Ok(format!(
        "Browser von {} laeuft, Tunnel nach {nach}:127.0.0.1:{port} steht.",
        gethostname::gethostname().to_string_lossy()
    ))
}

fn hier_zustand(port: u16) -> Result<String, String> {
    let rechner = gethostname::gethostname().to_string_lossy().to_string();
    let browser = laeuft(port, "browser");
    let ssh = laeuft(port, "ssh");

    // **Der Port ist die Wahrheit, die Merker sind nur Buchhaltung.**
    // Am 16.10.2026 meldete der Zustand „laeuft nicht", waehrend auf dem Port
    // ein Browser antwortete — ein Widerspruch, der den Leser an der falschen
    // Stelle suchen laesst. Beides wird genannt, und wenn es auseinandergeht,
    // steht auch das da.
    let antwortet = browser_antwortet(port);

    let mut text = format!(
        "Auf {rechner}, Port {port}:\n  Browser: {}\n  Tunnel : {}\n  Port   : {}\n",
        if browser { "laeuft" } else { "laeuft nicht" },
        if ssh { "laeuft" } else { "laeuft nicht" },
        if antwortet { "ein Browser antwortet" } else { "nichts antwortet" },
    );

    if antwortet && !browser {
        text.push_str(
            "\n  Der Browser dort stammt nicht von diesem Befehl — vermutlich eine andere \
             Sitzung oder ein Tunnel aus dem alten Skript.\n",
        );
    }

    if !browser && !ssh && !antwortet {
        return Err(text + "\nHier laeuft kein Tunnel.");
    }

    Ok(text)
}

/// Laeufe zu einem Port finden, fuer die es keinen Merker gibt.
///
/// **Warum das noetig ist.** Ein Tunnel, den das alte Shell-Skript gestartet
/// hat, fuehrt keinen Merker — `stop` meldete dann „lief nicht", waehrend auf
/// dem Port weiter ein Browser antwortete und ihn fuer alle blockierte. Am
/// 16.10.2026 auf dem Laptop genau so passiert (Port 9222).
///
/// Gesucht wird eng: nur Prozesse, die diesen Port in der Befehlszeile tragen,
/// und nur die beiden Formen, die ein Tunnel hat. `pgrep -f` wuerde sonst auch
/// den eigenen Aufruf finden — das Muster steht ja darin.
fn fremde_laeufe(port: u16) -> Vec<u32> {
    let muster = [
        format!("remote-debugging-port={port}"),
        format!("{port}:127.0.0.1:{port}"),
    ];

    let mut gefunden = Vec::new();
    let eigen = std::process::id();

    for m in muster {
        let Ok(ausgabe) = Command::new("pgrep").args(["-f", &m]).output() else {
            continue;
        };

        for zeile in String::from_utf8_lossy(&ausgabe.stdout).lines() {
            if let Ok(pid) = zeile.trim().parse::<u32>() {
                if pid != eigen && !gefunden.contains(&pid) {
                    gefunden.push(pid);
                }
            }
        }
    }

    gefunden
}

fn hier_beenden(port: u16) -> Result<String, String> {
    let ssh = beenden(port, "ssh");
    let browser = beenden(port, "browser");

    let mut text = match (ssh, browser) {
        (false, false) => String::new(),
        _ => format!(
            "Beendet: {}{}{}\n",
            if ssh { "Tunnel" } else { "" },
            if ssh && browser { " und " } else { "" },
            if browser { "Browser" } else { "" }
        ),
    };

    // Jetzt die ohne Merker — etwa aus dem alten Skript.
    if browser_antwortet(port) || (!ssh && !browser) {
        let fremde = fremde_laeufe(port);

        if !fremde.is_empty() {
            for pid in &fremde {
                let _ = Command::new("kill")
                    .arg(pid.to_string())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }

            std::thread::sleep(std::time::Duration::from_millis(600));

            text.push_str(&format!(
                "Ausserdem {} Lauf/Laeufe ohne Merker beendet (altes Skript?).\n",
                fremde.len()
            ));
        }
    }

    if text.is_empty() {
        return Ok("Hier lief kein Tunnel.".to_string());
    }

    if browser_antwortet(port) {
        text.push_str(&format!(
            "Achtung: auf Port {port} antwortet weiter ein Browser — der gehoert jemand anderem.\n"
        ));
    }

    Ok(text)
}

// ---------------------------------------------------------------------------
// Von hier aus über den Rechner des Menschen
// ---------------------------------------------------------------------------

/// Einen `tunnel`-Befehl auf dem Rechner des Menschen ausfuehren.
///
/// **Die Ausgabe wird eingefangen, nie durchgereicht.** Am 16.10.2026 hat mich
/// genau das eine halbe Stunde gekostet: `.status()` erbt stdout und stderr,
/// und als der Laptop einen Harness ohne `tunnel` hatte, erschien dessen
/// „Unbekannter Befehl: tunnel" samt Hilfetext **hier** — als haette der
/// Befehl auf diesem Rechner nicht existiert. Ich habe den lokalen Dispatch
/// zerlegt, bis eine Marke im Code bewies, dass er laeuft.
///
/// Deshalb: einfangen, und einen veralteten Harness auf der Gegenseite als
/// genau das melden.
fn fern(von: &str, befehl: &str, port: u16) -> Result<String, String> {
    let ausgabe = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            von,
            &format!("ai-brain-harness tunnel {befehl} --hier --port '{port}'"),
        ])
        .output()
        .map_err(|e| format!("ssh nach {von} liess sich nicht starten: {e}"))?;

    let fehlertext = String::from_utf8_lossy(&ausgabe.stderr).trim().to_string();
    let text = String::from_utf8_lossy(&ausgabe.stdout).to_string();

    if fehlertext.contains("Unbekannter Befehl") {
        return Err(format!(
            "Auf {von} laeuft ein Harness, der 'tunnel' noch nicht kennt.\n\
             Dort aktualisieren: curl -fsSL <instanz>/install.sh | sh"
        ));
    }

    if !ausgabe.status.success() {
        return Err(format!(
            "Auf {von} ging es nicht: {}",
            if fehlertext.is_empty() { text.trim() } else { &fehlertext }
        ));
    }

    Ok(text)
}

fn ueber_betreiber(wunsch: &Wunsch) -> Result<String, String> {
    let (von, nach) = betreiber(wunsch)?;
    let port = wunsch.port;

    match wunsch.tat {
        Tat::Starten => {
            // **Ein antwortender Port ist noch kein eigener Tunnel.**
            // Hier stand nur `browser_antwortet`, und damit galt jeder fremde
            // Browser als Erfolg: am 16.10.2026 meldete `tunnel start` auf dem
            // Standardport 9222 „Steht schon", wo in Wirklichkeit der Tunnel
            // des alten Skripts von gestern lag (ohne Merker). Der Agent haette
            // dort fremde Tabs gesteuert — denselben Riegel hat der lokale Weg
            // schon, dem Fernweg fehlte er.
            //
            // Gefragt wird der Rechner des Menschen: nur wenn DORT unser
            // Browser und unser Tunnel laufen, gehoert der Port uns.
            if browser_antwortet(port) {
                let bericht = fern(&von, "status", port).unwrap_or_default();

                if bericht.contains("Browser: laeuft") && bericht.contains("Tunnel : laeuft") {
                    return Ok(format!(
                        "Steht schon: 127.0.0.1:{port} zeigt auf den Browser von {von}.\n{}",
                        geteilt(port)
                    ));
                }

                return Err(format!(
                    "Auf Port {port} antwortet schon ein Browser, aber auf {von} laeuft kein \
                     Tunnel dieses Befehls — es ist ein fremder (altes Skript, andere Sitzung).\n\
                     Einen anderen Port nehmen: ai-brain-harness tunnel start --port <n>"
                ));
            }

            // **Kein Fortschrittsgeschwaetz auf stdout.** Im MCP-Modus ist
            // stdout der JSON-RPC-Kanal; eine Zwischenmeldung dort zerstoert
            // das Protokoll. Was der Mensch sehen soll, steht im Ergebnis.
            eprintln!("Starte den Browser auf {von}, Tunnel nach {nach}:{port} …");

            fern(&von, &format!("start --nach '{nach}'"), port)?;

            if !warten_bis_antwortet(port) {
                return Err(format!(
                    "Der Tunnel steht nicht. Auf {von} nachsehen: \
                     ai-brain-harness tunnel status --port {port}"
                ));
            }

            Ok(format!(
                "Bereit: der Browser von {von} ist hier unter 127.0.0.1:{port}.\n{}\n\
                 Am Ende schliessen mit: tunnel-stop (oder ai-brain-harness tunnel stop)",
                geteilt(port)
            ))
        }
        Tat::Zustand | Tat::Beenden => {
            let tat = if wunsch.tat == Tat::Zustand { "status" } else { "stop" };
            let ausgabe = fern(&von, tat, port)?;

            if wunsch.tat == Tat::Zustand && !browser_antwortet(port) {
                return Err(format!(
                    "{ausgabe}\nHier antwortet auf Port {port} kein Browser — \
                     der Tunnel von {von} steht nicht."
                ));
            }

            Ok(ausgabe)
        }
    }
}

// ---------------------------------------------------------------------------
// Befehlszeile
// ---------------------------------------------------------------------------

const HILFE: &str = "\
ai-brain-harness tunnel — den Browser deines Rechners an einen Agenten geben

  tunnel start    Browser auf deinem Rechner starten, hierher tunneln
  tunnel status   Steht er?
  tunnel stop     Wieder schliessen

Zusaetze:
  --von <rechner>  An welchem Rechner du sitzt (sonst: imkr operator, sonst
                   die SSH-Verbindung, ueber die du hier bist)
  --nach <host>    Unter welchem Namen dein Rechner diesen erreicht
                   (Vorgabe: der Hostname dieses Rechners)
  --port <n>       Vorgabe 9222. Je Port ein eigener Browser und ein eigenes Profil.
  --hier           Nicht ueber deinen Rechner gehen, sondern hier ausfuehren.
                   Setzt der Fernaufruf selbst; von Hand nur am Bildschirm sinnvoll.

Wofuer: Zum Pruefen und Testen genuegt der Browser hier (agent-browser,
headless). Dieser Befehl ist dafuer, dass DU etwas siehst — zusehen, einloggen,
ein Captcha loesen.

Der Port gibt volle Kontrolle ueber den Browser, der ihn oeffnet. Deshalb nie
dein Hauptprofil, sondern ein eigenes unter ~/.cache/agent-chrome.
";

fn lesen(args: &[String]) -> Result<Wunsch, String> {
    let mut wunsch = Wunsch {
        tat: Tat::Starten,
        von: None,
        nach: None,
        port: PORT_STANDARD,
        hier: false,
    };

    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "start" => wunsch.tat = Tat::Starten,
            "status" | "zustand" => wunsch.tat = Tat::Zustand,
            "stop" | "beenden" => wunsch.tat = Tat::Beenden,
            "--hier" => wunsch.hier = true,
            "--von" | "--from" => {
                i += 1;
                wunsch.von = args.get(i).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
            }
            "--nach" | "--to" => {
                i += 1;
                wunsch.nach = args.get(i).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
            }
            "--port" => {
                i += 1;

                let roh = args
                    .get(i)
                    .ok_or_else(|| "'--port' braucht eine Nummer.".to_string())?;

                wunsch.port = roh
                    .parse()
                    .map_err(|_| format!("'{roh}' ist keine Portnummer."))?;
            }
            anders => {
                return Err(format!(
                    "Unbekannte Angabe '{anders}'. Siehe: ai-brain-harness tunnel --help"
                ));
            }
        }

        i += 1;
    }

    Ok(wunsch)
}

pub fn verwalten(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HILFE}");
        return Ok(());
    }

    let wunsch = lesen(args)?;

    // Wer am Bildschirm sitzt, braucht keinen Umweg über sich selbst.
    if wunsch.hier || bildschirm_hier() {
        let nach = wunsch
            .nach
            .clone()
            .unwrap_or_else(|| "codingmachine".to_string());

        let bericht = match wunsch.tat {
            Tat::Starten => hier_starten(&nach, wunsch.port),
            Tat::Zustand => hier_zustand(wunsch.port),
            Tat::Beenden => hier_beenden(wunsch.port),
        }?;

        println!("{}", bericht.trim_end());

        return Ok(());
    }

    println!("{}", ueber_betreiber(&wunsch)?.trim_end());

    Ok(())
}

// ---------------------------------------------------------------------------
// Als MCP-Werkzeuge
// ---------------------------------------------------------------------------

/// Die drei Werkzeuge, die jeder Agent rufen kann.
///
/// **Warum im Proxy und nicht bei den Rechnerwerkzeugen.** `rechner-mcp` bekommt
/// nur die Maske; eine gewoehnliche Claude-Code-Sitzung auf diesem Rechner sieht
/// ausschliesslich den `ai-brain`-Server. Wer „oeffne den Tunnel" sagen will,
/// ohne zu wissen, in welcher Huelle er sitzt, muss das Werkzeug also dort
/// finden. Und der Riegel von `rechner.rs` passt hier nicht: er trennt lesen von
/// aendern, aber den eigenen Browser auf dem eigenen Rechner zu oeffnen ist
/// keine Rechteerhoehung — es ist genau das, was der Mensch gerade verlangt.
pub fn werkzeuge() -> Vec<serde_json::Value> {
    let von = serde_json::json!({
        "type": "string",
        "description": "An welchem Rechner der Mensch sitzt (z. B. laptop, desktop). \
                        Weglassen, solange er es nicht selbst nennt — der Befehl findet ihn."
    });

    let port = serde_json::json!({
        "type": "integer",
        "description": "Vorgabe 9222. Nur aendern, wenn dort schon ein fremder Browser antwortet."
    });

    vec![
        serde_json::json!({
            "name": "tunnel-start",
            "description": "Oeffnet den Browser des Menschen und tunnelt ihn hierher. \
                            Dafuer, dass DER MENSCH etwas sieht — zusehen, einloggen, Captcha. \
                            Zum eigenen Pruefen und Testen genuegt agent-browser headless, \
                            dafuer ist das hier nicht gedacht. Danach zeigt 127.0.0.1:<port> \
                            auf jenen Browser (agent-browser --cdp, playwright-tunnel, \
                            chrome-devtools-tunnel). Das Fenster ist geteilt: nur in einem \
                            eigenen, selbst geoeffneten Tab arbeiten und nur ihn schliessen, \
                            nie close --all, nie vorhandene Tabs navigieren.",
            "inputSchema": {
                "type": "object",
                "properties": { "von": von, "port": port },
            },
            "annotations": { "readOnlyHint": false },
        }),
        serde_json::json!({
            "name": "tunnel-status",
            "description": "Steht der Tunnel? Nennt Browser, Tunnel und ob auf dem Port \
                            ueberhaupt etwas antwortet — die drei koennen auseinandergehen.",
            "inputSchema": {
                "type": "object",
                "properties": { "port": port.clone() },
            },
            "annotations": { "readOnlyHint": true },
        }),
        serde_json::json!({
            "name": "tunnel-stop",
            "description": "Schliesst den Tunnel und den Browser wieder. Gehoert ans Ende \
                            jedes Zeigens — ein offener Fernsteuerungs-Port gibt volle \
                            Kontrolle ueber diesen Browser.",
            "inputSchema": {
                "type": "object",
                "properties": { "von": von.clone(), "port": port.clone() },
            },
            "annotations": { "readOnlyHint": false },
        }),
    ]
}

/// Einen Werkzeugaufruf bedienen — `None`, wenn er uns nicht gehoert.
pub fn fuer_mcp(name: &str, argumente: &serde_json::Value) -> Option<Result<String, String>> {
    let tat = match name {
        "tunnel-start" => Tat::Starten,
        "tunnel-status" => Tat::Zustand,
        "tunnel-stop" => Tat::Beenden,
        _ => return None,
    };

    let wunsch = Wunsch {
        tat,
        von: argumente
            .get("von")
            .and_then(|v| v.as_str())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        nach: None,
        port: argumente
            .get("port")
            .and_then(|v| v.as_u64())
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or(PORT_STANDARD),
        hier: false,
    };

    // Dieselbe Weggabelung wie auf der Befehlszeile: wer am Bildschirm sitzt,
    // braucht keinen Umweg über sich selbst.
    if bildschirm_hier() {
        let nach = "codingmachine".to_string();

        return Some(match wunsch.tat {
            Tat::Starten => hier_starten(&nach, wunsch.port),
            Tat::Zustand => hier_zustand(wunsch.port),
            Tat::Beenden => hier_beenden(wunsch.port),
        });
    }

    Some(ueber_betreiber(&wunsch))
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Befehlszeile --------------------------------------------------------

    fn gelesen(args: &[&str]) -> Wunsch {
        lesen(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>()).expect("lesbar")
    }

    #[test]
    fn ohne_angabe_wird_gestartet() {
        let w = gelesen(&[]);

        assert!(w.tat == Tat::Starten);
        assert_eq!(w.port, PORT_STANDARD);
        assert!(!w.hier);
        assert!(w.von.is_none());
    }

    #[test]
    fn die_drei_taten_werden_erkannt() {
        assert!(gelesen(&["start"]).tat == Tat::Starten);
        assert!(gelesen(&["status"]).tat == Tat::Zustand);
        assert!(gelesen(&["stop"]).tat == Tat::Beenden);
    }

    #[test]
    fn die_englischen_flaggen_des_skripts_gelten_weiter() {
        // `agent-tunnel` kennt --from/--to. Wer das Skript gewohnt ist, soll
        // nicht an der Uebersetzung scheitern.
        let w = gelesen(&["start", "--from", "laptop", "--to", "codingmachine"]);

        assert_eq!(w.von.as_deref(), Some("laptop"));
        assert_eq!(w.nach.as_deref(), Some("codingmachine"));
    }

    #[test]
    fn ein_port_wird_gelesen() {
        assert_eq!(gelesen(&["start", "--port", "9333"]).port, 9333);
    }

    #[test]
    fn ein_unfug_port_wird_abgelehnt() {
        let fehler = lesen(&["--port".to_string(), "neunzwei".to_string()]).expect_err("muss scheitern");

        assert!(fehler.contains("keine Portnummer"), "{fehler}");
    }

    #[test]
    fn eine_unbekannte_angabe_wird_abgelehnt() {
        // Still schlucken waere schlimmer: ein vertippter Rechnername wuerde
        // sonst ignoriert und der Tunnel landete woanders.
        let fehler = lesen(&["--vonn".to_string()]).expect_err("muss scheitern");

        assert!(fehler.contains("Unbekannte Angabe"), "{fehler}");
    }

    #[test]
    fn leere_werte_gelten_als_nicht_angegeben() {
        let w = gelesen(&["start", "--von", "  "]);

        assert!(w.von.is_none(), "ein leerer Rechnername ist keine Angabe");
    }

    // -- imkr ist optional ---------------------------------------------------

    #[test]
    fn der_eigene_name_kommt_aus_imkrs_konfiguration() {
        // Diese Maschine heisst `omarchy` und ist als `codingmachine`
        // erreichbar. Der Hostname waere die falsche Antwort — ein `ssh -R`
        // damit scheitert an der Namensauflösung.
        // So sieht die echte Datei aus: der Rechnername auf oberster Ebene,
        // vor dem ersten Abschnitt.
        assert_eq!(
            imkr_name("port = 7\nname = \"codingmachine\"\n\n[which_key]").as_deref(),
            Some("codingmachine")
        );
        assert_eq!(imkr_name("name=\"laptop\"").as_deref(), Some("laptop"));
        assert!(imkr_name("kein name hier").is_none());
        assert!(imkr_name("name = \"\"").is_none(), "leer ist keine Angabe");

        // Der Fall aus der echten Konfiguration: der Rechnername steht oben,
        // ein gleichnamiger Schluessel in einem Abschnitt weiter unten. Wer
        // den ersten Treffer OHNE Abschnittsgrenze nimmt, trifft hier noch
        // richtig — aber nur zufaellig.
        assert_eq!(
            imkr_name("name = \"codingmachine\"\n[[commands]]\nname = \"lazygit\"").as_deref(),
            Some("codingmachine")
        );
        assert!(
            imkr_name("[[commands]]\nname = \"lazygit\"").is_none(),
            "ein Name INNERHALB eines Abschnitts ist nicht der Rechnername"
        );
    }

    #[test]
    fn imkr_antwort_wird_gelesen() {
        let (maschine, weg) =
            imkr_lesen("{\"here\": false, \"machine\": \"laptop\", \"via\": \"codingmachine\"}")
                .expect("lesbar");

        assert_eq!(maschine, "laptop");
        assert_eq!(weg.as_deref(), Some("codingmachine"));
    }

    #[test]
    fn imkr_ohne_via_liefert_nur_die_maschine() {
        let (maschine, weg) = imkr_lesen("{\"machine\": \"desktop\"}").expect("lesbar");

        assert_eq!(maschine, "desktop");
        assert!(weg.is_none(), "ohne via entscheidet der eigene Hostname");
    }

    #[test]
    fn unlesbare_imkr_antwort_blockiert_nicht() {
        // Der Fall „imkr ist nicht installiert" oder „andere Fassung": die
        // Suche muss WEITERGEHEN (SSH_CONNECTION), nicht scheitern.
        assert!(imkr_lesen("").is_none());
        assert!(imkr_lesen("kein json").is_none());
        assert!(imkr_lesen("{\"machine\": \"\"}").is_none());
        assert!(imkr_lesen("{\"here\": true}").is_none());
    }

    // -- Ablage je Port ------------------------------------------------------

    #[test]
    fn der_standardport_hat_das_profil_ohne_nummer() {
        assert_eq!(profil_name(PORT_STANDARD), "agent-chrome");
    }

    #[test]
    fn ein_zweiter_port_bekommt_ein_eigenes_profil() {
        // Zwei Tunnel sind zwei Browser. Dasselbe Profil hiesse, dass der
        // zweite Chromium den ersten um sein Schloss bittet.
        assert_ne!(profil_name(PORT_STANDARD), profil_name(9333));
        assert_eq!(profil_name(9333), "agent-chrome-9333");
    }

    #[test]
    fn merker_trennen_port_und_sache() {
        assert_ne!(merker_name(9222, "browser"), merker_name(9222, "ssh"));
        assert_ne!(merker_name(9222, "ssh"), merker_name(9333, "ssh"));
        assert_eq!(merker_name(9222, "ssh"), "agent-tunnel-9222.ssh");
    }

    // -- Bildschirmerkennung -------------------------------------------------

    #[test]
    fn ueber_ssh_gilt_kein_bildschirm_als_eigener() {
        // Der Kern der Erkennung: ein Agent, der per SSH auf dem Rechner des
        // Menschen landet, darf sich nicht fuer den Menschen halten — sonst
        // startet er den Browser auf dem falschen Bildschirm.
        let vorher = std::env::var("SSH_CONNECTION").ok();
        let vorher_wayland = std::env::var("WAYLAND_DISPLAY").ok();

        std::env::set_var("WAYLAND_DISPLAY", "wayland-1");
        std::env::set_var("SSH_CONNECTION", "192.168.1.5 1234 192.168.1.9 22");

        assert!(!bildschirm_hier());

        std::env::remove_var("SSH_CONNECTION");
        assert!(bildschirm_hier(), "ohne SSH und mit Wayland sitzt hier jemand");

        match vorher {
            Some(v) => std::env::set_var("SSH_CONNECTION", v),
            None => std::env::remove_var("SSH_CONNECTION"),
        }
        match vorher_wayland {
            Some(v) => std::env::set_var("WAYLAND_DISPLAY", v),
            None => std::env::remove_var("WAYLAND_DISPLAY"),
        }
    }

    // -- Betreiber -----------------------------------------------------------

    #[test]
    fn ein_fremder_browser_auf_dem_port_wird_abgelehnt() {
        // Auf diesem Rechner halten parallele Sitzungen eigene
        // headless-Chromiums. Antwortet so einer auf dem Port, darf der Tunnel
        // ihn NICHT durchreichen — der Agent dahinter saehe fremde Tabs mit
        // fremden angemeldeten Sitzungen.
        //
        // Geprueft wird die Bedingung, nicht der Netzaufruf: ein Browser
        // antwortet, aber kein Merker gehoert uns.
        // Die Ablehnung haengt an einer Kombination: es antwortet etwas, und
        // kein Merker gehoert uns. Hier als Wahrheitstafel geprueft, damit der
        // Test weder Netz noch `HOME` braucht.
        let fremd = |antwortet: bool, unser: bool| antwortet && !unser;

        assert!(fremd(true, false), "antwortet jemand ohne unseren Merker: Fremder");
        assert!(!fremd(true, true), "unser eigener Browser ist kein Fremder");
        assert!(!fremd(false, false), "antwortet nichts, ist auch nichts fremd");
    }

    // -- Als MCP-Werkzeuge ---------------------------------------------------

    #[test]
    fn wer_den_browser_bekommt_erfaehrt_dass_er_geteilt_ist() {
        let regel = geteilt(9222);
        assert!(regel.contains("--cdp 9222 --pin-tab tab new"));
        assert!(regel.contains("Nie close --all"));

        let start = werkzeuge()[0]["description"].as_str().unwrap().to_string();
        assert!(start.contains("eigenen, selbst geoeffneten Tab"));
    }

    #[test]
    fn die_drei_werkzeuge_sind_da_und_richtig_markiert() {
        let w = werkzeuge();
        let namen: Vec<&str> = w.iter().map(|x| x["name"].as_str().unwrap()).collect();

        assert_eq!(namen, vec!["tunnel-start", "tunnel-status", "tunnel-stop"]);

        // Nur das Nachsehen ist lesend. Ein falsches `readOnlyHint` liesse ein
        // Produkt mit Nur-Lese-Zugang den Tunnel oeffnen.
        assert_eq!(w[0]["annotations"]["readOnlyHint"], false);
        assert_eq!(w[1]["annotations"]["readOnlyHint"], true);
        assert_eq!(w[2]["annotations"]["readOnlyHint"], false);
    }

    #[test]
    fn jedes_werkzeug_hat_ein_schema() {
        for x in werkzeuge() {
            assert_eq!(x["inputSchema"]["type"], "object", "{}", x["name"]);
            assert!(
                x["description"].as_str().unwrap().len() > 40,
                "{} braucht eine Beschreibung, aus der ein Modell die Lage erkennt",
                x["name"]
            );
        }
    }

    #[test]
    fn fremde_werkzeugnamen_gehoeren_nicht_uns() {
        // Der Proxy fragt uns bei JEDEM tools/call. Wer hier zu viel beansprucht,
        // verschluckt Werkzeuge der Instanz.
        assert!(fuer_mcp("list-tasks-tool", &serde_json::json!({})).is_none());
        assert!(fuer_mcp("tunnel", &serde_json::json!({})).is_none());
        assert!(fuer_mcp("", &serde_json::json!({})).is_none());
    }

    #[test]
    fn eine_genannte_angabe_schlaegt_jede_erkennung() {
        let w = Wunsch {
            tat: Tat::Starten,
            von: Some("desktop".to_string()),
            nach: Some("codingmachine".to_string()),
            port: PORT_STANDARD,
            hier: false,
        };

        let (von, nach) = betreiber(&w).expect("genannt");

        assert_eq!(von, "desktop");
        assert_eq!(nach, "codingmachine");
    }
}
