//! Der Tray zum AI-Brain-Harness (Companion).
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
//! ## Ein Programm, drei Leisten (Stand 27.10.2026)
//!
//! Bis zum 27.10.2026 gab es den Tray nur für Linux, und in der Begründung
//! stand: ksni spricht StatusNotifierItem über D-Bus, das gibt es unter macOS
//! und Windows nicht — ein Tray dort wäre ein **eigenes Programm**. Richtig
//! war daran die Technik, falsch die Folgerung. Ein eigenes Programm wäre
//! eine zweite Bedienung, die auseinanderläuft: andere Beschriftungen, andere
//! Farben, ein Fehler zweimal zu beheben.
//!
//! Deshalb drei Leisten in einem Programm:
//!
//! | | Leiste | Kiste |
//! |---|---|---|
//! | Linux | StatusNotifierItem (D-Bus) | `ksni` |
//! | macOS | NSStatusItem | `tray-icon` + `winit` |
//! | Windows | Shell_NotifyIcon | `tray-icon` + `winit` |
//!
//! Was angezeigt wird (`ansicht`), was ein Klick tut (`kern`), wie das
//! Symbol aussieht (`symbol`) und wie der Harness gefunden wird
//! (`harness`) ist für alle drei dasselbe und ohne Plattformbezug — also
//! überall testbar. Nur das Anbinden an die Leiste ist je Plattform eigen.
//!
//! **Linux-Verhalten bleibt, wie es war**: dieselbe Kiste, dieselben Aufrufe,
//! derselbe Autostart-Ort.
//!
//! Gedacht für Arbeitsrechner. Auf der codingmachine wird er nicht
//! eingerichtet: dort sitzt niemand, der ein Menü anklickt.

mod ansicht;
mod autostart;
mod harness;
mod kern;
mod symbol;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as leiste;

#[cfg(any(target_os = "macos", windows))]
mod fremd;
#[cfg(any(target_os = "macos", windows))]
use fremd as leiste;

const HILFE: &str = "\
ai-brain-tray — zeigt Verbindung und Live-Leitung des AI-Brain-Harness in der
Leiste und schaltet die Leitung.

Aufrufe:
  ai-brain-tray               den Tray starten
  ai-brain-tray --pruefen     nur pruefen, ob es hier eine Leiste gibt
  ai-brain-tray --autostart   beim Anmelden starten
  ai-brain-tray --kein-autostart

Bedienung:
  Klick         oeffnet das Menue — links wie rechts
  Im Menue      Haekchen „Live-Leitung“, Dauern, AI Brain oeffnen, Protokoll
                oeffnen, Beenden

Das Symbol: grau nicht verbunden, blau verbunden, gruen Leitung laeuft,
gelb unterwegs, rot gestoert. Gefuellt heisst Leitung an, hohl heisst aus.

Der Harness wird neben diesem Programm, in ~/.local/bin und im PATH gesucht,
oder unter AI_BRAIN_HARNESS, wenn gesetzt.
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let ergebnis = match args.first().map(String::as_str) {
        None => leiste::starten(),
        Some("--pruefen") => leiste::pruefen(),
        Some("--autostart") => autostart::ein(),
        Some("--kein-autostart") => autostart::aus(),
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
