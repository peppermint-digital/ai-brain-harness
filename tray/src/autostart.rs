//! Beim Anmelden starten — je Plattform anders, überall nur auf Nachfrage.
//!
//! **Nur auf Nachfrage** heisst: Das Installationsskript fragt einen Menschen,
//! und erst seine Antwort ruft `--autostart`. Keine Plattform bekommt hier
//! eine Ausnahme; was auf dem einen Rechner ungefragt startet, hat auf dem
//! anderen auch nichts zu suchen.
//!
//! | Plattform | Wo |
//! |---|---|
//! | Linux | `~/.config/autostart/ai-brain-tray.desktop` (XDG) |
//! | macOS | `~/Library/LaunchAgents/de.peppermint.ai-brain-tray.plist` |
//! | Windows | Run-Key unter `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` |
//!
//! Unter Windows **kein** Autostart-Ordner: Dort müsste eine Verknüpfung
//! (`.lnk`) liegen, und die schreibt man nicht ohne COM. Eine `.cmd` an
//! derselben Stelle würde bei jeder Anmeldung ein Konsolenfenster
//! aufblitzen lassen. Der Run-Key kostet keine Abhängigkeit, braucht keine
//! Administratorrechte (HKCU) und zeigt kein Fenster.
//!
//! Die Textbausteine stehen absichtlich plattformunabhängig hier: So lassen
//! sie sich auf jedem Rechner testen, auch wenn der Pfad daneben nur unter
//! einem Betriebssystem gilt.

#[cfg(not(windows))]
use std::path::PathBuf;

// Die drei folgenden Stücke gelten je nur für eine Plattform, stehen aber
// ungeschuetzt hier: So werden sie auf JEDEM Rechner mitkompiliert und
// mitgetestet. Ein `#[cfg(target_os = "macos")]` am plist hiesse, dass der
// einzige Test dafuer nur auf einem Mac laeuft — also nie, denn gebaut wird
// hier auf Linux. Dafuer ein ungenutztes Stueck in Kauf zu nehmen ist der
// bessere Handel.
/// Der Name, unter dem der Tray sich einträgt — überall derselbe.
#[allow(dead_code)]
pub const LABEL: &str = "de.peppermint.ai-brain-tray";
#[allow(dead_code)]
pub const RUN_WERT: &str = "ai-brain-tray";

// ---------------------------------------------------------------------------
// Die Textbausteine (überall übersetzt, überall testbar)
// ---------------------------------------------------------------------------

pub fn desktop_eintrag(programm: &str) -> String {
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

/// Der LaunchAgent für macOS.
///
/// `RunAtLoad` ja, `KeepAlive` NEIN: Ein Tray, den `launchd` wieder startet,
/// liesse sich über „Beenden" im Menü nicht beenden — der Menüpunkt wäre
/// eine Lüge. Beim nächsten Anmelden ist er wieder da, und das ist genau
/// das Versprechen von Autostart.
#[allow(dead_code)]
pub fn plist(programm: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{programm}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#
    )
}

// ---------------------------------------------------------------------------
// Linux
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn pfad() -> Result<PathBuf, String> {
    let basis = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or("Weder XDG_CONFIG_HOME noch HOME gesetzt.")?;

    Ok(basis.join("autostart").join("ai-brain-tray.desktop"))
}

#[cfg(target_os = "linux")]
pub fn ein() -> Result<(), String> {
    let pfad = pfad()?;
    schreiben(&pfad, &desktop_eintrag(&eigenes_programm()?))?;

    println!("Autostart eingerichtet: {}", pfad.display());
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn aus() -> Result<(), String> {
    entfernen(&pfad()?)
}

// ---------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn pfad() -> Result<PathBuf, String> {
    let heim = std::env::var_os("HOME").ok_or("HOME ist nicht gesetzt.")?;

    Ok(PathBuf::from(heim)
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

#[cfg(target_os = "macos")]
pub fn ein() -> Result<(), String> {
    let pfad = pfad()?;
    schreiben(&pfad, &plist(&eigenes_programm()?))?;

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
pub fn aus() -> Result<(), String> {
    let pfad = pfad()?;
    let uid = unsafe { libc_getuid() };

    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &format!("gui/{uid}/{LABEL}")])
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
pub fn ein() -> Result<(), String> {
    let programm = eigenes_programm()?;

    let ausgabe = std::process::Command::new("reg")
        .args([
            "add",
            RUN_SCHLUESSEL,
            "/v",
            RUN_WERT,
            "/t",
            "REG_SZ",
            "/d",
            // In Anführungszeichen: Der Pfad enthält regelmässig Leerzeichen
            // (`C:\Users\Vorname Nachname\…`), und ohne sie würde Windows
            // beim Anmelden nur den ersten Teil starten.
            &format!("\"{programm}\""),
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

    println!("Autostart eingerichtet: {RUN_SCHLUESSEL}\\{RUN_WERT}");
    Ok(())
}

#[cfg(windows)]
pub fn aus() -> Result<(), String> {
    let ausgabe = std::process::Command::new("reg")
        .args(["delete", RUN_SCHLUESSEL, "/v", RUN_WERT, "/f"])
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

    #[test]
    fn autostart_startet_genau_dieses_programm() {
        let e = desktop_eintrag("/home/x/.local/bin/ai-brain-tray");

        assert!(e.contains("\nExec=/home/x/.local/bin/ai-brain-tray\n"));
        assert!(e.starts_with("[Desktop Entry]\n"));
    }

    #[test]
    fn plist_nennt_programm_und_label() {
        let p = plist("/Users/x/.local/bin/ai-brain-tray");

        assert!(p.contains("<string>/Users/x/.local/bin/ai-brain-tray</string>"));
        assert!(p.contains("<string>de.peppermint.ai-brain-tray</string>"));
        assert!(p.contains("<key>RunAtLoad</key>\n    <true/>"));
    }

    /// Ein `launchd`-Agent mit KeepAlive liesse sich nicht beenden — der
    /// Menüpunkt „Tray beenden" wäre dann wirkungslos.
    #[test]
    fn plist_haelt_den_tray_nicht_am_leben() {
        assert!(plist("/x").contains("<key>KeepAlive</key>\n    <false/>"));
    }

    /// Das plist muss gültiges XML sein — ein kaputtes nimmt launchd
    /// stillschweigend nicht an.
    #[test]
    fn plist_ist_wohlgeformt() {
        let p = plist("/x");

        assert!(p.starts_with("<?xml version=\"1.0\""));
        assert_eq!(p.matches("<dict>").count(), p.matches("</dict>").count());
        assert_eq!(p.matches("<array>").count(), p.matches("</array>").count());
        assert!(p.trim_end().ends_with("</plist>"));
    }
}
