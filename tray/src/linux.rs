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
//! ## Ein Menü, und der Linksklick schaltet (Stand 29.10.2026, #9060)
//!
//! | | Linksklick | Rechtsklick |
//! |---|---|---|
//! | **`schalter`** (Vorgabe) | Live-Leitung an (30 min) oder aus | **das** Menü, von der Leiste gezeigt |
//! | `eigen` | unser Menüfenster auf und zu | Menü der Leiste — also wieder zwei |
//! | `leiste` | Menü der Leiste | Menü der Leiste |
//!
//! Bis v0.6.0 war `eigen` die Vorgabe. Damit gab es auf einem
//! Hyprland-Rechner **zwei** Menüs über dieselbe Sache: unser Fenster am
//! Linksklick, das der Leiste am Rechtsklick. Bastian am 29.10.2026,
//! wörtlich: *„Es gibt jetzt ein Rechtsklick und ein Linksklick-Tray-Menü.
//! Das macht ja keinen Sinn."*
//!
//! Gebeten hat er um „Linksklick = Schalter, Rechtsklick = unser Menü, und
//! der zweite Rechtsklick macht es wieder zu". Die erste Hälfte steht hier.
//! Die zweite ist auf Omarchy nicht zu bauen, und warum, steht gemessen an
//! `MENU_ON_ACTIVATE`.
//!
//! `MENU_ON_ACTIVATE` ist eine **Konstante** in `ksni::Tray`, also nicht zur
//! Laufzeit umschaltbar. Deshalb ist `Tray` über `LEISTE` generisch: Es gibt
//! zwei Ausprägungen derselben Struktur, und `starten` wählt eine. Was der
//! Klick dann *tut*, steht als `Klick` daneben — dafür braucht es keine
//! Konstante, und zwei Betriebsarten teilen sich dieselbe Ausprägung.
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
/// `schalter` (oder nichts) heisst: Der Linksklick schaltet die Leitung.
/// `eigen` erzwingt unser Menüfenster am Linksklick, `leiste` das Menü der
/// Leiste. Alles andere zählt wie nichts.
///
/// Dass es diesen Schalter gibt, ist keine Unentschlossenheit, sondern der
/// Rückweg. Die sechs Fassungen des Symbolklicks seit v0.4.1 haben eins
/// gezeigt: Wenn etwas an diesem Klick klemmt, sitzt jemand an einem
/// Arbeitsrechner und braucht die andere Fassung **jetzt**, nicht nach einem
/// Release.
///
/// Seit v0.6.1 wird nicht mehr von vornherein nachgesehen, ob es layer-shell
/// gibt: Die Vorgabe braucht keins. Nur `eigen` braucht es — und genau dort
/// wird gemessen.
const SCHALTER: &str = "AI_BRAIN_TRAY_MENUE";

/// Was der Linksklick auf das Symbol tut.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Klick {
    /// Die Live-Leitung an (30 min) oder aus — die Vorgabe seit v0.6.1,
    /// und dasselbe, was das Häkchen im Menü tut (`Kern::umschalten`).
    Schalter,
    /// Unser eigenes Menüfenster auf und zu (v0.5.4 bis v0.6.0).
    EigenesFenster,
    /// Die Leiste öffnet ihr Menü (v0.5.3). `Activate` kommt dann nicht an.
    MenueDerLeiste,
}

struct Tray<const LEISTE: bool> {
    kern: Kern,
    /// Was der Linksklick tut. Bei `LEISTE = true` wird er nie gefragt —
    /// dann ruft `ksni` `activate` gar nicht.
    klick: Klick,
    /// Das eigene Menüfenster — nur bei `Klick::EigenesFenster` benutzt.
    fenster: popup::Fenster,
}

impl<const LEISTE: bool> ksni::Tray for Tray<LEISTE> {
    /// **Wer das Menü öffnet: wir oder die Leiste.**
    ///
    /// `true` heisst: Die Leiste behandelt das Item als Menü (`ItemIsMenu`),
    /// öffnet es beim Linksklick selbst und ruft kein `Activate`. `ksni`
    /// setzt dafür die SNI-Eigenschaft **und** antwortet auf `Activate` mit
    /// `UnknownMethod("ItemIsMenu")`, woran GNOME und KDE &lt; 6.4 den
    /// Menüweg erkennen.
    ///
    /// `false` heisst: Der Linksklick kommt als `Activate` bei uns an — und
    /// genau das braucht die Vorgabe, denn der Linksklick **schaltet**.
    ///
    /// ## Das ist die sechste Fassung dieses Klicks (29.10.2026, #9060)
    ///
    /// Sie geht dahin zurück, wo v0.4.2 angefangen hat: Der Linksklick
    /// schaltet die Live-Leitung. Dazwischen lagen vier Fassungen, und die
    /// Liste steht im README.
    ///
    /// ## Warum der Rechtsklick NICHT unser Fenster zeigen kann
    ///
    /// Gebeten war: links schalten, rechts unser Menü, zweiter Rechtsklick
    /// macht es zu. Gemessen am Quelltext von Omarchy 4.0.4-1 /
    /// quickshell 0.3.1 am 29.10.2026 — und daran scheitert die zweite
    /// Hälfte, nicht an uns:
    ///
    /// | Frage | Befund |
    /// |---|---|
    /// | Wie meldet quickshell den Rechtsklick? | `Tray.qml`, `MouseArea.onPressed` → `displayMenu()` → `openTrayMenu()`. Bieten wir ein dbusmenu an, endet das **immer** mit `trayMenuOpen = true` — die Karte der Leiste, ohne Umschalten. |
    /// | Und wenn wir keins anbieten? | `openTrayMenu` nimmt den Zweig `!item.menu` und ruft `item.display(…)`. Das endet in quickshell mit `qCritical "No menu present for"`: Es passiert **nichts**. |
    /// | Ruft quickshell `ContextMenu`? | **Nie.** Im Programm gibt es genau eine Fundstelle der Zeichenkette, und die liegt im `qt_static_metacall` des erzeugten DBus-Stellvertreters. Die QML-Schnittstelle (`quickshell-service-statusnotifier.qmltypes`) kennt nur `activate`, `secondaryActivate`, `scroll` und `display` — kein `contextMenu`. |
    /// | Könnte ein spec-treuer Host uns erreichen? | Heute nicht: `ksni` 0.3.7 beantwortet `ContextMenu` selbst mit `UnknownMethod("Not supported, please use `menu`")` und gibt die Eigenschaft `Menu` fest als `/MenuBar` heraus. „Kein dbusmenu anbieten" geht damit gar nicht. |
    ///
    /// **Folge: Auf Omarchy erfahren wir nur den Linksklick.** „Links
    /// schaltet" und „unser Fenster bleibt erreichbar" schliessen sich dort
    /// also aus — es gibt nur einen Klick, den wir bekommen, und das Menü
    /// muss erreichbar bleiben. Deshalb zeigt es die Leiste, am Rechtsklick,
    /// und es ist das einzige.
    ///
    /// Was dabei verloren geht, und das ist benannt statt behauptet: Ein
    /// zweiter Rechtsklick schliesst die Karte der Leiste nicht, und Esc
    /// schliesst sie auch nicht (`Ui/PopupCard.qml` hat keine Tastenbehandlung).
    /// Ein Klick daneben schliesst sie — ihr `HyprlandFocusGrab` listet
    /// `[Karte, Leistenfenster]`, also alles ausser Leiste und Karte.
    ///
    /// Der Befund liegt bei Omarchy, nicht bei uns, und er ist in Task #8958
    /// festgehalten. Bastian hat am 28.10.2026 entschieden, ihn **nicht** zu
    /// melden; nachgebaut wird er hier erst recht nicht.
    ///
    /// Verworfen, weil jeder Weg schlimmer wäre als das, was er behebt:
    ///
    /// | Weg | Warum nicht |
    /// |---|---|
    /// | Eine eigene Fläche dauerhaft über dem Symbol | Wir wüssten nicht verlässlich, wo das Symbol liegt (der Bereich fährt ein und aus, Symbole kommen und gehen), und sie würde Hover und Tooltip der Leiste schlucken. |
    /// | `Tray.qml` ändern | Eine Paketdatei, beim nächsten `omarchy`-Update weg. Der Befund steht in #8958. |
    /// | Ein leeres dbusmenu anbieten | Die Karte geht trotzdem auf: `fittedContentHeight` setzt den Innenabstand als Untergrenze, es erscheint ein leerer Kasten von etwa 232×20 px. |
    /// | Bei `AboutToShow` einen Auffänger aufziehen | Er läge **über** der Karte und würde deren Zeilen schlucken. Nur die Leistenzeile abzudecken wäre denkbar, hängt aber daran, wie Hyprland den Fokus-Griff löst — vorgemerkt, bis Bastian die jetzige Fassung benutzt hat. |
    ///
    /// Auf waybar, KDE, GNOME, macOS und Windows ist die Bitte damit
    /// **vollständig** erfüllt: Dort ist das Menü am Rechtsklick ein
    /// richtiges Menü mit Griff, das beim zweiten Klick und beim Klick
    /// daneben zugeht. Fehlen tut nur Omarchys Karte etwas.
    const MENU_ON_ACTIVATE: bool = LEISTE;

    /// Der Linksklick.
    ///
    /// Kommt nur an, solange `MENU_ON_ACTIVATE` falsch ist — und bei
    /// `Klick::EigenesFenster` zusätzlich nur, solange kein Menü offen ist:
    /// Steht eins, liegt unser Auffänger über der Leiste und nimmt den Klick.
    fn activate(&mut self, _x: i32, _y: i32) {
        match self.klick {
            // Die Vorgabe. Was dabei entschieden wird — und dass ohne
            // Antwort NICHT geschaltet wird —, steht in `Kern::umschalten`
            // und gilt für das Häkchen im Menü genauso.
            Klick::Schalter => self.kern.umschalten(),

            Klick::EigenesFenster => {
                if self.fenster.offen() {
                    self.fenster.schliessen();
                    return;
                }

                let eintraege = menue::bauen(&self.kern);

                if let Err(grund) = self.fenster.oeffnen(&eintraege, self.kern.signalweg()) {
                    // Kein stilles Scheitern: Ein Klick, der nichts tut, ist
                    // genau die Beschwerde, mit der #1124 angefangen hat. Die
                    // Zeile steht im Protokoll des Trays, und der Fehler im
                    // Menü selbst.
                    eprintln!("Menüfenster ging nicht auf: {grund}");
                    crate::kern::fehler_melden(grund);
                }
            }

            // Mit `MENU_ON_ACTIVATE = true` ruft `ksni` das hier nicht
            // einmal. Der Zweig steht da, damit niemand aus dem Vorhandensein
            // der Methode schliesst, dass der Klick etwas tut.
            Klick::MenueDerLeiste => {}
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

    /// Das Menü — in der Vorgabe **das** Menü, am Rechtsklick.
    ///
    /// Gebaut aus `menue::bauen`, nicht hier zusammengestellt: Unter
    /// `AI_BRAIN_TRAY_MENUE=eigen` gibt es zwei Menüs für dieselbe Sache
    /// (unser Fenster am Linksklick, dieses am Rechtsklick), und zwei Listen
    /// wären zwei Gelegenheiten, verschieden zu sein — bis v0.5.3 war das
    /// zwischen Linux und macOS schon so.
    ///
    /// Dass diese Liste da ist, ist ausserdem die Bedingung dafür, dass der
    /// Rechtsklick überhaupt etwas tut: Ohne dbusmenu zeigt quickshell gar
    /// nichts. Siehe `MENU_ON_ACTIVATE`.
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

/// Was der Linksklick auf diesem Rechner tun soll.
///
/// Getrennt von der Umgebungsvariablen, damit die Entscheidung prüfbar ist:
/// Ein Test, der `set_var` ruft, greift in alle gleichzeitig laufenden Tests
/// hinein — und seit Rust 2024 ist `set_var` zu Recht `unsafe`.
///
/// `layer_shell` ist die Messung, ob es hier `zwlr_layer_shell_v1` gibt. Sie
/// zählt **nur** für `eigen`: Wer ein Fenster verlangt, das wir nicht
/// zeichnen können, bekommt den Schalter und eine Zeile im Protokoll — ein
/// Klick, der nichts tut, ist schlimmer als der falsche Klick.
fn entscheiden(wert: &str, layer_shell: impl FnOnce() -> bool) -> Klick {
    match wert.trim() {
        "eigen" => {
            if layer_shell() {
                Klick::EigenesFenster
            } else {
                eprintln!(
                    "{SCHALTER}=eigen, aber hier gibt es kein wlr-layer-shell — \
                     der Linksklick schaltet stattdessen die Leitung."
                );
                Klick::Schalter
            }
        }
        "leiste" => Klick::MenueDerLeiste,
        _ => Klick::Schalter,
    }
}

fn klick_art() -> Klick {
    entscheiden(
        &std::env::var(SCHALTER).unwrap_or_default(),
        popup::moeglich,
    )
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
        klick: Klick::Schalter,
        fenster: popup::Fenster::keins(),
    }
    .spawn()
    .map_err(|e| format!("Keine Leiste mit StatusNotifier: {e}"))?;

    handle.shutdown().wait();

    Ok(())
}

pub fn starten() -> Result<(), String> {
    match klick_art() {
        // Nur hier darf die Leiste das Menü am Linksklick öffnen, und nur
        // hier ist `ItemIsMenu` wahr.
        Klick::MenueDerLeiste => laufen::<true>(Klick::MenueDerLeiste),
        andere => laufen::<false>(andere),
    }
}

fn laufen<const LEISTE: bool>(klick: Klick) -> Result<(), String> {
    let (signal, geweckt) = mpsc::channel();

    let handle = Tray::<LEISTE> {
        kern: Kern::neu(signal),
        klick,
        fenster: popup::Fenster::keins(),
    }
    .spawn()
    .map_err(|e| {
        format!("Kein Tray verfügbar ({e}). Läuft eine Leiste mit StatusNotifier, z. B. waybar?")
    })?;

    loop {
        let wissen = crate::harness::abfragen();
        let fehler = crate::kern::fehler_abholen();

        handle.update(|t: &mut Tray<LEISTE>| {
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
                handle.update(|t: &mut Tray<LEISTE>| t.kern.tun(&aktion));
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

    /// Die Zusage aus #1124/#9006/#9008/#9060, in zwei Zeilen festgenagelt.
    ///
    /// Diese Konstante ist die ganze Entscheidung darüber, ob wir den
    /// Linksklick überhaupt **sehen**, und sie hat zwischen v0.5.0, v0.5.1,
    /// v0.5.3 und v0.5.4 viermal den Wert gewechselt — jedes Mal auf eine
    /// Aussage von Bastian hin, nie beim Aufräumen nebenbei. Deshalb steht
    /// hier eine Probe und kein Kommentar.
    ///
    /// Geprüft wird in einem `const`-Block, nicht zur Laufzeit: Der Wert ist
    /// zur Übersetzungszeit bekannt, also soll der Übersetzer schimpfen und
    /// nicht erst der Testlauf.
    #[test]
    fn nur_die_leisten_fassung_laesst_den_klick_liegen() {
        const {
            assert!(
                !<Tray<false> as ksni::Tray>::MENU_ON_ACTIVATE,
                "Ohne ItemIsMenu=false kommt kein Activate an — dann schaltet der Linksklick nicht"
            )
        };

        const {
            assert!(
                <Tray<true> as ksni::Tray>::MENU_ON_ACTIVATE,
                "In der Leisten-Fassung muss die Leiste das Menü öffnen — sonst tut der Klick nichts"
            )
        };
    }

    /// Die Vorgabe ist der Schalter — und sie fragt nicht nach layer-shell.
    ///
    /// Das ist der Unterschied zu v0.5.4 bis v0.6.0: Dort entschied die
    /// Messung, und auf einem Hyprland-Rechner kam dabei das eigene Fenster
    /// heraus. Jetzt braucht die Vorgabe nichts, was gemessen werden müsste.
    #[test]
    fn ohne_schalter_schaltet_der_linksklick() {
        for wert in ["", "ja", "1", "wayland", "unsinn", "schalter"] {
            assert_eq!(
                entscheiden(wert, || panic!("gemessen, obwohl nicht nötig")),
                Klick::Schalter,
                "{wert}"
            );
        }
    }

    /// Der Schalter von Hand schlägt die Vorgabe, in beide Richtungen.
    #[test]
    fn der_schalter_von_hand_waehlt_die_fassung() {
        assert_eq!(entscheiden("eigen", || true), Klick::EigenesFenster);
        assert_eq!(entscheiden("leiste", || true), Klick::MenueDerLeiste);
        assert_eq!(
            entscheiden(" eigen ", || true),
            Klick::EigenesFenster,
            "Leerzeichen zählen nicht"
        );
    }

    /// `eigen` ohne layer-shell gibt den Schalter, nicht einen toten Klick.
    ///
    /// Ein Fenster, das wir nicht zeichnen können, wäre ein Klick, der nichts
    /// tut — genau die Beschwerde, mit der #1124 angefangen hat.
    #[test]
    fn eigen_ohne_layer_shell_faellt_auf_den_schalter_zurueck() {
        assert_eq!(entscheiden("eigen", || false), Klick::Schalter);

        // `leiste` braucht kein layer-shell und fragt auch nicht danach.
        assert_eq!(
            entscheiden("leiste", || panic!("gemessen, obwohl nicht nötig")),
            Klick::MenueDerLeiste
        );
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
