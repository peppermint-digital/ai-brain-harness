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
mod menue;
mod symbol;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod popup;
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
  Linksklick    schaltet die Live-Leitung an (30 min) oder aus
  Rechtsklick   oeffnet das Menue
  Im Menue      Haekchen „Live-Leitung“, Dauern, AI Brain oeffnen, Protokoll
                oeffnen, Beenden

Das Menue zeigt die Leiste. Unter Omarchy (quickshell) schliesst ein zweiter
Rechtsklick es nicht wieder — ein Klick daneben schon. Das liegt an der
Leiste: StatusNotifierItem hat keinen Weg, ihr „schliess mein Menue“ zu sagen,
und quickshell ruft ContextMenu nicht.

AI_BRAIN_TRAY_MENUE legt die Bedienung von Hand fest:
  schalter   Vorgabe, siehe oben
  eigen      Linksklick zeigt unser eigenes Menuefenster (auf/zu, Esc, Klick
             daneben); braucht Wayland mit wlr-layer-shell. Der Rechtsklick
             zeigt dann weiter das Menue der Leiste — also zwei Menues.
  leiste     Linksklick oeffnet das Menue der Leiste, es wird nicht geschaltet

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

        // Nicht in der Hilfe: Diesen Aufruf macht der Tray an sich selbst,
        // wenn der Linksklick unser Menüfenster öffnet
        // (`AI_BRAIN_TRAY_MENUE=eigen`, bis v0.6.0 die Vorgabe). Dass er aber
        // **aufrufbar** ist, ist kein Zufall — so lässt sich das Fenster ohne
        // Leiste und ohne Maus prüfen:
        //
        //   ai-brain-tray --menue < menue.json
        //
        // Die Liste auf stdin ist dieselbe wie im Tray (`menue::Eintrag`),
        // auf stdout kommt die gewählte Aktion. Siehe `popup`.
        #[cfg(target_os = "linux")]
        Some("--menue") => popup::als_prozess(),

        // Auch nicht in der Hilfe, und auch kein Zufall: Dieser Aufruf malt
        // die Menuekarte in eine Bilddatei, ohne Compositor und ohne Leiste.
        //
        //   ai-brain-tray --menue-bild /tmp/menue.ppm < menue.json
        //
        // Gebraucht, weil „sieht es aus wie Omarchys Menue?" eine Frage an
        // ein BILD ist und sonst niemand sie beantworten kann: Auf der
        // codingmachine haengt `grim` (Screencopy ist dort gesperrt), auf
        // einem CI-Laeufer gibt es gar keine Sitzung. Hier kommt genau der
        // Puffer heraus, den das Fenster zeigt — dieselbe Rechnung, dieselben
        // Themefarben, dieselbe Schrift.
        //
        // PPM (P6), weil es ohne eine einzige weitere Kiste zu schreiben ist;
        // `magick bild.ppm bild.png` macht daraus, was ein Mensch ansieht.
        #[cfg(target_os = "linux")]
        Some("--menue-bild") => popup::als_bild(args.get(1).map(String::as_str)),
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
