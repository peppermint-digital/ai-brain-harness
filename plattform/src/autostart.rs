//! Beim Anmelden starten — je Plattform anders, überall nur auf Nachfrage.
//!
//! **Nur auf Nachfrage** heisst: Das Installationsskript fragt einen Menschen,
//! und erst seine Antwort ruft hier herein. Keine Plattform bekommt eine
//! Ausnahme; was auf dem einen Rechner ungefragt startet, hat auf dem anderen
//! auch nichts zu suchen.
//!
//! | Plattform | Wo |
//! |---|---|
//! | Linux | `~/.config/autostart/<datei>.desktop` (XDG) |
//! | macOS | `~/Library/LaunchAgents/<label>.plist` |
//! | Windows | Run-Key unter `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` |
//!
//! Unter Windows **kein** Autostart-Ordner: Dort müsste eine Verknüpfung
//! (`.lnk`) liegen, und die schreibt man nicht ohne COM. Eine `.cmd` an
//! derselben Stelle würde bei jeder Anmeldung ein Konsolenfenster
//! aufblitzen lassen. Der Run-Key kostet keine Abhängigkeit, braucht keine
//! Administratorrechte (HKCU) und zeigt kein Fenster.
//!
//! Warum das nicht im Tray steht, sondern hier: siehe den Kopf des Crates.
//!
//! Die Textbausteine stehen absichtlich plattformunabhängig hier: So lassen
//! sie sich auf jedem Rechner testen, auch wenn der Pfad daneben nur unter
//! einem Betriebssystem gilt.

#[cfg(not(windows))]
use std::path::PathBuf;

/// Was beim Anmelden starten soll.
///
/// Ein Wert statt dreier Konstanten, weil es jetzt zwei Programme gibt und
/// die Angaben zusammengehören: Ein Label ohne den passenden Dateinamen legt
/// unter macOS und Linux zwei verschiedene Einträge an, und `aus()` findet
/// dann nur einen davon.
#[derive(Clone, Copy, Debug)]
pub struct Eintrag<'a> {
    /// Umgekehrte Domain, für das macOS-plist und `launchctl`.
    pub label: &'a str,
    /// Dateiname ohne Endung, für die XDG-`.desktop`-Datei.
    pub datei: &'a str,
    /// Was in der Leiste bzw. im Autostart-Verzeichnis steht.
    pub name: &'a str,
    pub kommentar: &'a str,
    /// Name des Werts im Windows-Run-Key.
    pub run_wert: &'a str,
    /// Argumente hinter dem Programm, z. B. `["leitung", "dienst"]`.
    pub args: &'a [&'a str],
}

impl Eintrag<'_> {
    /// Programm und Argumente als eine Befehlszeile.
    ///
    /// **Der Pfad in Anführungszeichen, sobald Argumente folgen.** Er enthält
    /// regelmässig Leerzeichen (`C:\Users\Vorname Nachname\…`,
    /// `/Users/Vor Name/…`), und ohne sie liest die Gegenseite den ersten
    /// Teil als Programm und den Rest als Argumente.
    ///
    /// Ohne Argumente bleibt er nackt: Genau so stand er bis zum 27.10.2026
    /// in der `.desktop`-Datei des Trays, und eine Datei, die sich nach einem
    /// Update anders liest, lädt zu der Frage ein, ob sie noch gilt.
    fn zeile(&self, programm: &str) -> String {
        if self.args.is_empty() {
            return programm.to_string();
        }

        format!("\"{programm}\" {}", self.args.join(" "))
    }
}

// ---------------------------------------------------------------------------
// Die Textbausteine (überall übersetzt, überall testbar)
// ---------------------------------------------------------------------------

pub fn desktop_eintrag(e: &Eintrag, programm: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={}\n\
         Comment={}\n\
         Exec={}\n\
         Icon=network-transmit-receive\n\
         X-GNOME-Autostart-enabled=true\n",
        e.name,
        e.kommentar,
        e.zeile(programm),
    )
}

/// Der LaunchAgent für macOS.
///
/// `RunAtLoad` ja, `KeepAlive` **nein** — und das gilt für beide Aufrufer,
/// aus zwei verschiedenen Gründen:
///
/// - Ein Tray, den `launchd` wieder startet, liesse sich über „Beenden" im
///   Menü nicht beenden; der Menüpunkt wäre eine Lüge.
/// - Der Leitungsdienst endet nur, wenn AI Brain ihn endgültig abweist (403:
///   Besitz oder Recht fehlt). Ihn dann neu zu starten hiesse, im Takt von
///   launchd gegen eine Antwort anzurennen, die sich nicht ändert. Alles,
///   was sich von selbst erholt, erholt sich in seiner eigenen Schleife.
///
/// Beim nächsten Anmelden sind beide wieder da, und das ist das Versprechen
/// von Autostart.
pub fn plist(e: &Eintrag, programm: &str) -> String {
    let mut argumente = format!("        <string>{programm}</string>\n");

    for arg in e.args {
        argumente.push_str(&format!("        <string>{arg}</string>\n"));
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
{argumente}    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
        e.label,
    )
}

// ---------------------------------------------------------------------------
// Linux
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn pfad(e: &Eintrag) -> Result<PathBuf, String> {
    let basis = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or("Weder XDG_CONFIG_HOME noch HOME gesetzt.")?;

    Ok(basis.join("autostart").join(format!("{}.desktop", e.datei)))
}

#[cfg(target_os = "linux")]
pub fn ein(e: &Eintrag) -> Result<(), String> {
    let pfad = pfad(e)?;
    schreiben(&pfad, &desktop_eintrag(e, &eigenes_programm()?))?;

    println!("Autostart eingerichtet: {}", pfad.display());
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn aus(e: &Eintrag) -> Result<(), String> {
    entfernen(&pfad(e)?)
}

// ---------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn pfad(e: &Eintrag) -> Result<PathBuf, String> {
    let heim = std::env::var_os("HOME").ok_or("HOME ist nicht gesetzt.")?;

    Ok(PathBuf::from(heim)
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", e.label)))
}

#[cfg(target_os = "macos")]
pub fn ein(e: &Eintrag) -> Result<(), String> {
    let pfad = pfad(e)?;
    schreiben(&pfad, &plist(e, &eigenes_programm()?))?;

    println!("Autostart eingerichtet: {}", pfad.display());

    // Sofort anmelden, damit es nicht erst beim nächsten Anmelden gilt. Ein
    // Fehlschlag ist kein Grund zum Abbruch: Die Datei liegt, und beim
    // nächsten Anmelden liest launchd sie von selbst.
    let uid = unsafe { libc_getuid() };
    let ziel = format!("gui/{uid}");

    let geladen = std::process::Command::new("launchctl")
        .args(["bootstrap", &ziel])
        .arg(&pfad)
        .output()
        .map(|a| a.status.success())
        .unwrap_or(false);

    if !geladen {
        println!("launchctl hat ihn nicht sofort übernommen — er startet beim nächsten Anmelden.");
    }

    Ok(())
}

#[cfg(target_os = "macos")]
pub fn aus(e: &Eintrag) -> Result<(), String> {
    let pfad = pfad(e)?;
    let uid = unsafe { libc_getuid() };

    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &format!("gui/{uid}/{}", e.label)])
        .output();

    entfernen(&pfad)
}

/// Die eigene Benutzernummer. Statt einer Abhängigkeit auf `libc` nur für
/// diese eine Zahl: `launchctl` braucht sie für `gui/<uid>`.
#[cfg(target_os = "macos")]
extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
const RUN_SCHLUESSEL: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
pub fn ein(e: &Eintrag) -> Result<(), String> {
    let programm = eigenes_programm()?;
    let zeile = e.zeile_windows(&programm);

    let ausgabe = std::process::Command::new("reg")
        .args([
            "add",
            RUN_SCHLUESSEL,
            "/v",
            e.run_wert,
            "/t",
            "REG_SZ",
            "/d",
            &zeile,
            "/f",
        ])
        .output()
        .map_err(|e| format!("reg.exe nicht startbar: {e}"))?;

    if !ausgabe.status.success() {
        return Err(format!(
            "Der Run-Eintrag liess sich nicht schreiben: {}",
            String::from_utf8_lossy(&ausgabe.stderr).trim()
        ));
    }

    println!("Autostart eingerichtet: {RUN_SCHLUESSEL}\\{}", e.run_wert);
    Ok(())
}

#[cfg(windows)]
pub fn aus(e: &Eintrag) -> Result<(), String> {
    let ausgabe = std::process::Command::new("reg")
        .args(["delete", RUN_SCHLUESSEL, "/v", e.run_wert, "/f"])
        .output()
        .map_err(|e| format!("reg.exe nicht startbar: {e}"))?;

    if ausgabe.status.success() {
        println!("Autostart entfernt.");
    } else {
        // `reg delete` scheitert auch, wenn es nichts zu löschen gibt — das
        // ist kein Fehler, sondern der gewünschte Zustand.
        println!("Kein Autostart eingerichtet.");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Gemeinsam
// ---------------------------------------------------------------------------

impl Eintrag<'_> {
    /// Die Befehlszeile für den Run-Key: der Pfad IMMER in Anführungszeichen.
    ///
    /// Anders als [`Eintrag::zeile`], und zwar weil Windows hier anders liest:
    /// Ein Pfad mit Leerzeichen wird ohne Anführungszeichen beim Anmelden nur
    /// bis zum ersten Leerzeichen gestartet — auch ohne jedes Argument.
    #[allow(dead_code)]
    fn zeile_windows(&self, programm: &str) -> String {
        if self.args.is_empty() {
            return format!("\"{programm}\"");
        }

        format!("\"{programm}\" {}", self.args.join(" "))
    }
}

#[cfg(not(windows))]
fn schreiben(pfad: &std::path::Path, inhalt: &str) -> Result<(), String> {
    if let Some(ordner) = pfad.parent() {
        std::fs::create_dir_all(ordner)
            .map_err(|e| format!("{} ist nicht anlegbar: {e}", ordner.display()))?;
    }

    std::fs::write(pfad, inhalt)
        .map_err(|e| format!("{} ist nicht schreibbar: {e}", pfad.display()))
}

#[cfg(not(windows))]
fn entfernen(pfad: &std::path::Path) -> Result<(), String> {
    match std::fs::remove_file(pfad) {
        Ok(()) => println!("Autostart entfernt: {}", pfad.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("Kein Autostart eingerichtet.")
        }
        Err(e) => return Err(format!("{} lässt sich nicht löschen: {e}", pfad.display())),
    }

    Ok(())
}

fn eigenes_programm() -> Result<String, String> {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .map_err(|e| format!("Eigener Pfad unbekannt: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRAY: Eintrag = Eintrag {
        label: "de.peppermint.ai-brain-tray",
        datei: "ai-brain-tray",
        name: "AI Brain",
        kommentar: "Live-Leitung des AI-Brain-Harness",
        run_wert: "ai-brain-tray",
        args: &[],
    };

    const DIENST: Eintrag = Eintrag {
        label: "de.peppermint.ai-brain-leitung",
        datei: "ai-brain-leitung",
        name: "AI Brain — Leitung",
        kommentar:
            "Haelt die Leitung zu AI Brain, solange ein Channel diesen Rechner freigegeben hat",
        run_wert: "ai-brain-leitung",
        args: &["leitung", "dienst"],
    };

    #[test]
    fn autostart_startet_genau_dieses_programm() {
        let e = desktop_eintrag(&TRAY, "/home/x/.local/bin/ai-brain-tray");

        assert!(e.contains("\nExec=/home/x/.local/bin/ai-brain-tray\n"));
        assert!(e.starts_with("[Desktop Entry]\n"));
    }

    #[test]
    fn autostart_mit_argumenten_setzt_den_pfad_in_anfuehrungszeichen() {
        // Ohne sie startet ein Pfad mit Leerzeichen nur bis zum ersten
        // Leerzeichen — und `leitung dienst` wäre dann ein zweites Programm.
        let e = desktop_eintrag(&DIENST, "/home/vor name/.local/bin/ai-brain-harness");

        assert!(
            e.contains("\nExec=\"/home/vor name/.local/bin/ai-brain-harness\" leitung dienst\n"),
            "{e}"
        );
    }

    #[test]
    fn ohne_argumente_bleibt_die_zeile_nackt() {
        // Der Stand des Trays bis zum 27.10.2026. Eine Datei, die sich nach
        // einem Update anders liest, lädt zu der Frage ein, ob sie noch gilt.
        assert_eq!(TRAY.zeile("/x/ai-brain-tray"), "/x/ai-brain-tray");
    }

    #[test]
    #[allow(non_snake_case)]
    fn der_run_key_setzt_den_pfad_IMMER_in_anfuehrungszeichen() {
        // Hier anders als in der .desktop-Datei, und das ist Absicht: Windows
        // startet einen unquotierten Pfad mit Leerzeichen auch ohne Argumente
        // nur bis zum ersten Leerzeichen.
        assert_eq!(
            TRAY.zeile_windows(r"C:\Users\Vor Name\ai-brain-tray.exe"),
            r#""C:\Users\Vor Name\ai-brain-tray.exe""#
        );
        assert_eq!(
            DIENST.zeile_windows(r"C:\Users\Vor Name\ai-brain-harness.exe"),
            r#""C:\Users\Vor Name\ai-brain-harness.exe" leitung dienst"#
        );
    }

    #[test]
    fn plist_nennt_programm_und_label() {
        let p = plist(&TRAY, "/Users/x/.local/bin/ai-brain-tray");

        assert!(p.contains("<string>/Users/x/.local/bin/ai-brain-tray</string>"));
        assert!(p.contains("<string>de.peppermint.ai-brain-tray</string>"));
        assert!(p.contains("<key>RunAtLoad</key>\n    <true/>"));
    }

    #[test]
    fn plist_nennt_jedes_argument_als_eigenen_string() {
        // `launchd` nimmt keine Befehlszeile, sondern eine Liste. Ein
        // "leitung dienst" in EINEM <string> wäre ein Argument mit Leerzeichen
        // darin, und der Harness läse es als unbekannten Unterbefehl.
        let p = plist(&DIENST, "/usr/local/bin/ai-brain-harness");

        assert!(p.contains("<string>/usr/local/bin/ai-brain-harness</string>"));
        assert!(p.contains("<string>leitung</string>"));
        assert!(p.contains("<string>dienst</string>"));
        assert!(!p.contains("<string>leitung dienst</string>"));
    }

    /// Ein `launchd`-Agent mit KeepAlive liesse sich nicht beenden — der
    /// Menüpunkt „Tray beenden" wäre dann wirkungslos, und der Leitungsdienst
    /// würde gegen ein 403 anrennen, das sich nicht ändert.
    #[test]
    fn plist_haelt_nichts_am_leben() {
        for e in [TRAY, DIENST] {
            assert!(plist(&e, "/x").contains("<key>KeepAlive</key>\n    <false/>"));
        }
    }

    /// Das plist muss gültiges XML sein — ein kaputtes nimmt launchd
    /// stillschweigend nicht an.
    #[test]
    fn plist_ist_wohlgeformt() {
        for e in [TRAY, DIENST] {
            let p = plist(&e, "/x");

            assert!(p.starts_with("<?xml version=\"1.0\""));
            assert_eq!(p.matches("<dict>").count(), p.matches("</dict>").count());
            assert_eq!(p.matches("<array>").count(), p.matches("</array>").count());
            assert!(p.trim_end().ends_with("</plist>"));
        }
    }

    /// Zwei Einträge, zwei Label und zwei Dateinamen — sonst überschreibt der
    /// eine den anderen, und `aus()` nimmt den falschen weg.
    #[test]
    fn tray_und_dienst_teilen_keinen_platz() {
        assert_ne!(TRAY.label, DIENST.label);
        assert_ne!(TRAY.datei, DIENST.datei);
        assert_ne!(TRAY.run_wert, DIENST.run_wert);
    }
}
