//! Der Tray zum AI-Brain-Harness (Companion, Stufe 1: Linux).
//!
//! **Er zeigt an und schaltet, mehr nicht.** Alles, was eine Regel hat —
//! Höchstdauer, Ablauf, Auflegen —, bleibt im Harness. Der Tray ruft dafür
//! `ai-brain-harness leitung status --json`, `leitung an --dauer …` und
//! `leitung aus`; so gibt es jede Regel genau einmal.
//!
//! **Warum ein eigenes Binary.** Ein Tray braucht eine Desktop-Sitzung, der
//! Harness nicht (auf der codingmachine läuft er ohne Bildschirm). Ein
//! Absturz hier darf keine Leitung kappen, und GUI-Abhängigkeiten gehören
//! nicht in jede headless Installation.
//!
//! **Warum ksni.** StatusNotifierItem über D-Bus, reines Rust, ohne GTK.
//! Läuft in waybar (Omarchy), KDE und GNOME mit AppIndicator-Erweiterung.
//!
//! Gedacht für Arbeitsrechner. Auf der codingmachine wird er nicht
//! eingerichtet: dort sitzt niemand, der ein Menü anklickt.

use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use ksni::blocking::TrayMethods;
use serde::Deserialize;

/// So oft wird der Zustand gelesen.
const TAKT: Duration = Duration::from_secs(2);

/// Die Dauern im Menü. 8 h ist die Obergrenze des Harness.
const DAUERN: [(&str, &str); 3] = [("30 min", "30m"), ("2 h", "2h"), ("8 h", "8h")];

const HILFE: &str = "\
ai-brain-tray — zeigt die Live-Leitung des AI-Brain-Harness im Tray und schaltet sie.

Aufrufe:
  ai-brain-tray               den Tray starten
  ai-brain-tray --autostart   beim Anmelden starten (~/.config/autostart)
  ai-brain-tray --kein-autostart

Der Harness wird als `ai-brain-harness` im PATH gesucht, oder unter
AI_BRAIN_HARNESS, wenn gesetzt.
";

/// Was `leitung status --json` liefert.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Status {
    an: bool,
    bis: Option<i64>,
    prozess: Option<u32>,
    lage: Option<Lage>,
    protokoll: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Lage {
    stand: String,
    grund: Option<String>,
    pane: Option<String>,
}

/// Was der Tray gerade weiss.
#[derive(Debug)]
enum Wissen {
    /// Noch nicht gefragt.
    Unbekannt,
    Da(Status),
    /// Der Harness antwortete nicht oder unlesbar.
    Fehler(String),
}

struct Tray {
    wissen: Wissen,
    /// Was die letzte Aktion meldete, falls sie scheiterte.
    letzter_fehler: Option<String>,
    /// Weckt den Abfrage-Faden nach einer Aktion, damit das Menü sofort folgt.
    wecker: mpsc::Sender<()>,
}

/// Was im Menü oben steht und welches Symbol gilt — rein, damit es ohne
/// D-Bus prüfbar ist.
#[derive(Debug, PartialEq)]
struct Ansicht {
    kopf: String,
    lage: Option<String>,
    symbol: &'static str,
}

fn ansicht(wissen: &Wissen, jetzt: i64) -> Ansicht {
    let status = match wissen {
        Wissen::Unbekannt => {
            return Ansicht {
                kopf: "Live: …".into(),
                lage: None,
                symbol: "network-idle",
            }
        }
        Wissen::Fehler(grund) => {
            return Ansicht {
                kopf: "Harness antwortet nicht".into(),
                lage: Some(grund.clone()),
                symbol: "dialog-warning",
            }
        }
        Wissen::Da(s) => s,
    };

    let Some(bis) = status.bis.filter(|_| status.an) else {
        return Ansicht {
            kopf: "Live: aus".into(),
            lage: None,
            symbol: "network-offline",
        };
    };

    let kopf = format!("Live: an, noch {}", restzeit(bis - jetzt));

    let (lage, symbol) = match (&status.lage, status.prozess) {
        (Some(l), _) if l.stand == "aufgelegt" => (
            format!(
                "Aufgelegt: {}",
                l.grund.as_deref().unwrap_or("Grund im Protokoll")
            ),
            "network-error",
        ),
        (_, None) => (
            "Leitung läuft nicht, Grund im Protokoll".into(),
            "network-error",
        ),
        (Some(l), Some(_)) if l.stand == "verbunden" => match &l.pane {
            Some(pane) => (
                format!("Verbunden, zeigt {pane}"),
                "network-transmit-receive",
            ),
            None => ("Verbunden, zeigt nichts".into(), "network-transmit-receive"),
        },
        (Some(l), Some(_)) if l.stand == "getrennt" => (
            format!(
                "Getrennt: {}",
                l.grund.as_deref().unwrap_or("versucht es wieder")
            ),
            "network-error",
        ),
        (_, Some(_)) => ("Verbindet …".into(), "network-idle"),
    };

    Ansicht {
        kopf,
        lage: Some(lage),
        symbol,
    }
}

fn restzeit(sekunden: i64) -> String {
    let minuten = (sekunden.max(0) + 59) / 60;

    match (minuten / 60, minuten % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "ai-brain-tray".into()
    }

    fn title(&self) -> String {
        "AI Brain".into()
    }

    fn icon_name(&self) -> String {
        ansicht(&self.wissen, jetzt()).symbol.into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let a = ansicht(&self.wissen, jetzt());

        ksni::ToolTip {
            title: format!("AI Brain — {}", a.kopf),
            description: a.lage.unwrap_or_default(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        let a = ansicht(&self.wissen, jetzt());
        let an = matches!(&self.wissen, Wissen::Da(s) if s.an);

        let mut eintraege: Vec<ksni::MenuItem<Self>> = vec![StandardItem {
            label: a.kopf,
            enabled: false,
            ..Default::default()
        }
        .into()];

        if let Some(lage) = a.lage {
            eintraege.push(
                StandardItem {
                    label: lage,
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        }

        if let Some(fehler) = &self.letzter_fehler {
            eintraege.push(
                StandardItem {
                    label: format!("Fehler: {fehler}"),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        }

        eintraege.push(MenuItem::Separator);

        for (beschriftung, dauer) in DAUERN {
            eintraege.push(
                StandardItem {
                    label: if an {
                        format!("Ab jetzt {beschriftung}")
                    } else {
                        format!("Live einschalten: {beschriftung}")
                    },
                    activate: Box::new(move |t: &mut Self| {
                        t.ausfuehren(&["leitung", "an", "--dauer", dauer])
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }

        eintraege.push(
            StandardItem {
                label: "Live ausschalten".into(),
                enabled: an,
                activate: Box::new(|t: &mut Self| t.ausfuehren(&["leitung", "aus"])),
                ..Default::default()
            }
            .into(),
        );

        eintraege.push(MenuItem::Separator);

        let protokoll = match &self.wissen {
            Wissen::Da(s) => s.protokoll.clone(),
            _ => None,
        };

        eintraege.push(
            StandardItem {
                label: "Protokoll öffnen".into(),
                enabled: protokoll.is_some(),
                activate: Box::new(move |_: &mut Self| {
                    if let Some(pfad) = &protokoll {
                        let _ = Command::new("xdg-open").arg(pfad).spawn();
                    }
                }),
                ..Default::default()
            }
            .into(),
        );

        eintraege.push(
            StandardItem {
                label: "Tray beenden".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        );

        eintraege
    }
}

impl Tray {
    /// Ruft den Harness im Hintergrund — `leitung aus` wartet bis zu 8 s auf
    /// das Auflegen, und so lange darf das Menü nicht hängen.
    fn ausfuehren(&mut self, args: &[&str]) {
        self.letzter_fehler = None;

        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let wecker = self.wecker.clone();

        std::thread::spawn(move || {
            if let Err(grund) = harness(&args) {
                FEHLER.lock().map(|mut f| *f = Some(grund)).ok();
            }
            let _ = wecker.send(());
        });
    }
}

/// Der Fehler der letzten Aktion, vom Hintergrund-Faden an die Abfrage.
static FEHLER: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn harness_pfad() -> String {
    std::env::var("AI_BRAIN_HARNESS").unwrap_or_else(|_| "ai-brain-harness".into())
}

fn harness(args: &[String]) -> Result<String, String> {
    let ausgabe = Command::new(harness_pfad())
        .args(args)
        .output()
        .map_err(|e| format!("{} nicht startbar: {e}", harness_pfad()))?;

    if ausgabe.status.success() {
        Ok(String::from_utf8_lossy(&ausgabe.stdout).into_owned())
    } else {
        let text = String::from_utf8_lossy(&ausgabe.stderr);
        Err(text
            .lines()
            .last()
            .unwrap_or("unbekannter Fehler")
            .trim()
            .to_string())
    }
}

fn abfragen() -> Wissen {
    match harness(&["leitung".into(), "status".into(), "--json".into()]) {
        Ok(text) => match serde_json::from_str::<Status>(text.trim()) {
            Ok(s) => Wissen::Da(s),
            Err(_) => Wissen::Fehler("Harness zu alt: kennt `leitung status --json` nicht".into()),
        },
        Err(grund) => Wissen::Fehler(grund),
    }
}

fn jetzt() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Autostart
// ---------------------------------------------------------------------------

fn autostart_pfad() -> Result<std::path::PathBuf, String> {
    let basis = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .ok_or("Weder XDG_CONFIG_HOME noch HOME gesetzt.")?;

    Ok(basis.join("autostart").join("ai-brain-tray.desktop"))
}

fn autostart_eintrag(programm: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=AI Brain\n\
         Comment=Live-Leitung des AI-Brain-Harness\n\
         Exec={programm}\n\
         Icon=network-transmit-receive\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

fn autostart(an: bool) -> Result<(), String> {
    let pfad = autostart_pfad()?;

    if !an {
        match std::fs::remove_file(&pfad) {
            Ok(()) => println!("Autostart entfernt: {}", pfad.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("Kein Autostart eingerichtet.")
            }
            Err(e) => return Err(format!("{} lässt sich nicht löschen: {e}", pfad.display())),
        }
        return Ok(());
    }

    let programm = std::env::current_exe().map_err(|e| format!("Eigener Pfad unbekannt: {e}"))?;

    if let Some(ordner) = pfad.parent() {
        std::fs::create_dir_all(ordner)
            .map_err(|e| format!("{} ist nicht anlegbar: {e}", ordner.display()))?;
    }

    std::fs::write(&pfad, autostart_eintrag(&programm.display().to_string()))
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", pfad.display()))?;

    println!("Autostart eingerichtet: {}", pfad.display());
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let ergebnis = match args.first().map(String::as_str) {
        None => starten(),
        Some("--autostart") => autostart(true),
        Some("--kein-autostart") => autostart(false),
        Some("--help") | Some("-h") => {
            print!("{HILFE}");
            Ok(())
        }
        Some(anders) => Err(format!("Unbekannt: {anders}\n\n{HILFE}")),
    };

    if let Err(grund) = ergebnis {
        eprintln!("{grund}");
        std::process::exit(1);
    }
}

fn starten() -> Result<(), String> {
    let (wecker, geweckt) = mpsc::channel();

    let handle = Tray {
        wissen: Wissen::Unbekannt,
        letzter_fehler: None,
        wecker,
    }
    .spawn()
    .map_err(|e| {
        format!("Kein Tray verfügbar ({e}). Läuft eine Leiste mit StatusNotifier, z. B. waybar?")
    })?;

    loop {
        let wissen = abfragen();
        let fehler = FEHLER.lock().ok().and_then(|mut f| f.take());

        handle.update(|t: &mut Tray| {
            t.wissen = wissen;
            if fehler.is_some() {
                t.letzter_fehler = fehler;
            }
        });

        if handle.is_closed() {
            return Ok(());
        }

        // Nach einer Aktion sofort, sonst im Takt.
        let _ = geweckt.recv_timeout(TAKT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn da(json: &str) -> Wissen {
        Wissen::Da(serde_json::from_str(json).unwrap())
    }

    #[test]
    fn aus_ist_aus() {
        let a = ansicht(
            &da(r#"{"an":false,"bis":null,"prozess":null,"lage":null}"#),
            0,
        );
        assert_eq!(a.kopf, "Live: aus");
        assert_eq!(a.lage, None);
        assert_eq!(a.symbol, "network-offline");
    }

    #[test]
    fn verbunden_mit_pane_und_restzeit() {
        let a = ansicht(
            &da(
                r#"{"an":true,"bis":7200,"prozess":7,"lage":{"stand":"verbunden","seit":0,"pane":"bau/2"}}"#,
            ),
            0,
        );
        assert_eq!(a.kopf, "Live: an, noch 2 h");
        assert_eq!(a.lage.as_deref(), Some("Verbunden, zeigt bau/2"));
        assert_eq!(a.symbol, "network-transmit-receive");
    }

    #[test]
    fn aufgelegt_zeigt_den_grund_auch_ohne_prozess() {
        let a = ansicht(
            &da(
                r#"{"an":true,"bis":600,"prozess":null,"lage":{"stand":"aufgelegt","seit":0,"grund":"AI Brain verweigert (403)."}}"#,
            ),
            0,
        );
        assert_eq!(
            a.lage.as_deref(),
            Some("Aufgelegt: AI Brain verweigert (403).")
        );
        assert_eq!(a.symbol, "network-error");
    }

    #[test]
    fn an_ohne_prozess_ist_ein_fehler() {
        let a = ansicht(
            &da(r#"{"an":true,"bis":600,"prozess":null,"lage":null}"#),
            0,
        );
        assert_eq!(a.symbol, "network-error");
    }

    #[test]
    fn restzeit_rundet_auf() {
        assert_eq!(restzeit(1), "1 min");
        assert_eq!(restzeit(60 * 90), "1 h 30 min");
        assert_eq!(restzeit(8 * 3600), "8 h");
        assert_eq!(restzeit(-5), "0 min");
    }

    #[test]
    fn autostart_startet_genau_dieses_programm() {
        let e = autostart_eintrag("/home/x/.local/bin/ai-brain-tray");
        assert!(e.contains("\nExec=/home/x/.local/bin/ai-brain-tray\n"));
        assert!(e.starts_with("[Desktop Entry]\n"));
    }
}
