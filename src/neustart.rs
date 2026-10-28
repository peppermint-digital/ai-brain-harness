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
    /// Abgelöst: alter Prozess beendet, neuer gestartet.
    Aufgefrischt { alt: u32, neu: u32 },
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
            Lage::LaeuftAlt { pid, grund } => {
                format!("läuft noch in der ALTEN Fassung (Prozess {pid}): {grund}")
            }
            Lage::NichtGeprueft(grund) => format!("nicht nachgesehen ({grund})"),
        }
    }

    /// Muss der Mensch etwas tun?
    pub fn braucht_hand(&self) -> bool {
        matches!(self, Lage::LaeuftAlt { .. })
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

    let eigen = std::process::id();
    let Some(pid) = suchen(name, args).into_iter().find(|p| *p != eigen) else {
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

    match starten(programm, args) {
        Ok(neu) => Lage::Aufgefrischt { alt: pid, neu },
        Err(grund) => Lage::LaeuftAlt { pid, grund },
    }
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
fn suchen(name: &str, args: &[&str]) -> Vec<u32> {
    let mut befehl = Command::new("pgrep");
    befehl.arg("-x").arg(name).arg("-u").arg(nutzer());

    // Mit Argumenten muss die ganze Befehlszeile verglichen werden, und
    // `-x` gilt dann für sie. Deshalb `-f` und ein Muster, das vorn
    // verankert ist.
    if !args.is_empty() {
        befehl = Command::new("pgrep");
        befehl
            .arg("-f")
            .arg(format!("{name} {}", args.join(" ")))
            .arg("-u")
            .arg(nutzer());
    }

    let Ok(ausgabe) = befehl.stderr(Stdio::null()).output() else {
        return Vec::new();
    };

    String::from_utf8_lossy(&ausgabe.stdout)
        .lines()
        .filter_map(|z| z.trim().parse().ok())
        .collect()
}

fn nutzer() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "0".into())
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
fn starten(programm: &Path, args: &[&str]) -> Result<u32, String> {
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

    /// Die Zusammenfassung muss jeden Ausgang in Worte fassen — und nur der
    /// eine darf nach Handarbeit rufen.
    #[test]
    fn jede_lage_hat_ihr_wort() {
        let faelle = [
            Lage::LaeuftNicht,
            Lage::Aktuell,
            Lage::Aufgefrischt { alt: 11, neu: 22 },
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

        assert!(!Lage::LaeuftNicht.braucht_hand());
        assert!(!Lage::Aktuell.braucht_hand());
        assert!(!Lage::Aufgefrischt { alt: 1, neu: 2 }.braucht_hand());
        assert!(Lage::LaeuftAlt {
            pid: 3,
            grund: "x".into()
        }
        .braucht_hand());
        assert!(!Lage::NichtGeprueft("x").braucht_hand());

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
        assert!(!lage.braucht_hand());
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
}
