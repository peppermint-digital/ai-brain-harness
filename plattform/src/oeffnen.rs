//! Etwas mit dem Programm öffnen, das die Plattform dafür vorsieht.
//!
//! Eine Datei, einen Ordner, eine Adresse — die Weiche ist dieselbe, und
//! deshalb ist es eine Funktion und nicht drei.
//!
//! Vorher stand sie im Tray (`harness::oeffnen`, für das Protokoll). Mit dem
//! Einrichtungsassistenten (#8949) braucht der Harness sie auch: Er öffnet
//! die Bestätigungsseite der Anmeldung, damit niemand eine Adresse abtippen
//! muss.

use std::process::Command;

/// Das Programm, mit dem diese Plattform öffnet, und was davor muss.
///
/// Rein, damit die Weiche ohne Prozess prüfbar ist — auf jedem Rechner, nicht
/// nur auf dem, für den der Zweig gilt.
pub fn befehl() -> (&'static str, &'static [&'static str]) {
    if cfg!(target_os = "macos") {
        ("open", &[])
    } else if cfg!(windows) {
        // `cmd /c start` braucht ein leeres erstes Argument, sonst
        // verschluckt `start` den Pfad als Fenstertitel. Das ist kein
        // Schönheitsfehler: Mit einer Adresse öffnet sich dann nichts, und
        // ein Fenster mit dem Titel „https://…" steht irgendwo.
        ("cmd", &["/C", "start", ""])
    } else {
        ("xdg-open", &[])
    }
}

/// Öffnen und nicht warten. `false` heisst: liess sich nicht starten.
///
/// **Der Rückgabewert ist der Punkt.** Vorher war das ein `let _ = …`, weil
/// ein nicht geöffnetes Protokoll kein Drama ist. Beim Anmelden ist es eines:
/// Wer glaubt, der Browser gehe auf, wartet darauf — und der Code, den er
/// eintippen müsste, steht dann unbeachtet im Terminal. Der Aufrufer muss
/// also erfahren, ob es geklappt hat.
///
/// Nicht erfahren kann er, ob wirklich ein Fenster kam: `xdg-open` liefert
/// auf manchen Systemen 0 und tut nichts. Deshalb zeigt der Assistent die
/// Adresse **immer** zusätzlich an, statt sich auf diesen Wert zu verlassen.
pub fn jetzt(ziel: &str) -> bool {
    let (programm, vorab) = befehl();

    Command::new(programm).args(vorab).arg(ziel).spawn().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jede_plattform_hat_ein_programm() {
        let (programm, _) = befehl();

        assert!(!programm.is_empty());
        assert!(
            ["open", "cmd", "xdg-open"].contains(&programm),
            "{programm}"
        );
    }

    /// Der Windows-Zweig braucht das leere Argument — sonst wird die Adresse
    /// zum Fenstertitel und es öffnet sich nichts.
    #[test]
    fn der_windows_zweig_haette_sonst_einen_fenstertitel_geoeffnet() {
        // Der Zweig gilt nur unter Windows, die Erwartung lässt sich aber
        // überall formulieren: Wer `start` nimmt, nimmt auch das leere
        // Argument. Prüfbar ist das hier nur dort, wo es gilt — der Test
        // steht trotzdem, damit er dort läuft und nicht nie.
        if cfg!(windows) {
            let (programm, vorab) = befehl();

            assert_eq!(programm, "cmd");
            assert_eq!(vorab, &["/C", "start", ""]);
        }
    }
}
