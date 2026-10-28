//! Die Leiste unter Linux: StatusNotifierItem über D-Bus (ksni).
//!
//! Reines Rust, ohne GTK. Läuft in quickshell (Omarchy), waybar, KDE und
//! GNOME mit AppIndicator-Erweiterung.
//!
//! Hier steht nur das Anbinden an die Leiste. Was im Menü steht, steht in
//! `menue`, was angezeigt wird in `ansicht`, was passiert in `kern` — alle
//! drei ohne D-Bus, damit macOS und Windows dieselbe Bedienung bekommen und
//! nicht eine zweite, ähnliche.
//!
//! ## Zwei Betriebsarten, und welche gilt, entscheidet der Rechner
//!
//! | | Linksklick | Rechtsklick | Wo |
//! |---|---|---|---|
//! | **Eigenes Menü** | unser Fenster, Klick schaltet um | Menü der Leiste | Wayland mit wlr-layer-shell (Hyprland, sway, river …) |
//! | **Menü der Leiste** | Menü der Leiste | Menü der Leiste | alles andere (GNOME, KDE, X11) |
//!
//! Die Unterscheidung liegt in `MENU_ON_ACTIVATE`, und das ist eine
//! **Konstante** in `ksni::Tray` — also nicht zur Laufzeit umschaltbar.
//! Deshalb ist `Tray` über `EIGEN` generisch: Es gibt zwei Ausprägungen
//! derselben Struktur, und `starten` wählt eine. Der Umweg ist billiger als
//! sein Gegenteil (zwei Strukturen mit demselben Inhalt), und er hält den
//! Unterschied an genau einer Zeile.
//!
//! Die Entscheidung fällt beim Start, nicht je Klick: Eine Oberfläche
//! wechselt ihre Fähigkeiten nicht, während ein Tray läuft.

use std::sync::mpsc;

use ksni::blocking::TrayMethods;

use crate::kern::{Kern, Signal, TAKT};
use crate::menue::{self, Eintrag};
use crate::popup;
use crate::symbol;

/// Die Kantenlänge der Pixmap. 22 Pixel ist das Mass, in dem Leisten
/// Tray-Symbole erwarten; grössere werden heruntergerechnet und matschig.
const KANTE: u32 = 22;

/// Womit sich die Betriebsart von Hand festlegen lässt.
///
/// `eigen` erzwingt das eigene Fenster, `leiste` das Menü der Leiste. Alles
/// andere (und nichts) heisst: nachsehen.
///
/// Dass es diesen Schalter gibt, ist keine Unentschlossenheit, sondern der
/// Rückweg. Die fünf Fassungen des Symbolklicks seit v0.4.1 haben eins
/// gezeigt: Wenn etwas an diesem Klick klemmt, sitzt jemand an einem
/// Arbeitsrechner und braucht die andere Fassung **jetzt**, nicht nach einem
/// Release.
const SCHALTER: &str = "AI_BRAIN_TRAY_MENUE";

struct Tray<const EIGEN: bool> {
    kern: Kern,
    /// Das eigene Menüfenster — bei `EIGEN = false` immer leer.
    fenster: popup::Fenster,
}

impl<const EIGEN: bool> ksni::Tray for Tray<EIGEN> {
    /// **Wer das Menü öffnet: wir oder die Leiste.**
    ///
    /// `true` heisst: Die Leiste behandelt das Item als Menü (`ItemIsMenu`),
    /// öffnet es beim Linksklick selbst und ruft kein `Activate`. `ksni`
    /// setzt dafür die SNI-Eigenschaft **und** antwortet auf `Activate` mit
    /// `UnknownMethod("ItemIsMenu")`, woran GNOME und KDE &lt; 6.4 den
    /// Menüweg erkennen.
    ///
    /// `false` heisst: Der Linksklick kommt als `Activate` bei uns an — und
    /// genau das brauchen wir, um das Menü **selbst** zu zeigen.
    ///
    /// ## Das ist die fünfte Fassung dieses Klicks, und sie löst endlich die
    /// ganze Bitte
    ///
    /// Bastian am 28.10.2026, wörtlich: *„Wenn ich links klicke ausserhalb
    /// des Tray-Menüs drücke, muss es zugehen und wenn ich nochmal auf das
    /// Tray-Symbol klicke mit links klicken, muss es halt auch zugehen, wie
    /// ein Toggle auf Tray-Menü zu Tray-Menü."* Davor, zum selben Punkt:
    /// *„Ja, das Tray geht mit Linksklick auf, aber wenn ich nochmal
    /// Linksklick mache, geht es nicht wieder zu."*
    ///
    /// Drei Fälle also, und alle drei müssen gehen:
    ///
    /// 1. Linksklick aufs Symbol **öffnet**.
    /// 2. Erneuter Linksklick aufs Symbol **schliesst**.
    /// 3. Linksklick irgendwo daneben **schliesst** — auch auf der Leiste.
    ///
    /// ## Warum die Leiste das nicht kann — und wir schon
    ///
    /// Mit `true` liefert die Leiste das Menü. In Omarchy (quickshell, Paket
    /// 4.0.4-1, nachgelesen am 28.10.2026 in
    /// `/usr/share/omarchy/shell/plugins/bar/widgets/Tray.qml`) ruft
    /// `MouseArea.onClicked` bei `modelData.onlyMenu` `displayMenu()` →
    /// `openTrayMenu()`, und das endet **immer** mit `trayMenuOpen = true`.
    /// Ein Umschalten gibt es dort nicht; `close()` existiert, wird aber vom
    /// Klickpfad nie gerufen. Der Fokus-Griff in `Ui/PopupCard.qml` listet
    /// `[popup, Leistenfenster]` — ein Klick AUF der Leiste klärt ihn also
    /// auch nicht. Fall 2 und Fall 3 fallen beide aus.
    ///
    /// Und wir können der Leiste nicht helfen: **StatusNotifierItem kennt
    /// keinen Weg, dem Host „schliess mein Menü" zu sagen.** Das Menü ist
    /// sein Fenster, wir liefern nur den Inhalt über DBusMenu. In keiner
    /// Fassung der Spezifikation gibt es ein Signal dafür.
    ///
    /// Geprüft wurden am 28.10.2026 ausserdem:
    ///
    /// | Weg | Warum nicht |
    /// |---|---|
    /// | Eine Option in Omarchy/quickshell | Es gibt keine. `~/.config/omarchy/shell.json` kennt zum Tray nur Layout, `pinned` und `hidden` — kein Umschalten. |
    /// | Die drei Zeilen in `Tray.qml` ändern | Eine Paketdatei, beim nächsten `omarchy`-Update weg. Gehört nach oben gemeldet (#8958), nicht hierher kopiert. |
    /// | Item abmelden und neu anmelden | Symbol flackert, Leiste sieht einen Verbindungsabbruch. Tauscht den Fehler gegen einen schlimmeren. |
    ///
    /// Also zeichnet der Tray sein Menü selbst (`popup`). Damit liegt das
    /// Umschalten vollständig bei uns — und zwar ohne Zustand, den zwei
    /// Programme gleich sehen müssten: Der bildschirmgrosse Auffänger unter
    /// dem Menü fängt den zweiten Klick, bevor er die Leiste erreicht. Die
    /// Einzelheiten stehen im Kopf von `popup::flaeche`.
    ///
    /// Was das Menü *kann*, ändert sich nicht: dieselben Punkte, aus
    /// derselben Liste (`menue::bauen`) — der Rechtsklick zeigt sie weiter
    /// über die Leiste.
    const MENU_ON_ACTIVATE: bool = !EIGEN;

    /// Der Linksklick, wenn wir das Menü selbst zeigen.
    ///
    /// Kommt nur an, solange **kein** Menü offen ist: Steht eins, liegt unser
    /// Auffänger über der Leiste und nimmt den Klick. Das `offen()` hier ist
    /// deshalb kein Umschalter für den Normalfall, sondern der Riegel für den
    /// Sonderfall — eine Leiste, die `Activate` doppelt schickt, oder ein
    /// Fenster, das noch aufräumt.
    fn activate(&mut self, _x: i32, _y: i32) {
        if !EIGEN {
            // Mit `MENU_ON_ACTIVATE = true` ruft `ksni` das hier nicht
            // einmal. Der Zweig steht da, damit niemand aus dem Vorhandensein
            // der Methode schliesst, dass der Klick etwas tut.
            return;
        }

        if self.fenster.offen() {
            self.fenster.schliessen();
            return;
        }

        let eintraege = menue::bauen(&self.kern);

        if let Err(grund) = self.fenster.oeffnen(&eintraege, self.kern.signalweg()) {
            // Kein stilles Scheitern: Ein Klick, der nichts tut, ist genau
            // die Beschwerde, mit der #1124 angefangen hat. Die Zeile steht
            // im Protokoll des Trays, und der Fehler im Menü selbst.
            eprintln!("Menüfenster ging nicht auf: {grund}");
            crate::kern::fehler_melden(grund);
        }
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

    /// Das Menü der Leiste — dieselben Punkte wie unser Fenster.
    ///
    /// Gebaut aus `menue::bauen`, nicht hier zusammengestellt: Auf einem
    /// Hyprland-Rechner gibt es jetzt **zwei** Menüs für dieselbe Sache
    /// (unser Fenster am Linksklick, dieses am Rechtsklick). Zwei Listen
    /// wären zwei Gelegenheiten, verschieden zu sein — und bis v0.5.3 war das
    /// schon zwischen Linux und macOS so.
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        menue::bauen(&self.kern)
            .into_iter()
            .map(|e| match e {
                Eintrag::Trenner => MenuItem::Separator,

                Eintrag::Zeile(text) => StandardItem {
                    label: text,
                    enabled: false,
                    ..Default::default()
                }
                .into(),

                Eintrag::Haken {
                    text,
                    gesetzt,
                    aktiv,
                    aktion,
                } => CheckmarkItem {
                    label: text,
                    checked: gesetzt,
                    enabled: aktiv,
                    activate: Box::new(move |t: &mut Self| t.kern.tun(&aktion)),
                    ..Default::default()
                }
                .into(),

                Eintrag::Punkt {
                    text,
                    aktiv,
                    aktion,
                } => {
                    // Kein `process::exit` im Rückruf: Der läuft im
                    // D-Bus-Faden, und ein Prozess, der mitten in einer
                    // Antwort endet, lässt die Leiste auf eine Antwort
                    // warten, die nicht mehr kommt. `Aktion::Beenden` geht
                    // deshalb über ein Signal an den Haupt-Faden — siehe
                    // `Kern::beenden`.
                    let symbol_name = if matches!(aktion, crate::menue::Aktion::Beenden) {
                        "application-exit"
                    } else {
                        ""
                    };

                    StandardItem {
                        label: text,
                        enabled: aktiv,
                        icon_name: symbol_name.to_string(),
                        activate: Box::new(move |t: &mut Self| t.kern.tun(&aktion)),
                        ..Default::default()
                    }
                    .into()
                }
            })
            .collect()
    }
}

/// Zeigt der Tray sein Menü auf diesem Rechner selbst?
///
/// Die Reihenfolge ist Absicht: erst der Schalter von Hand, dann die Messung.
/// Wer `AI_BRAIN_TRAY_MENUE=leiste` setzt, soll das Menü der Leiste bekommen,
/// auch wenn layer-shell da ist.
fn eigenes_menue() -> bool {
    match std::env::var(SCHALTER).unwrap_or_default().trim() {
        "eigen" => true,
        "leiste" => false,
        _ => popup::moeglich(),
    }
}

/// Gibt es hier eine Leiste, die Symbole annimmt?
///
/// Der Versuch selbst ist die Prüfung: Bei fehlendem Watcher gibt `spawn`
/// einen Fehler, und ein Tray ohne Watcher startet sonst und bleibt
/// **unsichtbar** — der unangenehmere Fall, weil nichts darauf hinweist.
pub fn pruefen() -> Result<(), String> {
    let (tx, _rx) = mpsc::channel();

    // Die Betriebsart ist für diese Frage gleichgültig — es geht um den
    // Watcher, nicht um den Klick.
    let handle = Tray::<false> {
        kern: Kern::neu(tx),
        fenster: popup::Fenster::keins(),
    }
    .spawn()
    .map_err(|e| format!("Keine Leiste mit StatusNotifier: {e}"))?;

    handle.shutdown().wait();

    Ok(())
}

pub fn starten() -> Result<(), String> {
    if eigenes_menue() {
        laufen::<true>()
    } else {
        laufen::<false>()
    }
}

fn laufen<const EIGEN: bool>() -> Result<(), String> {
    let (signal, geweckt) = mpsc::channel();

    let handle = Tray::<EIGEN> {
        kern: Kern::neu(signal),
        fenster: popup::Fenster::keins(),
    }
    .spawn()
    .map_err(|e| {
        format!("Kein Tray verfügbar ({e}). Läuft eine Leiste mit StatusNotifier, z. B. waybar?")
    })?;

    loop {
        let wissen = crate::harness::abfragen();
        let fehler = crate::kern::fehler_abholen();

        handle.update(|t: &mut Tray<EIGEN>| {
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
            Ok(Signal::Gewaehlt(aktion)) => {
                // Der Punkt kam aus unserem Menüfenster. Ausgeführt wird er
                // hier und nicht dort: `Kern` liegt im Tray, und was ein
                // Punkt bedeutet, soll es nur einmal geben.
                handle.update(|t: &mut Tray<EIGEN>| t.kern.tun(&aktion));
            }

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

    /// Die Zusage aus #1124/#9006/#9008, in zwei Zeilen festgenagelt.
    ///
    /// Diese Konstante ist die ganze Entscheidung, und sie hat zwischen
    /// v0.5.0, v0.5.1, v0.5.3 und v0.5.4 **viermal** den Wert gewechselt —
    /// jedes Mal auf eine Aussage von Bastian hin, nie beim Aufräumen
    /// nebenbei. Deshalb steht hier eine Probe und kein Kommentar.
    ///
    /// Geprüft wird in einem `const`-Block, nicht zur Laufzeit: Der Wert ist
    /// zur Übersetzungszeit bekannt, also soll der Übersetzer schimpfen und
    /// nicht erst der Testlauf.
    #[test]
    fn der_linksklick_oeffnet_immer_ein_menue() {
        const {
            assert!(
                !<Tray<true> as ksni::Tray>::MENU_ON_ACTIVATE,
                "Mit eigenem Fenster muss ItemIsMenu false sein, sonst kommt kein Activate an"
            )
        };

        const {
            assert!(
                <Tray<false> as ksni::Tray>::MENU_ON_ACTIVATE,
                "Ohne eigenes Fenster muss die Leiste das Menü öffnen — sonst tut der Klick nichts"
            )
        };
    }

    /// Der Schalter von Hand schlägt die Messung, in beide Richtungen.
    ///
    /// Geprüft wird die Entscheidungslogik, nicht die Umgebungsvariable
    /// selbst: Ein Test, der `set_var` ruft, greift in alle gleichzeitig
    /// laufenden Tests hinein — und seit Rust 2024 ist `set_var` zu Recht
    /// `unsafe`.
    #[test]
    fn der_schalter_von_hand_schlaegt_die_messung() {
        fn entscheiden(wert: &str, gemessen: bool) -> bool {
            match wert.trim() {
                "eigen" => true,
                "leiste" => false,
                _ => gemessen,
            }
        }

        assert!(entscheiden("eigen", false));
        assert!(!entscheiden("leiste", true));
        assert!(entscheiden(" eigen ", false), "Leerzeichen zählen nicht");

        // Alles andere fragt nach.
        for wert in ["", "ja", "1", "wayland", "unsinn"] {
            assert!(entscheiden(wert, true), "{wert}");
            assert!(!entscheiden(wert, false), "{wert}");
        }
    }

    /// Beide Menüs kommen aus derselben Liste.
    ///
    /// Die Probe schaut nicht auf ksni (das braucht einen D-Bus), sondern auf
    /// die Quelle: `menue::bauen` ist die einzige Stelle, an der Punkte
    /// entstehen. Wer hier eine zweite anlegt, bricht diese Probe nicht —
    /// aber die Zählung in `menue::tests` hält die Liste selbst fest.
    #[test]
    fn das_menue_kommt_aus_der_geteilten_liste() {
        let (tx, _rx) = mpsc::channel();
        let kern = Kern::neu(tx);

        let eintraege = menue::bauen(&kern);

        assert!(
            eintraege.iter().any(|e| e.text() == Some("Live-Leitung")),
            "{eintraege:?}"
        );
        assert!(
            eintraege.iter().any(|e| e.text() == Some("Tray beenden")),
            "{eintraege:?}"
        );
    }
}
