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
use crate::kern::{self, Kern, Signal, DAUERN, TAKT};
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

/// Was an einem Menüpunkt hängt.
///
/// Unter Linux trägt jeder Eintrag seinen Rückruf selbst; `muda` liefert
/// stattdessen eine Kennung, und wir halten die Zuordnung. Deshalb wird sie
/// beim Neubauen des Menüs mit erneuert — eine alte Kennung zeigt sonst auf
/// einen Punkt, den es nicht mehr gibt.
#[derive(Clone, Copy)]
enum Aktion {
    An(&'static str),
    /// Das Häkchen: an, wenn aus — aus, wenn an.
    Umschalten,
    Aus,
    Protokoll,
    Beenden,
}

/// Was von aussen in die Schleife kommt.
enum Nachricht {
    /// Zeit, den Zustand neu zu lesen.
    Takt,
    Menue(MenuId),
    Ende,
}

/// Baut das Menü neu und gibt die Zuordnung Kennung → Aktion dazu.
fn menue(
    a: &Ansicht,
    an: bool,
    fehler: Option<&str>,
    protokoll: bool,
    weiss_etwas: bool,
    aufraeumen: bool,
) -> (Menu, HashMap<MenuId, Aktion>) {
    let menu = Menu::new();
    let mut zuordnung = HashMap::new();

    // Zwei Zeilen, nicht eine: Verbindung und Leitung sind zwei Zustände.
    let mut hinweise: Vec<String> = vec![a.verbindung.clone(), a.leitung.clone()];

    if let Some(lage) = &a.lage {
        hinweise.push(lage.clone());
    }

    if let Some(f) = fehler {
        hinweise.push(format!("Fehler: {f}"));
    }

    for text in &hinweise {
        let _ = menu.append(&MenuItem::new(text, false, None));
    }

    let _ = menu.append(&PredefinedMenuItem::separator());

    // Ein Häkchen, kein Knopf — und auf allen Plattformen dasselbe. Die
    // Begründung steht in `linux.rs` und in `Kern::umschalten`: Ein Knopf
    // „Live einschalten" sagt nicht, ob sie gerade an ist, ein Häkchen trägt
    // Zustand und Handlung in einem.
    let haken = CheckMenuItem::new("Live-Leitung", weiss_etwas, an, None);
    zuordnung.insert(haken.id().clone(), Aktion::Umschalten);
    let _ = menu.append(&haken);

    for (beschriftung, dauer) in DAUERN {
        let punkt = MenuItem::new(format!("… ab jetzt {beschriftung}"), true, None);

        zuordnung.insert(punkt.id().clone(), Aktion::An(dauer));
        let _ = menu.append(&punkt);
    }

    // Nur im Störfall, den das Häkchen nicht abdeckt — siehe `linux.rs`.
    if aufraeumen {
        let aus = MenuItem::new(
            "Leitung aufräumen (Prozess läuft ohne Schalter)",
            true,
            None,
        );
        zuordnung.insert(aus.id().clone(), Aktion::Aus);
        let _ = menu.append(&aus);
    }

    let _ = menu.append(&PredefinedMenuItem::separator());

    let log = MenuItem::new("Protokoll öffnen", protokoll, None);
    zuordnung.insert(log.id().clone(), Aktion::Protokoll);
    let _ = menu.append(&log);

    let ende = MenuItem::new("Tray beenden", true, None);
    zuordnung.insert(ende.id().clone(), Aktion::Beenden);
    let _ = menu.append(&ende);

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

    // Drei Quellen, ein Weg hinein: Menü, Symbolklick und der eigene Takt.
    let bote = schleife.create_proxy();
    let bote_menue = bote.clone();

    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = bote_menue.send_event(Nachricht::Menue(e.id));
    }));

    // KEINE eigene Klickbehandlung mehr: Das Menü öffnet die Leiste selbst
    // (`with_menu_on_left_click`), und sie schliesst es deshalb beim zweiten
    // Klick auch selbst. Wer `Activate` selbst behandelt, macht das Menü zu
    // einer Sache der Anwendung — und dann fehlt ihr der Weg, es wieder
    // zuzumachen.

    let (signal, geweckt) = mpsc::channel();
    let bote_signal = bote.clone();

    std::thread::spawn(move || {
        while let Ok(s) = geweckt.recv() {
            let _ = bote_signal.send_event(match s {
                Signal::Wecken => Nachricht::Takt,
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
                let (menu, neu) = menue(
                    &a,
                    kern.an(),
                    None,
                    false,
                    kern.weiss_etwas(),
                    kern.aufraeumen_noetig(),
                );
                zuordnung = neu;

                match TrayIconBuilder::new()
                    .with_tooltip(a.kurz())
                    .with_menu(Box::new(menu))
                    // Linksklick öffnet das Menü — das Gegenstück zu
                    // `ItemIsMenu` unter Linux (siehe `linux.rs`). Ein Tray,
                    // der sich je Plattform anders bedienen lässt, ist zwei
                    // Trays; und ein Symbolklick, der etwas SCHALTET, ist in
                    // keiner Leiste so.
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

            Event::UserEvent(Nachricht::Menue(id)) => match zuordnung.get(&id) {
                Some(Aktion::An(dauer)) => kern.einschalten(dauer),
                Some(Aktion::Umschalten) => kern.umschalten(),
                Some(Aktion::Aus) => kern.ausschalten(),
                Some(Aktion::Protokoll) => kern.protokoll_oeffnen(),
                Some(Aktion::Beenden) => kern.beenden(),
                None => {}
            },

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
                    let (menu, neu) = menue(
                        &stand.0,
                        kern.an(),
                        stand.1.as_deref(),
                        kern.protokoll().is_some(),
                        kern.weiss_etwas(),
                        kern.aufraeumen_noetig(),
                    );

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
