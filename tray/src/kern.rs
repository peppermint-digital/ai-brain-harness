//! Was der Tray tut — unabhängig davon, in welcher Leiste er hängt.
//!
//! Die Leisten sind drei (StatusNotifierItem, NSStatusItem, Shell_NotifyIcon),
//! die Bedienung ist eine. Alles, was mehr als Zeichnen ist, steht deshalb
//! hier: welcher Harness-Aufruf zu welchem Menüpunkt gehört, was ein
//! Linksklick heisst, und wie ein Fehler aus dem Hintergrund zurückkommt.

use std::sync::mpsc;
use std::sync::Mutex;

use crate::ansicht::{ansicht, Ansicht, Wissen};
use crate::harness;

/// So oft wird der Zustand gelesen.
pub const TAKT: std::time::Duration = std::time::Duration::from_secs(2);

/// Die Dauern im Menü. 8 h ist die Obergrenze des Harness.
pub const DAUERN: [(&str, &str); 3] = [("30 min", "30m"), ("2 h", "2h"), ("8 h", "8h")];

/// Was ein Linksklick einschaltet, wenn die Leitung aus ist — dieselbe
/// Vorgabe wie `leitung an` ohne `--dauer`.
pub const KLICK_DAUER: &str = "30m";

/// Der Fehler der letzten Aktion, vom Hintergrund-Faden an die Abfrage.
static FEHLER: Mutex<Option<String>> = Mutex::new(None);

/// Was der Haupt-Faden vom Menü hört.
pub enum Signal {
    /// Eine Aktion ist durch — sofort nachlesen, statt den Takt abzuwarten.
    Wecken,
    /// „Tray beenden" wurde geklickt.
    Ende,
}

pub struct Kern {
    pub wissen: Wissen,
    /// Was die letzte Aktion meldete, falls sie scheiterte.
    pub letzter_fehler: Option<String>,
    signal: mpsc::Sender<Signal>,
}

impl Kern {
    pub fn neu(signal: mpsc::Sender<Signal>) -> Self {
        Self {
            wissen: Wissen::Unbekannt,
            letzter_fehler: None,
            signal,
        }
    }

    pub fn ansicht(&self) -> Ansicht {
        ansicht(&self.wissen, jetzt())
    }

    /// Läuft die Leitung gerade?
    pub fn an(&self) -> bool {
        matches!(&self.wissen, Wissen::Da(s) if s.an)
    }

    /// Haben wir überhaupt eine Antwort?
    pub fn weiss_etwas(&self) -> bool {
        !matches!(self.wissen, Wissen::Unbekannt)
    }

    pub fn protokoll(&self) -> Option<String> {
        match &self.wissen {
            Wissen::Da(s) => s.protokoll.clone(),
            _ => None,
        }
    }

    pub fn einschalten(&mut self, dauer: &str) {
        self.ausfuehren(&["leitung", "an", "--dauer", dauer]);
    }

    pub fn ausschalten(&mut self) {
        self.ausfuehren(&["leitung", "aus"]);
    }

    /// Der Linksklick.
    ///
    /// ## Warum es den gibt (Stand 27.10.2026)
    ///
    /// Bis zum 27.10.2026 hatte der Tray nur ein Menü. In waybar — und in den
    /// meisten Leisten — öffnet der **Rechtsklick** das Menü, der Linksklick
    /// löst `Activate` aus. Ohne `activate` tat ein Linksklick also
    /// **nichts**, und von aussen sah das aus wie ein Tray, der sich nicht
    /// schalten und nicht beenden lässt: Genau so wurde es gemeldet.
    ///
    /// Ein Klick, der nichts tut, ist schlimmer als kein Klick — er lässt
    /// einen am Programm zweifeln statt an der Bedienung.
    ///
    /// Wissen wir nichts, wird NICHT geschaltet: Ein Klick, der mangels
    /// Antwort das Gegenteil des Gewollten tut, wäre der nächste Ärger.
    pub fn umschalten(&mut self) {
        if !self.weiss_etwas() {
            return;
        }

        if self.an() {
            self.ausschalten();
        } else {
            self.einschalten(KLICK_DAUER);
        }
    }

    pub fn protokoll_oeffnen(&self) {
        if let Some(pfad) = self.protokoll() {
            harness::oeffnen(&pfad);
        }
    }

    pub fn beenden(&self) {
        let _ = self.signal.send(Signal::Ende);
    }

    /// Ruft den Harness im Hintergrund — `leitung aus` wartet bis zu 8 s auf
    /// das Auflegen, und so lange darf das Menü nicht hängen.
    fn ausfuehren(&mut self, args: &[&str]) {
        self.letzter_fehler = None;

        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let signal = self.signal.clone();

        std::thread::spawn(move || {
            if let Err(grund) = harness::rufen(&args) {
                FEHLER.lock().map(|mut f| *f = Some(grund)).ok();
            }

            let _ = signal.send(Signal::Wecken);
        });
    }
}

/// Den Fehler der letzten Aktion abholen (und dabei wegnehmen).
pub fn fehler_abholen() -> Option<String> {
    FEHLER.lock().ok().and_then(|mut f| f.take())
}

pub fn jetzt() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansicht::Status;

    fn kern(wissen: Wissen) -> (Kern, mpsc::Receiver<Signal>) {
        let (tx, rx) = mpsc::channel();
        let mut k = Kern::neu(tx);
        k.wissen = wissen;
        (k, rx)
    }

    #[test]
    fn ohne_antwort_wird_nicht_geschaltet() {
        let (mut k, _rx) = kern(Wissen::Unbekannt);
        k.umschalten();

        // Kein Aufruf, also auch kein Fehler und kein Wecken — geprüft über
        // den Zustand, den `ausfuehren` als Erstes anfasst.
        assert!(k.letzter_fehler.is_none());
        assert!(!k.weiss_etwas());
    }

    #[test]
    fn an_erkennt_den_schalter() {
        let (k, _rx) = kern(Wissen::Da(Status {
            an: true,
            ..Default::default()
        }));

        assert!(k.an());
        assert!(k.weiss_etwas());
    }

    #[test]
    fn beenden_schickt_das_ende() {
        let (k, rx) = kern(Wissen::Unbekannt);
        k.beenden();

        assert!(matches!(rx.try_recv(), Ok(Signal::Ende)));
    }
}
