//! Der Autostart des Trays — die Mechanismen liegen im geteilten Crate.
//!
//! Hier bleibt nur, WAS gestartet werden soll. Die drei Plattform-Wege
//! (XDG-`.desktop`, LaunchAgent, Windows-Run-Key) standen bis zum 27.10.2026
//! in dieser Datei; seit der Harness selbst einen Dienst beim Anmelden
//! startet (#8932), wohnen sie in `ai-brain-plattform` — ein Crate, ein Satz
//! Tests, zwei Aufrufer. Siehe dort, warum nicht abgeschrieben wurde.

use ai_brain_plattform::Eintrag;

/// Der Tray startet sich selbst, ohne Argumente.
const TRAY: Eintrag = Eintrag {
    label: "de.peppermint.ai-brain-tray",
    datei: "ai-brain-tray",
    name: "AI Brain",
    kommentar: "Live-Leitung des AI-Brain-Harness",
    run_wert: "ai-brain-tray",
    args: &[],
};

pub fn ein() -> Result<(), String> {
    ai_brain_plattform::autostart::ein(&TRAY)
}

pub fn aus() -> Result<(), String> {
    ai_brain_plattform::autostart::aus(&TRAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Das Label hing bis zum 27.10.2026 an einer Konstante in dieser Datei.
    /// Es gehört zu einem Eintrag, den `launchctl bootout` wiederfinden muss —
    /// wer es ändert, lässt einen alten LaunchAgent auf jedem Mac zurück.
    #[test]
    fn der_tray_traegt_sein_bisheriges_label_weiter() {
        assert_eq!(TRAY.label, "de.peppermint.ai-brain-tray");
        assert_eq!(TRAY.datei, "ai-brain-tray");
        assert_eq!(TRAY.run_wert, "ai-brain-tray");
        // Und keine Argumente: Der Tray startet sich selbst.
        assert!(TRAY.args.is_empty());
    }
}
