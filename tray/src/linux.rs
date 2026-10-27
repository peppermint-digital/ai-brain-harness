//! Die Leiste unter Linux: StatusNotifierItem über D-Bus (ksni).
//!
//! Reines Rust, ohne GTK. Läuft in waybar (Omarchy), KDE und GNOME mit
//! AppIndicator-Erweiterung.
//!
//! Hier steht nur das Anbinden an die Leiste. Was angezeigt wird, steht in
//! `ansicht`, was passiert, in `kern` — beide ohne D-Bus, damit macOS und
//! Windows dieselbe Bedienung bekommen und nicht eine zweite, ähnliche.

use std::sync::mpsc;

use ksni::blocking::TrayMethods;

use crate::kern::{Kern, Signal, DAUERN, TAKT};
use crate::symbol;

/// Die Kantenlänge der Pixmap. 22 Pixel ist das Mass, in dem Leisten
/// Tray-Symbole erwarten; grössere werden heruntergerechnet und matschig.
const KANTE: u32 = 22;

struct Tray {
    kern: Kern,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "ai-brain-tray".into()
    }

    fn title(&self) -> String {
        "AI Brain".into()
    }

    /// Leer, mit Absicht.
    ///
    /// Nennt ein Item einen Symbolnamen, nimmt die Leiste **den** und lässt
    /// die Pixmap liegen. Genau das war der Grund für „dunkel auf dunkel":
    /// Das Theme entscheidet über die Farbe, und keine Zusage über den
    /// Kontrast zur Leiste kommt dabei heraus. Siehe `symbol.rs`.
    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let a = self.kern.ansicht();
        let bild = symbol::zeichnen(a.farbe, a.gefuellt, KANTE);

        vec![ksni::Icon {
            width: bild.kante as i32,
            height: bild.kante as i32,
            data: bild.argb(),
        }]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let a = self.kern.ansicht();

        let mut zeilen = vec![a.leitung];

        if let Some(lage) = a.lage {
            zeilen.push(lage);
        }

        if let Some(quelle) = a.quelle {
            zeilen.push(quelle);
        }

        ksni::ToolTip {
            title: format!("AI Brain — {}", a.verbindung),
            description: zeilen.join("\n"),
            ..Default::default()
        }
    }

    /// Linksklick: Leitung an oder aus. Siehe `Kern::umschalten`.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.kern.umschalten();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        let a = self.kern.ansicht();
        let an = self.kern.an();

        let mut eintraege: Vec<ksni::MenuItem<Self>> = Vec::new();

        // Zwei Zeilen, nicht eine: Verbindung und Leitung sind zwei Zustände.
        // Die dritte kommt dazu, wenn es etwas zu sagen gibt.
        for text in [Some(a.verbindung), Some(a.leitung), a.lage]
            .into_iter()
            .flatten()
        {
            eintraege.push(
                StandardItem {
                    label: text,
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        }

        if let Some(fehler) = &self.kern.letzter_fehler {
            eintraege.push(
                StandardItem {
                    label: format!("Fehler: {fehler}"),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        }

        eintraege.push(MenuItem::Separator);

        for (beschriftung, dauer) in DAUERN {
            eintraege.push(
                StandardItem {
                    label: if an {
                        format!("Ab jetzt {beschriftung}")
                    } else {
                        format!("Live einschalten: {beschriftung}")
                    },
                    activate: Box::new(move |t: &mut Self| t.kern.einschalten(dauer)),
                    ..Default::default()
                }
                .into(),
            );
        }

        // ## Immer anklickbar, solange wir überhaupt etwas wissen
        //
        // Bis zum 27.10.2026 stand hier `enabled: an`. Das klingt vernünftig
        // — ausschalten, was nicht an ist, hat ja keinen Sinn — ist aber
        // genau im Störfall falsch: Steht der Schalter auf aus, während ein
        // Prozess weiterläuft (oder umgekehrt), ist `leitung aus` das
        // Werkzeug, das aufräumt, und es war ausgegraut. Der Harness
        // verträgt den Aufruf im Leerlauf und sagt dann „Leitung ist aus".
        eintraege.push(
            StandardItem {
                label: "Live ausschalten".into(),
                enabled: self.kern.weiss_etwas(),
                activate: Box::new(|t: &mut Self| t.kern.ausschalten()),
                ..Default::default()
            }
            .into(),
        );

        eintraege.push(MenuItem::Separator);

        eintraege.push(
            StandardItem {
                label: "Protokoll öffnen".into(),
                enabled: self.kern.protokoll().is_some(),
                activate: Box::new(|t: &mut Self| t.kern.protokoll_oeffnen()),
                ..Default::default()
            }
            .into(),
        );

        eintraege.push(
            StandardItem {
                label: "Linksklick schaltet die Leitung".into(),
                enabled: false,
                ..Default::default()
            }
            .into(),
        );

        // Kein `process::exit` im Rückruf: Der läuft im D-Bus-Faden, und ein
        // Prozess, der mitten in einer Antwort endet, lässt die Leiste auf
        // eine Antwort warten, die nicht mehr kommt. Stattdessen ein Signal
        // an den Haupt-Faden, der das Item abmeldet und dann geht.
        eintraege.push(
            StandardItem {
                label: "Tray beenden".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| t.kern.beenden()),
                ..Default::default()
            }
            .into(),
        );

        eintraege
    }
}

/// Gibt es hier eine Leiste, die Symbole annimmt?
///
/// Der Versuch selbst ist die Prüfung: Bei fehlendem Watcher gibt `spawn`
/// einen Fehler, und ein Tray ohne Watcher startet sonst und bleibt
/// **unsichtbar** — der unangenehmere Fall, weil nichts darauf hinweist.
pub fn pruefen() -> Result<(), String> {
    let (tx, _rx) = mpsc::channel();

    let handle = Tray {
        kern: Kern::neu(tx),
    }
    .spawn()
    .map_err(|e| format!("Keine Leiste mit StatusNotifier: {e}"))?;

    handle.shutdown().wait();

    Ok(())
}

pub fn starten() -> Result<(), String> {
    let (signal, geweckt) = mpsc::channel();

    let handle = Tray {
        kern: Kern::neu(signal),
    }
    .spawn()
    .map_err(|e| {
        format!("Kein Tray verfügbar ({e}). Läuft eine Leiste mit StatusNotifier, z. B. waybar?")
    })?;

    loop {
        let wissen = crate::harness::abfragen();
        let fehler = crate::kern::fehler_abholen();

        handle.update(|t: &mut Tray| {
            t.kern.wissen = wissen;
            if fehler.is_some() {
                t.kern.letzter_fehler = fehler;
            }
        });

        if handle.is_closed() {
            return Ok(());
        }

        // Nach einer Aktion sofort, sonst im Takt.
        match geweckt.recv_timeout(TAKT) {
            Ok(Signal::Ende) => {
                // Dem Menue-Aufruf noch seine Antwort lassen.
                //
                // Gemessen am 27.10.2026: Ohne diese Atempause meldet der
                // Aufrufer „Remote peer disconnected", weil der Prozess
                // endet, bevor ksni auf das `Event` des Menues antworten
                // konnte. Der Tray verschwindet dabei korrekt — aber die
                // Leiste sieht einen Verbindungsabbruch statt eines
                // beantworteten Klicks, und das steht dann in ihrem
                // Protokoll.
                std::thread::sleep(std::time::Duration::from_millis(250));

                handle.shutdown().wait();
                return Ok(());
            }
            _ => continue,
        }
    }
}
