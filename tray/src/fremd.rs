//! Die Leisten unter macOS und Windows: `tray-icon` mit `tao` als
//! Ereignisschleife.
//!
//! ## Warum nicht ksni, und warum kein zweites Programm
//!
//! ksni spricht StatusNotifierItem über D-Bus. Das ist eine
//! freedesktop-Sache; unter macOS heisst die Leiste `NSStatusItem`, unter
//! Windows `Shell_NotifyIcon`. `tray-icon` (aus dem Tauri-Umfeld) bedient
//! beide und bringt mit `muda` das Menü mit — eine Kiste für zwei
//! Plattformen.
//!
//! Was hier NICHT noch einmal steht: welche Zeile angezeigt wird, welche
//! Farbe gilt, was ein Klick bedeutet, wie der Harness gefunden wird. Das
//! liegt in `ansicht`, `symbol`, `kern` und `harness` und ist mit Linux
//! geteilt. Dieses Modul bindet nur an.
//!
//! ## Die Ereignisschleife
//!
//! Beide Leisten nehmen ein Symbol nur aus einem laufenden Eventloop im
//! **Haupt-Faden** an — unter macOS, weil AppKit das so verlangt, unter
//! Windows, weil das Symbol an die Nachrichtenschlange seines Fensters
//! hängt. Deshalb läuft hier `tao` und nicht die Schleife aus `linux.rs`:
//! Der Takt kommt als Ereignis von aussen herein, statt dass eine eigene
//! Schleife schläft.
//!
//! Das Symbol wird erst bei `StartCause::Init` gebaut, nicht vorher: Vor dem
//! ersten Durchlauf der Schleife gibt es unter macOS noch keine laufende
//! Anwendung, an die es sich hängen könnte.

use std::collections::HashMap;
use std::sync::mpsc;

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::ansicht::Ansicht;
use crate::harness;
use crate::kern::{self, Kern, Signal, TAKT};
use crate::menue::{self, Aktion, Eintrag};
use crate::symbol;

/// Die Kantenlänge des gezeichneten Symbols.
///
/// macOS rechnet es selbst auf die Höhe der Menüleiste herunter (tray-icon
/// macht das, Seitenverhältnis bleibt), deshalb lohnt dort die doppelte
/// Auflösung für Retina-Bildschirme. Windows erwartet 16 oder 32 Pixel.
#[cfg(target_os = "macos")]
const KANTE: u32 = 44;
#[cfg(windows)]
const KANTE: u32 = 32;

/// Was von aussen in die Schleife kommt.
enum Nachricht {
    /// Zeit, den Zustand neu zu lesen.
    Takt,
    Menue(MenuId),
    /// Ein Punkt aus einem Menü, das nicht `muda` gehört.
    ///
    /// Heute gibt es das nur unter Linux (das eigene Menüfenster, #9008) —
    /// aber `Signal` ist für alle drei Leisten dieselbe Aufzählung, und ein
    /// Signal, das hier verschluckt wird, wäre ein Menüpunkt, der nichts tut.
    /// Genau der Fehler, mit dem Bug #1124 angefangen hat.
    Gewaehlt(Aktion),
    Ende,
}

/// Baut das Menü neu und gibt die Zuordnung Kennung → Aktion dazu.
///
/// ## Die Punkte kommen aus `menue::bauen` (Stand 28.10.2026, #9008)
///
/// Bis v0.5.3 stand hier eine zweite, von Hand gepflegte Fassung derselben
/// Liste — Zeile für Zeile wie die in `linux.rs`, samt eigener `Aktion`-Aufzählung.
/// Zwei Listen über dieselbe Sache, und sie **sind** auseinandergelaufen:
/// „Leitung aufräumen" kam unter Linux dazu und fehlte hier eine Fassung lang.
///
/// `muda` braucht trotzdem eine Zuordnung Kennung → Aktion: Anders als unter
/// Linux trägt ein Punkt seinen Rückruf nicht selbst, sondern meldet eine
/// Kennung. Die Zuordnung wird beim Neubauen **mit** erneuert — eine alte
/// Kennung zeigt sonst auf einen Punkt, den es nicht mehr gibt.
fn menue(kern: &Kern) -> (Menu, HashMap<MenuId, Aktion>) {
    let menu = Menu::new();
    let mut zuordnung = HashMap::new();

    for eintrag in menue::bauen(kern) {
        match eintrag {
            Eintrag::Trenner => {
                let _ = menu.append(&PredefinedMenuItem::separator());
            }

            Eintrag::Zeile(text) => {
                let _ = menu.append(&MenuItem::new(text, false, None));
            }

            // Ein Häkchen, kein Knopf — und auf allen Plattformen dasselbe.
            // Die Begründung steht in `menue::bauen` und `Kern::umschalten`:
            // Ein Knopf „Live einschalten" sagt nicht, ob sie gerade an ist,
            // ein Häkchen trägt Zustand und Handlung in einem.
            Eintrag::Haken {
                text,
                gesetzt,
                aktiv,
                aktion,
            } => {
                let haken = CheckMenuItem::new(text, aktiv, gesetzt, None);
                zuordnung.insert(haken.id().clone(), aktion);
                let _ = menu.append(&haken);
            }

            Eintrag::Punkt {
                text,
                aktiv,
                aktion,
            } => {
                let punkt = MenuItem::new(text, aktiv, None);
                zuordnung.insert(punkt.id().clone(), aktion);
                let _ = menu.append(&punkt);
            }
        }
    }

    (menu, zuordnung)
}

fn symbol_bauen(a: &Ansicht) -> Option<Icon> {
    let bild = symbol::zeichnen(a.farbe, a.gefuellt, KANTE);

    Icon::from_rgba(bild.rgba, bild.kante, bild.kante).ok()
}

/// Gibt es hier eine Leiste, die Symbole annimmt?
///
/// Das ist die Frage, die unter Linux der `org.kde.StatusNotifierWatcher`
/// beantwortet — und sie muss je Plattform neu gestellt werden, weil es
/// dort keinen Watcher gibt.
///
/// * **macOS:** `launchctl managername` sagt `Aqua`, wenn der Prozess in
///   einer Fenster-Sitzung läuft, und `Background` bzw. `StandardIO` sonst
///   (etwa über SSH). Ohne Aqua gibt es keine Menüleiste, und ein
///   NSStatusItem hätte nichts, woran es hängen könnte.
/// * **Windows:** Der Infobereich gehört zum Explorer der angemeldeten
///   Sitzung. `SESSIONNAME` ist dort `Console` oder `RDP-Tcp#…`; in einer
///   Dienstsitzung steht `Services`, und dort gibt es keine Leiste.
///
/// Beides ist weniger genau als der Watcher: Es sagt „hier ist eine
/// Sitzung", nicht „hier nimmt etwas Symbole an". Mehr ist auf diesen
/// Plattformen nicht zu holen — dafür ist der stille Fall, den der Watcher
/// abfängt (Tray startet, bleibt aber unsichtbar), dort auch nicht bekannt:
/// Menüleiste und Infobereich sind Teil des Systems, nicht ein Programm,
/// das fehlen kann.
pub fn pruefen() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let ausgabe = std::process::Command::new("launchctl")
            .arg("managername")
            .output()
            .map_err(|e| format!("launchctl nicht startbar: {e}"))?;

        let name = String::from_utf8_lossy(&ausgabe.stdout).trim().to_string();

        if name != "Aqua" {
            return Err(format!(
                "Keine Fenster-Sitzung (launchctl managername: {name}) — hier gibt es keine Menüleiste."
            ));
        }
    }

    #[cfg(windows)]
    {
        let sitzung = std::env::var("SESSIONNAME").unwrap_or_default();

        if sitzung.eq_ignore_ascii_case("Services") {
            return Err("Dienstsitzung ohne Infobereich.".into());
        }
    }

    Ok(())
}

pub fn starten() -> Result<(), String> {
    pruefen()?;

    let schleife = EventLoopBuilder::<Nachricht>::with_user_event().build();

    // Zwei Quellen, ein Weg hinein: Menü und der eigene Takt.
    let bote = schleife.create_proxy();
    let bote_menue = bote.clone();

    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = bote_menue.send_event(Nachricht::Menue(e.id));
    }));

    // Kein `TrayIconEvent`-Handler mehr (bis v0.5.2 hing dort der Linksklick
    // auf „AI Brain öffnen"). Die Leiste zeigt das Menü jetzt selbst, und
    // `tray-icon` meldet den Klick **trotzdem** — beides zu behandeln hiesse,
    // ein Fenster aufzumachen, während sich darüber ein Menü öffnet.

    let (signal, geweckt) = mpsc::channel();
    let bote_signal = bote.clone();

    std::thread::spawn(move || {
        while let Ok(s) = geweckt.recv() {
            // Vollständig und ohne `_`: Kommt eine Variante dazu, soll der
            // Übersetzer hier schimpfen. Beim Hinzufügen von
            // `Signal::Gewaehlt` hat er das getan — und nur deshalb ist es
            // aufgefallen, denn diese Datei wird auf einem Linux-Rechner gar
            // nicht übersetzt. Dafür gibt es den „Bauen"-Workflow.
            let _ = bote_signal.send_event(match s {
                Signal::Wecken => Nachricht::Takt,
                Signal::Gewaehlt(aktion) => Nachricht::Gewaehlt(aktion),
                Signal::Ende => Nachricht::Ende,
            });
        }
    });

    std::thread::spawn(move || loop {
        std::thread::sleep(TAKT);

        if bote.send_event(Nachricht::Takt).is_err() {
            return;
        }
    });

    let mut kern = Kern::neu(signal);
    let mut symbol: Option<TrayIcon> = None;
    let mut zuordnung: HashMap<MenuId, Aktion> = HashMap::new();
    let mut gezeigt: Option<(Ansicht, Option<String>)> = None;

    schleife.run(move |ereignis, _ziel, lauf| {
        *lauf = ControlFlow::Wait;

        match ereignis {
            Event::NewEvents(StartCause::Init) => {
                let a = kern.ansicht();
                let (menu, neu) = menue(&kern);
                zuordnung = neu;

                match TrayIconBuilder::new()
                    .with_tooltip(a.kurz())
                    .with_menu(Box::new(menu))
                    // Linksklick öffnet das Menü — wie unter Linux
                    // (#1124, Bastians Wunsch vom 28.10.2026).
                    //
                    // ## Die Konvention, je Plattform
                    //
                    // **macOS:** genau so. Ein `NSStatusItem` mit Menü zeigt
                    // es bei jedem Klick; Apples Richtlinien kennen für
                    // Menüleisten-Extras keine zweite Gestik. `true` ist hier
                    // also nicht nur Bastians Wunsch, sondern die Norm — und
                    // `false` war seit v0.5.1 der Ausreisser.
                    //
                    // **Windows:** dort ist die Norm eine andere. Der
                    // Infobereich trennt Linksklick (Standardaktion, meist
                    // das Hauptfenster) von Rechtsklick (Kontextmenü). Wir
                    // weichen bewusst ab, aus zwei Gründen: Dieser Tray hat
                    // **kein Hauptfenster**, das eine Standardaktion öffnen
                    // könnte — die Anwendung liegt im Browser —, und ein
                    // Tray, der sich je Leiste anders bedienen lässt, ist
                    // drei Trays. Der Rechtsklick führt zum selben Menü, es
                    // geht also keine Gewohnheit verloren.
                    .with_menu_on_left_click(true)
                    .build()
                {
                    Ok(t) => {
                        if let Some(i) = symbol_bauen(&a) {
                            let _ = t.set_icon(Some(i));
                        }

                        symbol = Some(t);
                    }
                    Err(e) => {
                        eprintln!("Kein Tray verfügbar: {e}");
                        *lauf = ControlFlow::ExitWithCode(1);
                    }
                }
            }

            Event::UserEvent(Nachricht::Ende) => {
                // Das Symbol vor dem Ende abmelden, damit kein Rest in der
                // Leiste stehen bleibt — Windows räumt verwaiste Symbole
                // erst auf, wenn jemand mit der Maus darüberfährt.
                symbol = None;
                *lauf = ControlFlow::ExitWithCode(0);
            }

            // Was ein Punkt bedeutet, steht in `Kern::tun` — einmal, für
            // alle drei Leisten. Die Aufzählung hier abzuarbeiten hiesse,
            // die Zuordnung ein zweites Mal zu schreiben.
            Event::UserEvent(Nachricht::Menue(id)) => {
                if let Some(aktion) = zuordnung.get(&id).cloned() {
                    kern.tun(&aktion);
                }
            }

            Event::UserEvent(Nachricht::Gewaehlt(aktion)) => kern.tun(&aktion),

            Event::UserEvent(Nachricht::Takt) => {
                kern.wissen = harness::abfragen();

                if let Some(f) = kern::fehler_abholen() {
                    kern.letzter_fehler = Some(f);
                }

                let a = kern.ansicht();
                let stand = (a, kern.letzter_fehler.clone());

                // Nur neu setzen, wenn sich etwas geändert hat: Ein Menü,
                // das im Takt ersetzt wird, schliesst sich unter Windows
                // unter der Hand des Nutzers.
                if gezeigt.as_ref() == Some(&stand) {
                    return;
                }

                if let Some(t) = &symbol {
                    let (menu, neu) = menue(&kern);

                    zuordnung = neu;
                    t.set_menu(Some(Box::new(menu)));
                    let _ = t.set_tooltip(Some(stand.0.kurz()));

                    if let Some(i) = symbol_bauen(&stand.0) {
                        let _ = t.set_icon(Some(i));
                    }
                }

                gezeigt = Some(stand);
            }

            _ => {}
        }
    })
}
