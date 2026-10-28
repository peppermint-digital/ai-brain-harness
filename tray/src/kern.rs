//! Was der Tray tut — unabhängig davon, in welcher Leiste er hängt.
//!
//! Die Leisten sind drei (StatusNotifierItem, NSStatusItem, Shell_NotifyIcon),
//! die Bedienung ist eine. Alles, was mehr als Zeichnen ist, steht deshalb
//! hier: welcher Harness-Aufruf zu welchem Menüpunkt gehört und wie ein Fehler
//! aus dem Hintergrund zurückkommt.
//!
//! Der Klick auf das Symbol selbst steht **nicht** hier, weil er nichts tut,
//! was eine Regel bräuchte: Er öffnet das Menü und schliesst es wieder. Wer
//! es zeigt — wir oder die Leiste —, entscheidet `linux.rs`.

use std::sync::mpsc;
use std::sync::Mutex;

use crate::ansicht::{ansicht, Ansicht, Wissen};
use crate::harness;

/// So oft wird der Zustand gelesen.
pub const TAKT: std::time::Duration = std::time::Duration::from_secs(2);

/// Die Dauern im Menü. 8 h ist die Obergrenze des Harness.
pub const DAUERN: [(&str, &str); 3] = [("30 min", "30m"), ("2 h", "2h"), ("8 h", "8h")];

/// Was das Häkchen einschaltet, wenn die Leitung aus ist — dieselbe Vorgabe
/// wie `leitung an` ohne `--dauer`.
///
/// Der Name stammt aus der Zeit, in der der Linksklick geschaltet hat (v0.4.2).
/// Er bleibt, weil er in der Vorgeschichte und in #8932 so zitiert ist.
pub const KLICK_DAUER: &str = "30m";

/// Der Fehler der letzten Aktion, vom Hintergrund-Faden an die Abfrage.
static FEHLER: Mutex<Option<String>> = Mutex::new(None);

/// Was der Haupt-Faden vom Menü hört.
pub enum Signal {
    /// Eine Aktion ist durch — sofort nachlesen, statt den Takt abzuwarten.
    Wecken,
    /// „Tray beenden" wurde geklickt.
    Ende,
    /// Im eigenen Menüfenster wurde ein Punkt gewählt (#9008).
    ///
    /// Das Fenster ist ein eigener Prozess; er schreibt die Aktion auf
    /// `stdout` und endet. Ein Faden im Tray wartet darauf und schickt sie
    /// hierher — **nicht** in den D-Bus-Faden, denn dort hält niemand mehr
    /// den `Kern`, wenn der Klick kommt.
    Gewaehlt(crate::menue::Aktion),
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

    /// Der Störfall: Der Schalter steht auf aus, ein Prozess läuft trotzdem.
    ///
    /// Das Häkchen deckt diesen Fall NICHT ab — es wäre leer, und ein Klick
    /// darauf würde einschalten statt aufzuräumen. `leitung aus` ist dafür
    /// das Werkzeug: Der Harness verträgt den Aufruf im Leerlauf, lässt die
    /// Leitung selbst auflegen und beendet den Prozess erst, wenn er nach
    /// acht Sekunden nicht reagiert.
    pub fn aufraeumen_noetig(&self) -> bool {
        matches!(&self.wissen, Wissen::Da(s) if !s.an && s.prozess.is_some())
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

    /// Das Häkchen „Live-Leitung" im Menü.
    ///
    /// ## Wo das herkommt, und wohin es gewandert ist (Stand 27.10.2026)
    ///
    /// Zuerst hatte der Tray nur ein Menü und keine Behandlung von
    /// `Activate`. In waybar — und in den meisten Leisten — öffnet der
    /// Rechtsklick das Menü, der Linksklick löst `Activate` aus. Ohne
    /// Behandlung tat ein Linksklick also **nichts**, und von aussen sah das
    /// aus wie ein Tray, der sich nicht schalten und nicht beenden lässt.
    /// Gemeldet wurde genau das.
    ///
    /// Darauf schaltete der Linksklick die Leitung. Das löste den toten Klick
    /// und schuf den nächsten Punkt: Das Menü liess sich nicht mehr
    /// schliessen, weil wir es selbst öffneten statt es der Leiste zu
    /// überlassen — und ein Symbolklick, der etwas schaltet, ist in keiner
    /// anderen Leiste so.
    ///
    /// Seit v0.5.0 hängt es als **Häkchen** im Menü: Haken rein heisst
    /// `leitung an` und hebt ein Veto auf, Haken raus heisst `leitung aus`
    /// und setzt es. Das bleibt so — nur der Weg INS Menü hat mehrfach
    /// gewechselt: v0.5.1 auf den Rechtsklick, v0.5.3 zurück auf beide
    /// Klicks, und seit v0.5.4 zeigt der Linksklick unter Wayland unser
    /// **eigenes** Menüfenster, damit ein zweiter Klick es wieder schliesst.
    /// Die Begründung steht in `linux.rs` und in Bug #1124.
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

    /// Die Adresse der Instanz, mit der dieser Rechner verbunden ist.
    ///
    /// `None` heisst: nicht verbunden — oder ein Harness, der das Feld noch
    /// nicht mitschickt (vor #8932). Beides ist derselbe Fall fuer uns: Wir
    /// wissen die Adresse nicht und erfinden keine.
    pub fn instanz_adresse(&self) -> Option<String> {
        match &self.wissen {
            Wissen::Da(s) => s.verbindung.clone().flatten().and_then(|v| v.basis),
            _ => None,
        }
    }

    /// Der Menuepunkt „AI Brain oeffnen" (#1124).
    ///
    /// ## Vom Linksklick zum Menuepunkt
    ///
    /// Diesen Weg gibt es seit v0.5.1, und dort hing er am **Linksklick**:
    /// Rechtsklick oeffnete das Menue, Linksklick die Anwendung — wie bei den
    /// anderen Symbolen in Bastians Leiste. Seit v0.5.3 oeffnet der Linksklick
    /// wieder das Menue (Bastian am 28.10.2026: „der sollte doch einfach nur
    /// das Tray-Menue aufmachen"), und damit ist hier nur noch der sichtbare
    /// Punkt im Menue uebrig.
    ///
    /// Die fuenf Fassungen des Symbolklicks und ihre Gruende stehen an einer
    /// Stelle: an `MENU_ON_ACTIVATE` in `linux.rs`.
    ///
    /// Ohne bekannte Adresse passiert nichts, und der Punkt ist ausgegraut —
    /// eine erfundene URL waere schlimmer als ein Klick, der nichts tut.
    pub fn brain_oeffnen(&self) {
        if let Some(adresse) = self.instanz_adresse() {
            harness::oeffnen(&adresse);
        }
    }

    pub fn beenden(&self) {
        let _ = self.signal.send(Signal::Ende);
    }

    /// Eine Aktion aus dem Menü ausführen — egal, welches Menü sie geschickt
    /// hat.
    ///
    /// Es gibt auf einem Linux-Rechner jetzt **zwei** Menüs für dieselben
    /// Punkte: unser Fenster am Linksklick und das der Leiste am Rechtsklick
    /// (#9008). Eine zweite Stelle, die entscheidet, was „AI Brain öffnen"
    /// bedeutet, wäre die Stelle, an der die beiden auseinanderlaufen.
    pub fn tun(&mut self, aktion: &crate::menue::Aktion) {
        use crate::menue::Aktion;

        match aktion {
            Aktion::Umschalten => self.umschalten(),
            Aktion::An(dauer) => self.einschalten(dauer),
            Aktion::Aus => self.ausschalten(),
            Aktion::Brain => self.brain_oeffnen(),
            Aktion::Protokoll => self.protokoll_oeffnen(),
            Aktion::Beenden => self.beenden(),
        }
    }

    /// Ein zweites Ende des Signalwegs — für den Faden, der auf das
    /// Menüfenster wartet.
    pub fn signalweg(&self) -> mpsc::Sender<Signal> {
        self.signal.clone()
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

/// Einen Fehler von aussen in dieselbe Ablage legen.
///
/// Gebraucht vom Linksklick, wenn das eigene Menüfenster nicht aufgeht
/// (#9008): Der Klick läuft im D-Bus-Faden, wo niemand `&mut Kern` hält, und
/// ein Klick, der stillschweigend nichts tut, ist genau die Beschwerde, mit
/// der Bug #1124 angefangen hat. So steht der Grund beim nächsten Takt im
/// Menü — in dem der Leiste, das in diesem Fall noch geht.
pub fn fehler_melden(grund: String) {
    FEHLER.lock().map(|mut f| *f = Some(grund)).ok();
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

    /// #1124: Woher „AI Brain oeffnen" seine Adresse nimmt — und wann nicht.
    ///
    /// Die drei Schichten aus #8914 gelten hier genauso: „weiss ich nicht"
    /// ist nicht „nicht verbunden", und beides ist nicht „Adresse da". Ein
    /// Punkt, der aus Unwissen irgendeine URL oeffnet, waere schlimmer als
    /// einer, der ausgegraut ist.
    #[test]
    fn die_adresse_fuer_ai_brain_wird_nicht_erfunden() {
        use crate::ansicht::Verbindung;

        // Harness zu alt: Feld fehlt ganz.
        let (k, _rx) = kern(Wissen::Da(Status::default()));
        assert_eq!(k.instanz_adresse(), None);

        // Harness kennt das Feld und sagt: kein Zugang hinterlegt.
        let (k, _rx) = kern(Wissen::Da(Status {
            verbindung: Some(None),
            ..Status::default()
        }));
        assert_eq!(k.instanz_adresse(), None);

        // Verbunden, aber ohne `basis` (Harness vor #8932).
        let (k, _rx) = kern(Wissen::Da(Status {
            verbindung: Some(Some(Verbindung {
                instanz: "AI Brain".into(),
                geraet: "laptop".into(),
                basis: None,
            })),
            ..Status::default()
        }));
        assert_eq!(k.instanz_adresse(), None);

        // Und der Normalfall.
        let (k, _rx) = kern(Wissen::Da(Status {
            verbindung: Some(Some(Verbindung {
                instanz: "AI Brain".into(),
                geraet: "laptop".into(),
                basis: Some("https://brain.example.com".into()),
            })),
            ..Status::default()
        }));
        assert_eq!(
            k.instanz_adresse().as_deref(),
            Some("https://brain.example.com")
        );

        // Ohne Antwort gar nichts — der Klick bleibt still.
        let (k, _rx) = kern(Wissen::Unbekannt);
        assert_eq!(k.instanz_adresse(), None);
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
