//! Nach einem Update das Laufende auffrischen — oder ehrlich sagen, dass es
//! alt läuft (Bug #1136).
//!
//! ## Der Befund, der dieses Modul nötig macht
//!
//! Am 28.10.2026 meldete `ai-brain-harness einrichten --ja` auf Laptop und
//! Desktop „Tray: eingerichtet". Der laufende Tray-Prozess war aber der vom
//! Vortag — die neue Datei lag da, die alte Fassung lief weiter. Gemessen
//! über D-Bus: `ItemIsMenu` zeigte vor dem Neustart den alten Wert, danach
//! den neuen.
//!
//! Das ist nicht nur unschön. Wer danach den Fix prüft, prüft die alte
//! Fassung und hält ihn für kaputt — und genau das stand bei Bug #1124
//! kurz bevor.
//!
//! ## Die Regel: nachlesen, dann handeln, dann berichten
//!
//! Dieselbe wie in `einrichten` (#8949): Keine Zeile der Zusammenfassung
//! behauptet etwas, das nicht nachgesehen wurde. Hier heisst das drei
//! Schritte:
//!
//! 1. **Läuft überhaupt einer?** Über den Prozessnamen, nicht über eine
//!    Merkdatei — der Tray, den wir ablösen wollen, ist ja gerade die alte
//!    Fassung und schreibt vielleicht noch keine.
//! 2. **Ist er älter als die Datei?** Startzeit des Prozesses gegen
//!    Änderungszeit des Programms. Beide sind da, beide sind billig.
//! 3. **Ablösen — oder sagen, dass es nicht ging.** Ein Neustart, der
//!    scheitert, muss in der Zusammenfassung stehen, nicht im Nichts.
//!
//! ## Der Nachtrag vom 28.10.2026: ein Neustart erbt die falsche Umgebung
//!
//! Die erste Fassung (v0.5.4) löste den Tray korrekt ab — und startete ihn
//! mit **der Umgebung des Einrichtungslaufs**. Wer `einrichten` über **ssh**
//! ausführt, hat dort kein `WAYLAND_DISPLAY` und keine
//! `HYPRLAND_INSTANCE_SIGNATURE`. Der neue Tray fand also keine
//! Wayland-Verbindung, fiel auf das Menü der Leiste zurück
//! (`ItemIsMenu = true`) — und das eigene Menü war weg.
//!
//! Gemessen auf Bastians Desktop: über ssh `b true`, von Hand mit
//! `WAYLAND_DISPLAY=wayland-1` und der Hyprland-Signatur gestartet `b false`.
//! Lokal ausgeführt war es von Anfang an richtig; **genau deshalb ist es hier
//! nicht aufgefallen.**
//!
//! > **Nachtrag 29.10.2026 (v0.6.1, #9060):** Die Messung oben steht
//! > unverändert, ihr Symptom gibt es aber nicht mehr. Seit v0.6.1 schaltet
//! > der Linksklick die Leitung und das Menü kommt von der Leiste; dafür
//! > braucht es kein layer-shell, also wird beim Start auch nicht mehr danach
//! > gesehen. `ItemIsMenu` hängt damit nicht länger an der Wayland-Umgebung.
//! > Die **Regel** darunter gilt weiter, und zwar unabhängig vom Tray: Wer
//! > einen Prozess ersetzt, stellt ihn so wieder hin, wie er stand.
//!
//! > **Ein Prozess zu ersetzen heisst nicht, ihn neu zu starten — es heisst,
//! > ihn so wieder hinzustellen, wie er stand.** Dazu gehört seine Umgebung.
//!
//! Deshalb kommt die Sitzungsbindung jetzt aus `/proc/<pid>/environ` des
//! **alten** Prozesses. Sie ist per Definition die richtige: Der Tray hing ja
//! sichtbar in der Leiste.
//!
//! Zwei Alternativen wurden erwogen und nicht genommen:
//!
//! | Weg | Warum nicht |
//! |---|---|
//! | `hyprctl dispatch exec` | Nur Hyprland. Der Tray läuft auch unter sway, waybar, KDE und GNOME |
//! | `systemd-run --user` | Setzt voraus, dass die Sitzung ihre Umgebung in den Benutzer-Manager importiert hat (`systemctl --user import-environment`). uwsm tut das, nicht jede Einrichtung |
//!
//! ## Was dieses Modul NICHT tut
//!
//! Es startet nichts, das vorher nicht lief. Wer seinen Tray beendet hat,
//! hat ihn beendet; ein Einrichtungslauf ist kein Anlass, ihn
//! zurückzubringen. Den Autostart richtet `einrichten` getrennt ein, und das
//! ist die Zusage für das nächste Anmelden.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

/// Was mit einem laufenden Programm geschehen ist.
#[derive(Debug, PartialEq, Eq)]
pub enum Lage {
    /// Es lief keiner — dann gibt es nichts aufzufrischen.
    LaeuftNicht,
    /// Der laufende Prozess ist jünger als die Datei, also schon der neue.
    Aktuell,
    /// Abgelöst: alter Prozess beendet, neuer gestartet — mit seiner
    /// Sitzungsbindung.
    Aufgefrischt { alt: u32, neu: u32 },
    /// Abgelöst, aber **ohne** Sitzungsbindung: Der alte Prozess hatte selbst
    /// keine, oder sie war nicht zu lesen. Der neue sieht dann keinen
    /// Bildschirm — unter Linux heisst das: kein eigenes Menü.
    AufgefrischtBlind { alt: u32, neu: u32 },
    /// Er läuft noch alt, und wir konnten es nicht ändern.
    LaeuftAlt { pid: u32, grund: String },
    /// Auf dieser Plattform wird nicht nachgesehen.
    NichtGeprueft(&'static str),
}

impl Lage {
    /// Die eine Zeile für die Zusammenfassung von `einrichten`.
    pub fn wort(&self) -> String {
        match self {
            Lage::LaeuftNicht => "lief nicht".into(),
            Lage::Aktuell => "läuft bereits in dieser Fassung".into(),
            Lage::Aufgefrischt { alt, neu } => {
                format!("neu gestartet ({alt} → {neu})")
            }
            Lage::AufgefrischtBlind { alt, neu } => {
                format!("neu gestartet ({alt} → {neu}) — OHNE Sitzungsumgebung")
            }
            Lage::LaeuftAlt { pid, grund } => {
                format!("läuft noch in der ALTEN Fassung (Prozess {pid}): {grund}")
            }
            Lage::NichtGeprueft(grund) => format!("nicht nachgesehen ({grund})"),
        }
    }

    /// Was der Mensch dann tun soll — oder `None`, wenn nichts offen ist.
    ///
    /// Hier stand daneben ein `braucht_hand()`, das dasselbe sagte. Zwei
    /// Wahrheiten über dieselbe Sache laufen auseinander, sobald eine Lage
    /// dazukommt: `braucht_hand` vergisst man eher als den Satz, den der
    /// Mensch liest.
    pub fn rat(&self) -> Option<&'static str> {
        match self {
            Lage::LaeuftAlt { .. } => {
                Some("Beende den Prozess und starte ihn neu, oder melde dich ab und wieder an.")
            }
            Lage::AufgefrischtBlind { .. } => Some(
                "Das passiert, wenn `einrichten` über ssh läuft: Der neue Prozess findet \
                 dann keine Bildschirmsitzung. Starte ihn aus einem Terminal AUF dem \
                 Rechner neu — oder melde dich ab und wieder an.",
            ),
            _ => None,
        }
    }
}

/// Einen laufenden Tray auf die Fassung bringen, die jetzt auf der Platte
/// liegt.
pub fn tray_auffrischen(programm: &Path) -> Lage {
    auffrischen(programm, "ai-brain-tray", &[])
}

/// Dasselbe für den Leitungsdienst.
///
/// Er läuft als `ai-brain-harness leitung dienst` — derselbe Programmname wie
/// der Einrichtungsassistent selbst, der gerade läuft. Deshalb wird hier auf
/// die **Argumente** gefiltert; sonst würde sich der Assistent im Zweifel
/// selbst abschiessen.
pub fn dienst_auffrischen(programm: &Path) -> Lage {
    auffrischen(programm, "ai-brain-harness", &["leitung", "dienst"])
}

fn auffrischen(programm: &Path, name: &str, args: &[&str]) -> Lage {
    if cfg!(windows) {
        return Lage::NichtGeprueft("Windows: beim nächsten Anmelden");
    }

    let Ok(geaendert) = programm.metadata().and_then(|m| m.modified()) else {
        return Lage::NichtGeprueft("Änderungszeit des Programms nicht lesbar");
    };

    let Some(nutzer) = nutzer() else {
        return Lage::NichtGeprueft("eigener Benutzer nicht zu ermitteln");
    };

    let eigen = std::process::id();
    let Some(pid) = suchen(name, args, &nutzer)
        .into_iter()
        .find(|p| *p != eigen)
    else {
        return Lage::LaeuftNicht;
    };

    let Some(lief_seit) = gestartet(pid) else {
        return Lage::LaeuftAlt {
            pid,
            grund: "Startzeit nicht zu ermitteln".into(),
        };
    };

    if !ist_veraltet(lief_seit, geaendert) {
        return Lage::Aktuell;
    }

    // **Vor** dem Beenden lesen: `/proc/<pid>/environ` verschwindet mit dem
    // Prozess. Das ist die ganze Lehre aus dem Nachtrag oben — wer erst
    // beendet und dann die Umgebung sucht, findet nichts mehr.
    let umgebung = sitzungsumgebung(pid);
    let hat_sitzung = umgebung.iter().any(|(k, _)| ist_bildschirm(k));

    beenden(pid);

    // Kurz warten, bis er das Symbol abgemeldet hat: Zwei Symbole gleichen
    // Namens in der Leiste sind für einen Moment zu sehen, und der zweite
    // bleibt manchmal als Leiche stehen.
    for _ in 0..40 {
        if !lebt(pid) {
            break;
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    match starten(programm, args, &umgebung) {
        Ok(neu) if hat_sitzung => Lage::Aufgefrischt { alt: pid, neu },
        // Kein `WAYLAND_DISPLAY`, kein `DISPLAY`: Der neue Prozess sieht
        // keinen Bildschirm. Gestartet wird er trotzdem — die alte Fassung
        // hatte die Bindung ja auch nicht, es wird also nichts schlechter —
        // aber die Zusammenfassung sagt es.
        Ok(neu) => Lage::AufgefrischtBlind { alt: pid, neu },
        Err(grund) => Lage::LaeuftAlt { pid, grund },
    }
}

/// Die Sitzungsbindung eines laufenden Prozesses, aus `/proc/<pid>/environ`.
///
/// ## Warum eine Auswahl und nicht die ganze Umgebung
///
/// Die ganze zu übernehmen wäre verlockend — „stell ihn hin, wie er stand" —
/// und falsch: Darin steckt auch ein `PATH` und ein `LANG` von damals, und
/// ein Programm, das nach einem Update mit der Umgebung von vor zwei Wochen
/// startet, ist schwerer zu erklären als eins, das sie frisch hat. Gebraucht
/// wird genau das, was **an die Sitzung bindet** — und das ist eine kurze,
/// benennbare Liste.
///
/// `WAYLAND_SOCKET` steht bewusst **nicht** darin: Das ist eine
/// Dateizeigernummer, die nur im alten Prozess etwas bedeutet. Sie
/// mitzugeben hiesse, dem neuen einen Zeiger auf nichts zu geben.
fn sitzungsumgebung(pid: u32) -> Vec<(String, String)> {
    /// Was an die Bildschirmsitzung bindet.
    ///
    /// `DISPLAY` und `XAUTHORITY` für X11, `WAYLAND_DISPLAY` für Wayland,
    /// die Compositor-Sockel für Hyprland, sway und niri, dazu der
    /// Sitzungsbus — ohne ihn findet der Tray keinen
    /// `StatusNotifierWatcher`.
    const NAMEN: [&str; 7] = [
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "XAUTHORITY",
        "HYPRLAND_INSTANCE_SIGNATURE",
        "SWAYSOCK",
        "NIRI_SOCKET",
        "DBUS_SESSION_BUS_ADDRESS",
    ];

    /// `XDG_RUNTIME_DIR`, `XDG_SESSION_TYPE`, `XDG_CURRENT_DESKTOP` und
    /// Geschwister. Als Vorsilbe statt als Liste, weil hier regelmässig
    /// etwas dazukommt und eine Liste dann still unvollständig wird.
    const VORSILBE: &str = "XDG_";

    let Ok(roh) = std::fs::read(format!("/proc/{pid}/environ")) else {
        // Kein `/proc` (macOS) oder keine Leseerlaubnis. Dann bleibt die
        // Umgebung dieses Laufs — unter macOS ist das richtig, solange
        // `einrichten` in der Fenster-Sitzung läuft, und über ssh ist dort
        // ohnehin keine Leiste zu erreichen.
        return Vec::new();
    };

    roh.split(|b| *b == 0)
        .filter(|teil| !teil.is_empty())
        .filter_map(|teil| {
            let text = String::from_utf8_lossy(teil);
            let (name, wert) = text.split_once('=')?;

            (NAMEN.contains(&name) || name.starts_with(VORSILBE))
                .then(|| (name.to_string(), wert.to_string()))
        })
        .collect()
}

/// Bindet dieser Name an einen Bildschirm?
///
/// Nur diese zwei: Ein `XDG_RUNTIME_DIR` hat auch eine ssh-Sitzung, ein
/// `DBUS_SESSION_BUS_ADDRESS` oft ebenfalls. Woran es wirklich hängt, ob ein
/// Tray etwas sieht, sind `WAYLAND_DISPLAY` und `DISPLAY`.
fn ist_bildschirm(name: &str) -> bool {
    name == "WAYLAND_DISPLAY" || name == "DISPLAY"
}

/// Lief der Prozess schon, bevor das Programm geschrieben wurde?
///
/// Eine Sekunde Zugabe: Datei- und Prozesszeiten kommen aus verschiedenen
/// Quellen, und `ps` gibt die vergangene Zeit nur sekundengenau. Ohne die
/// Zugabe würde ein Tray, der eine halbe Sekunde NACH dem Entpacken
/// gestartet ist, als veraltet gelten und grundlos neu starten.
fn ist_veraltet(lief_seit: SystemTime, programm_geaendert: SystemTime) -> bool {
    match programm_geaendert.duration_since(lief_seit) {
        Ok(abstand) => abstand > Duration::from_secs(1),
        // Die Datei ist älter als der Prozess: alles in Ordnung.
        Err(_) => false,
    }
}

/// Die Prozesse mit diesem Namen, die diesem Benutzer gehören.
///
/// Über `pgrep`, weil es das unter Linux und macOS gibt und weil `/proc`
/// unter macOS fehlt. `-x` vergleicht den ganzen Namen, nicht ein Stück
/// davon: Ohne das träfe „ai-brain-harness" auch „ai-brain-harness-alt".
fn suchen(name: &str, args: &[&str], nutzer: &str) -> Vec<u32> {
    let mut befehl = Command::new("pgrep");
    befehl.arg("-x").arg(name).arg("-u").arg(nutzer);

    // Mit Argumenten muss die ganze Befehlszeile verglichen werden, und
    // `-x` gilt dann für sie. Deshalb `-f` und ein Muster, das vorn
    // verankert ist.
    if !args.is_empty() {
        befehl = Command::new("pgrep");
        befehl
            .arg("-f")
            .arg(format!("{name} {}", args.join(" ")))
            .arg("-u")
            .arg(nutzer);
    }

    let Ok(ausgabe) = befehl.stderr(Stdio::null()).output() else {
        return Vec::new();
    };

    String::from_utf8_lossy(&ausgabe.stdout)
        .lines()
        .filter_map(|z| z.trim().parse().ok())
        .collect()
}

/// Wer sind wir — als Zahl, nicht als Name aus der Umgebung.
///
/// ## Warum nicht `$USER`
///
/// Hier stand `std::env::var("USER")` mit `"0"` als Rückfall, und das ist
/// genau die Art Fehler, die dieses Modul eigentlich fangen soll: Startet
/// `einrichten` aus einer **leeren Umgebung** (`env -i`, ein systemd-Dienst,
/// ein Wrapper), ist `USER` nicht gesetzt — dann suchte `pgrep -u 0` die
/// Prozesse von **root**, fand nichts, und die Zusammenfassung meldete
/// seelenruhig „lief nicht".
///
/// Gemessen am 28.10.2026: Über ssh ging es (dort setzt pam `USER`), mit
/// `env -i` nicht. **Ein Rückfall, der eine falsche Antwort gibt, ist
/// schlimmer als keiner** — er sieht aus wie ein Befund.
///
/// `id -u` fragt den Kern statt die Umgebung. Antwortet auch das nicht, wird
/// **nicht geraten**: Dann sagt die Zusammenfassung, dass nicht nachgesehen
/// wurde.
fn nutzer() -> Option<String> {
    let ausgabe = Command::new("id")
        .arg("-u")
        .stderr(Stdio::null())
        .output()
        .ok()?;

    let uid = String::from_utf8_lossy(&ausgabe.stdout).trim().to_string();

    if uid.chars().all(|c| c.is_ascii_digit()) && !uid.is_empty() {
        return Some(uid);
    }

    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .ok()
        .filter(|n| !n.trim().is_empty())
}

/// Wann ist dieser Prozess gestartet?
///
/// Über `ps -o etime=` (vergangene Zeit) statt `lstart`: Die Startzeit
/// schreibt `ps` in der Sprache und Zeitzone des Systems („Mon Sep 28
/// 10:43:40 2026"), die vergangene Zeit dagegen immer als `[[tt-]hh:]mm:ss`.
/// Eine Zahl, die man rechnen kann, ist einer zu parsenden Datumszeile
/// vorzuziehen.
fn gestartet(pid: u32) -> Option<SystemTime> {
    let ausgabe = Command::new("ps")
        .args(["-o", "etime=", "-p", &pid.to_string()])
        .stderr(Stdio::null())
        .output()
        .ok()?;

    let gelaufen = etime_lesen(&String::from_utf8_lossy(&ausgabe.stdout))?;

    SystemTime::now().checked_sub(gelaufen)
}

/// `[[tt-]hh:]mm:ss` in eine Dauer.
fn etime_lesen(text: &str) -> Option<Duration> {
    let text = text.trim();

    if text.is_empty() {
        return None;
    }

    let (tage, rest) = match text.split_once('-') {
        Some((t, r)) => (t.trim().parse::<u64>().ok()?, r),
        None => (0, text),
    };

    let teile: Vec<&str> = rest.split(':').collect();

    let (stunden, minuten, sekunden) = match teile.as_slice() {
        [h, m, s] => (
            h.trim().parse::<u64>().ok()?,
            m.trim().parse::<u64>().ok()?,
            s.trim().parse::<u64>().ok()?,
        ),
        [m, s] => (
            0,
            m.trim().parse::<u64>().ok()?,
            s.trim().parse::<u64>().ok()?,
        ),
        _ => return None,
    };

    Some(Duration::from_secs(
        tage * 86_400 + stunden * 3_600 + minuten * 60 + sekunden,
    ))
}

fn lebt(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// SIGTERM, nicht SIGKILL.
///
/// Dieselbe Begründung wie beim Leitungsdienst: Ein Programm, das abgewürgt
/// wird, räumt nicht auf. Der Tray meldet beim Beenden sein Symbol ab, und
/// die Leiste sieht dann einen beantworteten Abschied statt eines
/// Verbindungsabbruchs.
fn beenden(pid: u32) {
    let _ = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Das Programm neu starten — losgelöst von diesem Terminal.
///
/// `setsid`, wo es das gibt: Sonst hängt der neue Tray an der Sitzung des
/// Terminals, in dem `einrichten` lief, und bekommt beim Schliessen ein
/// SIGHUP. Das wäre der unangenehmste Ausgang — der Tray erscheint, und beim
/// nächsten Fensterschliessen ist er weg, ohne dass jemand den Zusammenhang
/// sieht.
fn starten(programm: &Path, args: &[&str], umgebung: &[(String, String)]) -> Result<u32, String> {
    let mit_setsid = Path::new("/usr/bin/setsid").is_file() || Path::new("/bin/setsid").is_file();

    let mut befehl = if mit_setsid {
        let mut c = Command::new("setsid");
        c.arg(programm);
        c
    } else {
        Command::new(programm)
    };

    befehl
        .args(args)
        .envs(umgebung.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let kind = befehl
        .spawn()
        .map_err(|e| format!("Neustart gescheitert: {e}"))?;

    let pid = kind.id();

    // Nicht auf das Kind warten: Es soll ja laufen bleiben. Mit `setsid`
    // ist die hier gemeldete Nummer die des `setsid`-Aufrufs, der sich
    // sofort beendet — gut genug für die Meldung, dass etwas gestartet
    // wurde, und wir behaupten nichts Genaueres.
    Ok(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etime_in_allen_formen() {
        assert_eq!(etime_lesen("      05:12"), Some(Duration::from_secs(312)));
        assert_eq!(etime_lesen("01:05:12"), Some(Duration::from_secs(3912)));
        assert_eq!(
            etime_lesen("2-01:05:12"),
            Some(Duration::from_secs(2 * 86_400 + 3912))
        );
        assert_eq!(etime_lesen("00:00"), Some(Duration::ZERO));
        // So gibt `ps` es wirklich aus: führende Leerzeichen, Zeilenende.
        assert_eq!(
            etime_lesen("   30-02:16:19\n"),
            Some(Duration::from_secs(30 * 86_400 + 2 * 3600 + 16 * 60 + 19))
        );

        assert_eq!(etime_lesen(""), None);
        assert_eq!(etime_lesen("  \n"), None);
        assert_eq!(etime_lesen("gestern"), None);
        assert_eq!(etime_lesen("1:2:3:4"), None);
    }

    /// Der Kern des Bugs in einer Zeile: Lief er vor der Datei, ist er alt.
    #[test]
    fn alt_ist_wer_vor_der_datei_gestartet_ist() {
        let jetzt = SystemTime::now();
        let vorher = jetzt - Duration::from_secs(3600);

        // Prozess um eine Stunde älter als die Datei → veraltet.
        assert!(ist_veraltet(vorher, jetzt));

        // Datei älter als der Prozess → in Ordnung.
        assert!(!ist_veraltet(jetzt, vorher));

        // Gleichzeitig → in Ordnung.
        assert!(!ist_veraltet(jetzt, jetzt));
    }

    /// Eine Sekunde Zugabe, und zwar aus einem Grund.
    ///
    /// `ps` gibt die vergangene Zeit sekundengenau, die Datei hat
    /// Nanosekunden. Ohne Zugabe gälte ein Tray, der unmittelbar nach dem
    /// Entpacken gestartet wurde, als veraltet — und `einrichten` würde ihn
    /// bei JEDEM Lauf neu starten, für nichts.
    #[test]
    fn eine_knappe_sekunde_zaehlt_nicht_als_alt() {
        let jetzt = SystemTime::now();

        assert!(!ist_veraltet(jetzt - Duration::from_millis(900), jetzt));
        assert!(!ist_veraltet(jetzt, jetzt + Duration::from_millis(999)));
        assert!(ist_veraltet(jetzt, jetzt + Duration::from_secs(2)));
    }

    /// Die Zusammenfassung muss jeden Ausgang in Worte fassen — und nur die
    /// beiden Ausgänge, bei denen etwas offen bleibt, dürfen nach Handarbeit
    /// rufen.
    #[test]
    fn jede_lage_hat_ihr_wort() {
        let faelle = [
            Lage::LaeuftNicht,
            Lage::Aktuell,
            Lage::Aufgefrischt { alt: 11, neu: 22 },
            Lage::AufgefrischtBlind { alt: 11, neu: 22 },
            Lage::LaeuftAlt {
                pid: 33,
                grund: "Neustart gescheitert".into(),
            },
            Lage::NichtGeprueft("Windows: beim nächsten Anmelden"),
        ];

        for lage in &faelle {
            let w = lage.wort();
            assert!(!w.is_empty(), "{lage:?}");
            assert!(w.len() < 110, "zu lang für die Übersicht: {w}");
        }

        // Genau die zwei Ausgänge, bei denen etwas offen bleibt, geben einen
        // Rat — und die anderen vier keinen.
        assert!(Lage::AufgefrischtBlind { alt: 1, neu: 2 }.rat().is_some());
        assert!(Lage::LaeuftAlt {
            pid: 3,
            grund: "x".into()
        }
        .rat()
        .is_some());

        assert!(Lage::LaeuftNicht.rat().is_none());
        assert!(Lage::Aktuell.rat().is_none());
        assert!(Lage::Aufgefrischt { alt: 1, neu: 2 }.rat().is_none());
        assert!(Lage::NichtGeprueft("x").rat().is_none());

        // Der ssh-Fall muss als solcher benannt sein — sonst sucht jemand
        // den Fehler im Tray statt in seinem Zugang.
        assert!(Lage::AufgefrischtBlind { alt: 1, neu: 2 }
            .rat()
            .expect("Rat")
            .contains("ssh"));

        // Die ALTE Fassung muss in der Zeile stehen — das ist der Satz, den
        // jemand lesen soll, bevor er den Fix für kaputt hält.
        assert!(Lage::LaeuftAlt {
            pid: 42,
            grund: "Startzeit nicht zu ermitteln".into()
        }
        .wort()
        .contains("ALTEN"));
    }

    /// Ein Programm, das es nicht gibt, lässt sich nicht datieren — und dann
    /// wird nichts behauptet.
    #[test]
    fn ohne_programm_wird_nichts_behauptet() {
        let lage = tray_auffrischen(Path::new("/gibt/es/nicht/ai-brain-tray"));

        assert!(
            matches!(lage, Lage::NichtGeprueft(_)),
            "{lage:?} — erwartet wurde NichtGeprueft"
        );
        assert!(lage.rat().is_none());
    }

    /// `gestartet` liefert für den eigenen Prozess eine Zeit, die in der
    /// Vergangenheit liegt — und zwar in der nahen.
    ///
    /// Die Probe prüft das Zusammenspiel mit `ps`, nicht nur das Parsen:
    /// Welche Spalten `ps` kennt, ist zwischen Linux und macOS verschieden,
    /// und `etime` ist die, die es auf beiden gibt.
    #[test]
    fn die_eigene_startzeit_ist_zu_finden() {
        if cfg!(windows) {
            return;
        }

        let Some(start) = gestartet(std::process::id()) else {
            // Kein `ps` — dann sagt diese Probe nichts.
            return;
        };

        let alter = SystemTime::now()
            .duration_since(start)
            .expect("Start liegt in der Vergangenheit");

        assert!(
            alter < Duration::from_secs(3600),
            "der Testlauf läuft angeblich seit {alter:?}"
        );
    }

    /// Die Sitzungsbindung des eigenen Prozesses ist zu finden — und es
    /// kommt nur heraus, was wirklich bindet.
    ///
    /// Geprüft gegen den Testlauf selbst: Was `/proc/self/environ` enthält,
    /// weiss auch `std::env`. Die Probe ist damit unabhängig davon, ob hier
    /// gerade eine Bildschirmsitzung läuft.
    #[test]
    fn die_sitzungsbindung_kommt_aus_proc() {
        if !Path::new("/proc/self/environ").exists() {
            return;
        }

        let gelesen = sitzungsumgebung(std::process::id());

        for (name, wert) in &gelesen {
            assert_eq!(
                std::env::var(name).ok().as_deref(),
                Some(wert.as_str()),
                "{name} stimmt nicht mit der eigenen Umgebung überein"
            );

            assert!(
                name.starts_with("XDG_")
                    || [
                        "WAYLAND_DISPLAY",
                        "DISPLAY",
                        "XAUTHORITY",
                        "HYPRLAND_INSTANCE_SIGNATURE",
                        "SWAYSOCK",
                        "NIRI_SOCKET",
                        "DBUS_SESSION_BUS_ADDRESS",
                    ]
                    .contains(&name.as_str()),
                "{name} gehört nicht zur Sitzungsbindung"
            );
        }

        // Was es in der eigenen Umgebung gibt, muss auch ankommen — sonst
        // hätte der Leser eine Lücke, die niemand sieht.
        for name in ["XDG_RUNTIME_DIR", "WAYLAND_DISPLAY", "DISPLAY"] {
            if std::env::var(name).is_ok() {
                assert!(
                    gelesen.iter().any(|(k, _)| k == name),
                    "{name} steht in der Umgebung, kommt aber nicht mit"
                );
            }
        }

        // `WAYLAND_SOCKET` ist eine Dateizeigernummer und darf NIE mit.
        assert!(!gelesen.iter().any(|(k, _)| k == "WAYLAND_SOCKET"));
    }

    /// Der eigene Benutzer wird ermittelt, nicht geraten — auch ohne `$USER`.
    ///
    /// Die Probe ruft `nutzer()` ohne jede Annahme über die Umgebung dieses
    /// Testlaufs und prüft gegen `id -u`. Das ist die Stelle, an der die
    /// erste Fassung mit `env -i` still ins Leere lief: `$USER` fehlte, der
    /// Rückfall `"0"` suchte bei root, und „lief nicht" war die Antwort.
    #[test]
    fn der_eigene_benutzer_kommt_vom_kern() {
        let Ok(ausgabe) = Command::new("id").arg("-u").output() else {
            return;
        };

        let uid = String::from_utf8_lossy(&ausgabe.stdout).trim().to_string();

        assert_eq!(nutzer().as_deref(), Some(uid.as_str()));
        assert!(uid.chars().all(|c| c.is_ascii_digit()), "{uid}");

        // Und niemals die 0, wenn wir nicht root sind.
        if uid != "0" {
            assert_ne!(nutzer().as_deref(), Some("0"));
        }
    }

    /// Ein Prozess, den es nicht gibt, liefert nichts statt irgendetwas.
    #[test]
    fn ohne_prozess_keine_umgebung() {
        // Prozessnummern über 4 Millionen gibt es auf einem gewöhnlichen
        // Linux nicht (`pid_max` liegt bei 4194304).
        assert!(sitzungsumgebung(4_194_303).is_empty());
    }

    /// An einem Bildschirm hängt es nur über diese zwei.
    ///
    /// Ein `XDG_RUNTIME_DIR` hat auch eine ssh-Sitzung — genau daran ist die
    /// erste Fassung gescheitert: Sie war vorhanden, und der Tray startete
    /// trotzdem blind.
    #[test]
    fn nur_zwei_namen_binden_an_einen_bildschirm() {
        assert!(ist_bildschirm("WAYLAND_DISPLAY"));
        assert!(ist_bildschirm("DISPLAY"));

        for name in [
            "XDG_RUNTIME_DIR",
            "XDG_SESSION_TYPE",
            "DBUS_SESSION_BUS_ADDRESS",
            "HYPRLAND_INSTANCE_SIGNATURE",
            "HOME",
            "",
        ] {
            assert!(!ist_bildschirm(name), "{name}");
        }
    }
}
