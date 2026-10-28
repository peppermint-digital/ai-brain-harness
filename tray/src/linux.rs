//! Die Leiste unter Linux: StatusNotifierItem über D-Bus (ksni).
//!
//! Reines Rust, ohne GTK. Läuft in quickshell (Omarchy), waybar, KDE und
//! GNOME mit AppIndicator-Erweiterung.
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
    /// **Ein Klick öffnet das Menü — links wie rechts.**
    ///
    /// `true` heisst: Die Leiste behandelt das Item als Menü (`ItemIsMenu`),
    /// öffnet es beim Linksklick selbst und ruft kein `Activate`. `ksni`
    /// setzt dafür die SNI-Eigenschaft **und** antwortet auf `Activate` mit
    /// `UnknownMethod("ItemIsMenu")`, woran GNOME und KDE &lt; 6.4 den
    /// Menüweg erkennen.
    ///
    /// ## Das ist die vierte Fassung dieses Klicks, und sie ist eine Ansage
    ///
    /// Bastian am 28.10.2026, wörtlich: *„Der Harness macht jetzt auf Klick
    /// AI-Brain auf, aber der sollte doch einfach nur das Tray-Menü
    /// aufmachen. Mit Linksklick."*
    ///
    /// Damit ist v0.5.1 zurückgenommen. Dort stand hier `false`, und der
    /// Linksklick öffnete AI Brain im Browser — auf Grundlage seiner Antwort
    /// „wie die anderen" auf eine Rückfrage zu Bug #1124. Gemeint war etwas
    /// anderes, als wir daraus gelesen haben. Die Entscheidung steht also
    /// nicht mehr auf einer Auslegung, sondern auf einem Satz.
    ///
    /// Was das Menü *kann*, ändert sich nicht: Das Häkchen „Live-Leitung"
    /// schaltet die Leitung, und „AI Brain öffnen" ist weiter drin — der Weg
    /// aus v0.5.1 bleibt, er hängt nur nicht mehr am Klick auf das Symbol.
    ///
    /// ## Die Kehrseite ist bekannt, gemessen und bleibt
    ///
    /// In Omarchys Leiste (quickshell, Paket 4.0.4-1) schliesst ein **zweiter
    /// Linksklick das Menü nicht**; nur ein Klick ausserhalb der Leiste macht
    /// es zu. Nachgelesen am 28.10.2026 im Quelltext
    /// `/usr/share/omarchy/shell/plugins/bar/widgets/Tray.qml`
    /// (unverändert gegenüber dem 27.10.):
    ///
    /// - `MouseArea.onClicked` ruft bei `modelData.onlyMenu` (= `ItemIsMenu`)
    ///   `displayMenu()` → `openTrayMenu()`, und das endet **immer** mit
    ///   `trayMenuOpen = true`. Ein Umschalten gibt es nicht; `close()`
    ///   existiert, wird aber von dort nicht gerufen.
    /// - Der Fokus-Griff (`HyprlandFocusGrab` in `Ui/PopupCard.qml`) listet
    ///   `[popup, Leistenfenster]`. Ein Klick AUF der Leiste klärt ihn
    ///   deshalb nie.
    ///
    /// **Auf unserer Seite ist das nicht umgehbar**, und das ist keine
    /// Bequemlichkeit: StatusNotifierItem kennt keinen Weg, dem Host „schliess
    /// mein Menü" zu sagen — das Menü ist sein Fenster, wir liefern nur den
    /// Inhalt über DBusMenu. Mit `ItemIsMenu = true` erfahren wir vom Klick
    /// nicht einmal (`Activate` kommt nicht mehr an), und selbst wenn:
    /// Abmelden und neu anmelden, um ein Popup wegzubekommen, würde das
    /// Symbol flackern lassen und den Fehler gegen einen schlimmeren
    /// tauschen. Gemeldet wird der Punkt bei Omarchy (#8958); gepatcht wird
    /// dort nichts, ein Patch an einer Paketdatei ist beim nächsten Update
    /// weg.
    ///
    /// Bastian kennt die Kehrseite und will den Linksklick trotzdem. Siehe
    /// Bug #1124.
    const MENU_ON_ACTIVATE: bool = true;

    // Kein `activate` mehr: Mit `MENU_ON_ACTIVATE = true` ruft `ksni` es
    // nicht, sondern beantwortet `Activate` mit `UnknownMethod`. Eine
    // Behandlung hier wäre toter Code, der aussieht wie eine Zusage — und
    // „Linksklick öffnet AI Brain" stand genau so bis v0.5.2 hier. Den Weg
    // selbst gibt es weiter, sichtbar als Menüpunkt „AI Brain öffnen".

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

        // Der Weg zu AI Brain — seit v0.5.3 nur noch hier, nicht mehr am
        // Linksklick (der öffnet das Menü). Ohne bekannte Adresse ist der
        // Punkt ausgegraut und sagt damit, woran es liegt; eine erfundene
        // URL wäre schlimmer als ein Klick, der nichts tut.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Die Zusage aus #1124/#9006, in einer Zeile festgenagelt.
    ///
    /// Diese Konstante ist die ganze Entscheidung: `true` heisst, die Leiste
    /// öffnet das Menü beim Linksklick selbst. Sie hat zwischen v0.5.0,
    /// v0.5.1 und v0.5.3 dreimal den Wert gewechselt, und jedes Mal war der
    /// Grund eine Aussage von Bastian — nicht etwas, das man beim Aufräumen
    /// nebenbei umdreht. Deshalb steht hier eine Probe und kein Kommentar.
    ///
    /// Geprüft wird sie in einem `const`-Block, nicht zur Laufzeit: Der Wert
    /// ist zur Übersetzungszeit bekannt, also soll der Übersetzer schimpfen
    /// und nicht erst der Testlauf.
    #[test]
    fn der_linksklick_oeffnet_das_menue() {
        const {
            assert!(
                <Tray as ksni::Tray>::MENU_ON_ACTIVATE,
                "ItemIsMenu muss true bleiben — Bastians Wunsch vom 28.10.2026"
            )
        };
    }
}
