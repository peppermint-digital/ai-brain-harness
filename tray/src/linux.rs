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
    /// **Rechtsklick öffnet das Menü, Linksklick öffnet AI Brain.**
    ///
    /// `false` heisst: Die Leiste ruft bei einem Linksklick `Activate`, und
    /// das ist {@link Tray::activate} hier drunter. Genau so verhalten sich
    /// die anderen Symbole in derselben Leiste, und das war Bastians
    /// Entscheidung zu Bug #1124: „wie die anderen".
    ///
    /// ## Die Fassung davor war spezifikationstreu und trotzdem falsch
    ///
    /// v0.5.0 setzte hier `true`. Das ist der von StatusNotifierItem
    /// vorgesehene Weg, ein Menü vom Host öffnen zu lassen (`ItemIsMenu`;
    /// `ksni` setzt die Eigenschaft und antwortet auf `Activate` zusätzlich
    /// mit `UnknownMethod`, woran GNOME und KDE &lt; 6.4 es erkennen). Der
    /// Gedanke war: Wer das Menü selbst öffnet, kann es nicht schliessen —
    /// also soll es die Leiste öffnen, dann schliesst sie es auch.
    ///
    /// Gemessen am 27.10.2026 im Quelltext der Leiste, die Bastian benutzt
    /// (Omarchy 4.0.0.alpha, **quickshell**, nicht waybar —
    /// `/usr/share/omarchy/shell/plugins/bar/widgets/Tray.qml`):
    ///
    /// - Ein Linksklick auf ein Item mit `onlyMenu` (= `ItemIsMenu`) ruft
    ///   `openTrayMenu()`, und das endet **immer** mit `trayMenuOpen = true`.
    ///   Es gibt kein Umschalten; `resetTrayMenu()` schliesst nichts. Ein
    ///   zweiter Linksklick öffnet also neu statt zu schliessen.
    /// - Der Fokus-Griff des Menüs (`HyprlandFocusGrab` in
    ///   `Ui/PopupCard.qml`) listet `[popup, Leistenfenster]`. Ein Klick AUF
    ///   der Leiste klärt ihn deshalb nie.
    ///
    /// Beides gehört der Leiste und nicht uns. Gemeldet wird es dort; gepatcht
    /// wird dort nichts — ein Patch an einer Paketdatei ist beim nächsten
    /// Update weg, und Bastian braucht ein Tray, das sich heute wie die
    /// anderen bedient.
    ///
    /// > Dass eine Begründung technisch richtig ist, heisst nicht, dass das
    /// > Ergebnis stimmt. „Die Leiste soll das Menü öffnen" war korrekt
    /// > hergeleitet — und die Leiste macht daraus kein Umschalten.
    ///
    /// Die Umschaltung der Leitung bleibt da, wo sie seit v0.5.0 sitzt: als
    /// Häkchen im Menü, nicht am Klick auf das Symbol.
    const MENU_ON_ACTIVATE: bool = false;

    /// Linksklick: AI Brain im Browser öffnen (#1124).
    ///
    /// Die Entscheidung dahinter steht an {@link crate::kern::Kern::brain_oeffnen}.
    /// Hier ist nur die Anbindung — und sie ist bewusst dünn: `activate`
    /// läuft im D-Bus-Faden, und alles, was länger dauert, gehört nicht
    /// hinein. `oeffnen` startet den Browser und wartet nicht auf ihn.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.kern.brain_oeffnen();
    }

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

        // ## Ein Häkchen, kein Knopf (Stand 27.10.2026)
        //
        // Ein Knopf „Live einschalten" sagt nicht, ob sie gerade an ist — man
        // muss die Zeilen oben lesen, um es zu wissen. Ein Häkchen trägt den
        // Zustand und die Handlung in einem, und es ist das, was ein Menü in
        // jedem anderen Programm dafür anbietet.
        //
        // Dahinter liegt dieselbe Logik wie vorher am Linksklick: Haken rein
        // heisst `leitung an` (hebt ein Veto auf), Haken raus heisst
        // `leitung aus` (setzt es). Siehe `Kern::umschalten`.
        eintraege.push(
            CheckmarkItem {
                label: "Live-Leitung".into(),
                checked: an,
                // Ohne Antwort nicht schalten: Ein Klick, der mangels Wissen
                // das Gegenteil des Gewollten tut, wäre der nächste Ärger.
                enabled: self.kern.weiss_etwas(),
                activate: Box::new(|t: &mut Self| t.kern.umschalten()),
                ..Default::default()
            }
            .into(),
        );

        for (beschriftung, dauer) in DAUERN {
            eintraege.push(
                StandardItem {
                    label: format!("… ab jetzt {beschriftung}"),
                    activate: Box::new(move |t: &mut Self| t.kern.einschalten(dauer)),
                    ..Default::default()
                }
                .into(),
            );
        }

        // ## „Aufräumen" erscheint nur, wenn es etwas aufzuräumen gibt
        //
        // Bis zum 27.10.2026 stand hier ein dauerhaftes „Live ausschalten"
        // mit `enabled: an`. Das war im Störfall falsch — Schalter aus,
        // Prozess läuft weiter, und genau der Punkt, der aufräumt, war
        // ausgegraut. Dann stand es dauerhaft da und war `enabled`.
        //
        // Jetzt trägt das Häkchen den Normalfall, und dieser Punkt bleibt für
        // den Fall, den das Häkchen NICHT abdeckt: Steht der Schalter auf
        // aus, während ein Prozess weiterläuft, ist das Häkchen leer — ein
        // Klick darauf würde einschalten, nicht aufräumen. Deshalb hier ein
        // eigener Weg, und zwar sichtbar nur dann.
        if self.kern.aufraeumen_noetig() {
            eintraege.push(
                StandardItem {
                    label: "Leitung aufräumen (Prozess läuft ohne Schalter)".into(),
                    activate: Box::new(|t: &mut Self| t.kern.ausschalten()),
                    ..Default::default()
                }
                .into(),
            );
        }

        eintraege.push(MenuItem::Separator);

        // Derselbe Weg wie der Linksklick, nur sichtbar. Ein Klick, dessen
        // Wirkung nirgends steht, ist ein Geheimnis; und ohne bekannte
        // Adresse sagt der ausgegraute Punkt, woran es liegt.
        eintraege.push(
            StandardItem {
                label: "AI Brain öffnen".into(),
                enabled: self.kern.instanz_adresse().is_some(),
                activate: Box::new(|t: &mut Self| t.kern.brain_oeffnen()),
                ..Default::default()
            }
            .into(),
        );

        eintraege.push(
            StandardItem {
                label: "Protokoll öffnen".into(),
                enabled: self.kern.protokoll().is_some(),
                activate: Box::new(|t: &mut Self| t.kern.protokoll_oeffnen()),
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
