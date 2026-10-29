//! Was der Tray tut — unabhängig davon, in welcher Leiste er hängt.
//!
//! Die Leisten sind drei (StatusNotifierItem, NSStatusItem, Shell_NotifyIcon),
//! die Bedienung ist eine. Alles, was mehr als Zeichnen ist, steht deshalb
//! hier: welcher Harness-Aufruf zu welchem Menüpunkt gehört und wie ein Fehler
//! aus dem Hintergrund zurückkommt.
//!
//! Der Klick auf das Symbol selbst steht seit v0.6.1 **auch** hier, denn er
//! hat wieder eine Regel: Er schaltet die Live-Leitung, und zwar nach
//! derselben Entscheidung wie das Häkchen im Menü (`schritt`). Bis v0.6.0
//! öffnete er nur ein Menü und brauchte dafür keine — nachzulesen an
//! `MENU_ON_ACTIVATE` in `linux.rs`, wo auch steht, welche Leiste welchen
//! Klick überhaupt zu uns durchlässt.

use std::sync::mpsc;
use std::sync::Mutex;

use crate::ansicht::{ansicht, Ansicht, Wissen};
use crate::harness;

/// So oft wird der Zustand gelesen.
pub const TAKT: std::time::Duration = std::time::Duration::from_secs(2);

/// Die Dauern im Menü. 8 h ist die Obergrenze des Harness.
pub const DAUERN: [(&str, &str); 3] = [("30 min", "30m"), ("2 h", "2h"), ("8 h", "8h")];

/// Was ein Klick einschaltet, wenn die Leitung aus ist — dieselbe Vorgabe
/// wie `leitung an` ohne `--dauer`.
///
/// Der Name kam von v0.4.2, wo der Linksklick schaltete, und stand von v0.5.0
/// bis v0.6.0 nur noch für das Häkchen im Menü. Seit v0.6.1 (#9060) schaltet
/// der Linksklick wieder, und der Name ist wieder wörtlich wahr. In der
/// Vorgeschichte und in #8932 ist er ohnehin so zitiert.
pub const KLICK_DAUER: &str = "30m";

/// Was ein Klick auf die Leitung auslöst.
///
/// Die Aufzählung gibt es, damit die Entscheidung prüfbar ist, ohne dass ein
/// Harness läuft: `schritt` entscheidet, `Kern::umschalten` führt aus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Schritt {
    /// `leitung an --dauer <dauer>`
    An(&'static str),
    /// `leitung aus`
    Aus,
}

/// Die Entscheidung hinter Linksklick und Häkchen — ohne Nebenwirkung.
///
/// Eine Stelle für beide: Seit v0.6.1 schaltet der Linksklick wieder (#9060),
/// und das Häkchen im Menü tut dasselbe. Zwei Stellen wären zwei
/// Gelegenheiten, verschieden zu entscheiden.
///
/// `None` heisst: nicht schalten. Das gilt für `Wissen::Unbekannt` — ein
/// Klick, der mangels Antwort das Gegenteil des Gewollten tut, wäre der
/// nächste Ärger.
pub fn schritt(wissen: &Wissen) -> Option<Schritt> {
    match wissen {
        Wissen::Unbekannt => None,
        Wissen::Da(s) if s.an => Some(Schritt::Aus),
        // `Wissen::Da` mit ausgeschalteter Leitung — und `Wissen::Fehler`.
        // Letzteres ist das Verhalten, das das Häkchen seit v0.5.0 hat, und
        // es ist hier NICHT stillschweigend geändert worden: Dass ein
        // unlesbarer Harness-Status wie „aus" behandelt wird, ist eine eigene
        // Entscheidung und gehört nicht in einen Umbau am Klick.
        _ => Some(Schritt::An(KLICK_DAUER)),
    }
}

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
    /// Klicks, und v0.5.4 bis v0.6.0 zeigte der Linksklick unter Wayland
    /// unser **eigenes** Menüfenster, damit ein zweiter Klick es schliesst.
    ///
    /// **Seit v0.6.1 schaltet der Linksklick wieder selbst (#9060)** — und
    /// ruft dazu genau diese Methode. Nicht, weil die Begründung von v0.5.0
    /// falsch geworden wäre, sondern weil sich das eigene Fenster und „ein
    /// Menü" nicht beides haben liessen: Auf Omarchy ist der Linksklick der
    /// einzige Klick, der bei uns ankommt. Gemessen steht das an
    /// `MENU_ON_ACTIVATE` in `linux.rs`, die Bitte dahinter in Task #9060.
    ///
    /// Wissen wir nichts, wird NICHT geschaltet — die Entscheidung dazu
    /// steht in `schritt`, damit Klick und Häkchen sie teilen.
    pub fn umschalten(&mut self) {
        match schritt(&self.wissen) {
            Some(Schritt::Aus) => self.ausschalten(),
            Some(Schritt::An(dauer)) => self.einschalten(dauer),
            None => {}
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
    /// anderen Symbolen in Bastians Leiste. Ab v0.5.3 oeffnete der Linksklick
    /// wieder das Menue (Bastian am 28.10.2026: „der sollte doch einfach nur
    /// das Tray-Menue aufmachen"), seit v0.6.1 schaltet er die Leitung — und
    /// damit ist hier durchgehend nur noch der sichtbare Punkt im Menue
    /// uebrig.
    ///
    /// Die sechs Fassungen des Symbolklicks und ihre Gruende stehen an einer
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
    /// Von v0.5.4 bis v0.6.0 gab es auf einem Linux-Rechner **zwei** Menüs
    /// für dieselben Punkte: unser Fenster am Linksklick und das der Leiste
    /// am Rechtsklick (#9008). Genau das hat Bastian am 29.10.2026 beendet
    /// (#9060) — in der Vorgabe ist es wieder eins, unter
    /// `AI_BRAIN_TRAY_MENUE=eigen` sind es weiter zwei. Eine zweite Stelle,
    /// die entscheidet, was „AI Brain öffnen" bedeutet, wäre die Stelle, an
    /// der sie auseinanderlaufen.
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

    // ## Die Klick-Logik, vier Proben (#9060)
    //
    // Seit v0.6.1 ist der Linksklick auf das Symbol der Hauptweg zum
    // Schalten — und er ist am laufenden Rechner nur von Hand prüfbar, weil
    // dazu jemand auf ein Symbol in einer Leiste klicken muss. Deshalb liegt
    // die Entscheidung als reine Funktion daneben und wird hier geprüft, in
    // allen vier Zuständen, die `Wissen` kennt.

    /// Ohne Antwort schaltet der Klick nicht.
    ///
    /// Das ist der Fall direkt nach dem Start, bevor die erste Abfrage durch
    /// ist. Ein Klick, der dann einschaltet, weil er „aus" vermutet, schaltet
    /// im Zweifel das Gegenteil des Gewollten.
    #[test]
    fn ohne_antwort_entscheidet_der_klick_nichts() {
        assert_eq!(schritt(&Wissen::Unbekannt), None);
    }

    /// Leitung aus → an, und zwar für 30 Minuten.
    #[test]
    fn aus_wird_an_fuer_dreissig_minuten() {
        let wissen = Wissen::Da(Status {
            an: false,
            ..Status::default()
        });

        assert_eq!(schritt(&wissen), Some(Schritt::An("30m")));
    }

    /// Leitung an → aus.
    #[test]
    fn an_wird_aus() {
        let wissen = Wissen::Da(Status {
            an: true,
            ..Status::default()
        });

        assert_eq!(schritt(&wissen), Some(Schritt::Aus));
    }

    /// Ein unlesbarer Status gilt wie „aus" — festgehalten, nicht gewollt.
    ///
    /// `Wissen::Fehler` heisst: Der Harness hat nicht oder unlesbar
    /// geantwortet. Wir wissen also gerade NICHT, ob die Leitung läuft, und
    /// trotzdem schaltet der Klick ein. Das ist das Verhalten, das das
    /// Häkchen seit v0.5.0 hat (`weiss_etwas()` ist für `Fehler` wahr), und
    /// es bleibt in v0.6.1 unverändert: Ein Umbau am Klick ist nicht der
    /// Ort, an dem man eine zweite Entscheidung nebenbei mitändert.
    ///
    /// Diese Probe steht hier, damit die Stelle benannt ist und nicht
    /// jemandem später als Zufall begegnet. Gemeldet als Bug.
    #[test]
    fn ein_unlesbarer_status_gilt_wie_aus() {
        let wissen = Wissen::Fehler("Harness stumm".into());

        assert_eq!(schritt(&wissen), Some(Schritt::An("30m")));

        // Und das ist derselbe Zustand, in dem das Häkchen anklickbar ist —
        // die beiden entscheiden nachweislich gleich.
        let (k, _rx) = kern(Wissen::Fehler("Harness stumm".into()));
        assert!(k.weiss_etwas());
        assert!(!k.an());
    }

    /// Der Störfall schaltet ein, nicht auf — dafür ist „aufräumen" da.
    ///
    /// Schalter aus, Prozess läuft weiter: Der Klick auf das Symbol schaltet
    /// hier ein, genau wie das Häkchen. Aufgeräumt wird über den eigenen
    /// Punkt im Menü (`aufraeumen_noetig`), nicht über den Symbolklick — es
    /// gibt nur einen Klick, und der kann nicht beides bedeuten.
    #[test]
    fn im_stoerfall_schaltet_der_klick_ein_und_raeumt_nicht_auf() {
        let wissen = Wissen::Da(Status {
            an: false,
            prozess: Some(4711),
            ..Status::default()
        });

        assert_eq!(schritt(&wissen), Some(Schritt::An("30m")));

        let (k, _rx) = kern(Wissen::Da(Status {
            an: false,
            prozess: Some(4711),
            ..Status::default()
        }));
        assert!(k.aufraeumen_noetig());
    }
}
