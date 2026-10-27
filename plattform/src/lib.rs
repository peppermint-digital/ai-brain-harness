//! Was Harness und Tray über die Plattform wissen müssen — an einer Stelle.
//!
//! ## Warum es dieses Crate gibt (#8932, erweitert #8949)
//!
//! Angefangen hat es als `ai-brain-autostart`: Der Tray trug die drei
//! Mechanismen „beim Anmelden starten" (XDG-`.desktop`, LaunchAgent,
//! Windows-Run-Key), und dann bekam der Harness selbst einen Dienst, der
//! dasselbe brauchte. Abschreiben hätte an genau der gefährlichen Stelle
//! gedriftet — die beiden unterscheiden sich nur in Label, Dateiname und
//! Argumenten, also in Kleinigkeiten, die man beim Kopieren anpasst, während
//! eine Korrektur am Run-Key später nur in einer Fassung landet.
//!
//! Am 27.10.2026 kam der zweite Fall dazu: Der Einrichtungsassistent (#8949)
//! soll für die Anmeldung einen Browser öffnen, und dafür gab es schon eine
//! Plattform-Weiche — im **Tray**, für das Protokoll. Das wäre die dritte
//! Kopie von Plattform-Logik an einem Tag gewesen.
//!
//! Also heisst das Crate jetzt `ai-brain-plattform` und nicht mehr
//! `ai-brain-autostart`. Der Name war nach dem zweiten Modul falsch, und ein
//! Crate, dessen Name seinen Inhalt untertreibt, lädt zur vierten Kopie ein —
//! wer `oeffnen` sucht, sucht nicht in „autostart". Der Umbenennung stand
//! nichts im Weg: eine Pfad-Abhängigkeit in zwei `Cargo.toml`, nichts
//! veröffentlicht.
//!
//! ## Die Regel, nach der hier etwas landet
//!
//! Nicht „alles Plattformabhängige", sondern: **was mehr als ein Programm
//! braucht und je Plattform anders ist.** Eine Weiche, die nur der Harness
//! braucht, gehört in den Harness — dort steht sie neben ihrem Aufrufer und
//! muss nicht über eine Crate-Grenze gelesen werden.

pub mod autostart;
pub mod oeffnen;

pub use autostart::Eintrag;
