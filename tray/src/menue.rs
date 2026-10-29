//! Das Menü als Liste von Einträgen — einmal beschrieben, dreimal gezeichnet.
//!
//! ## Warum das Modell hier liegt (Stand 28.10.2026, #9008)
//!
//! Bis v0.5.3 baute jede Leiste ihr Menü selbst: `linux.rs` als
//! `ksni::MenuItem`, `fremd.rs` als `muda`-Punkte. Zwei Listen über dieselbe
//! Sache, Zeile für Zeile gleich — und sie **sind** auseinandergelaufen, als
//! „Leitung aufräumen" dazukam.
//!
//! Mit dem eigenen Menüfenster unter Wayland wären es **drei** geworden, und
//! zwei davon auf demselben Rechner: Linksklick zeigt unser Fenster,
//! Rechtsklick weiter das Menü der Leiste. Zwei Menüs, die verschiedene Dinge
//! anbieten, sind schlimmer als ein unbequemes. Deshalb beschreibt dieses
//! Modul das Menü einmal, und die drei Anbindungen zeichnen nur noch.
//!
//! **Nachtrag 29.10.2026 (v0.6.1, #9060):** Zwei Menüs auf demselben Rechner
//! waren auch *gleich* bestückt zu viel — Bastian: *„Das macht ja keinen
//! Sinn."* In der Vorgabe gibt es jetzt wieder eines, am Rechtsklick, und der
//! Linksklick schaltet. Dieses Modul ändert sich dadurch nicht: Unser Fenster
//! gibt es weiter (`AI_BRAIN_TRAY_MENUE=eigen`), und zwei Zeichner über eine
//! Liste bleiben richtig, auch wenn nur einer gleichzeitig zu sehen ist.
//!
//! Siehe auch das Wiki: *Zwei Tabellen über dieselbe Sache laufen auseinander
//! — und niemand sieht es.*
//!
//! ## Warum serde drauf liegt
//!
//! Das eigene Menüfenster ist ein **eigener Prozess** (`ai-brain-tray
//! --menue`). Der Tray schickt ihm die Einträge als JSON auf `stdin` und liest
//! die gewählte Aktion als JSON von `stdout`. Beides ist dieselbe Liste wie im
//! Hauptprozess — kein zweites Format, keine Zuordnung, die driften kann.

use serde::{Deserialize, Serialize};

use crate::kern::{Kern, DAUERN};

/// Was ein Menüpunkt auslöst.
///
/// Die Aktion steht am Eintrag, nicht in einer Tabelle daneben: Eine
/// Zuordnung „Kennung → Aktion" muss beim Neubauen des Menüs mit erneuert
/// werden, und eine alte Kennung zeigt sonst auf einen Punkt, den es nicht
/// mehr gibt. Genau das hat `fremd.rs` bis v0.5.3 von Hand gemacht.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Aktion {
    /// Das Häkchen „Live-Leitung": an, wenn aus — aus, wenn an.
    Umschalten,
    /// „… ab jetzt 30 min" und Geschwister. Die Dauer, wie der Harness sie
    /// versteht (`30m`, `2h`, `8h`).
    An(String),
    /// Der Störfall: Schalter aus, Prozess läuft.
    Aus,
    Brain,
    Protokoll,
    Beenden,
}

/// Eine Zeile im Menü.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Eintrag {
    /// Nur Text, nie anklickbar — Verbindung, Leitung, Lage, Fehler.
    Zeile(String),
    Trenner,
    Haken {
        text: String,
        gesetzt: bool,
        aktiv: bool,
        aktion: Aktion,
    },
    Punkt {
        text: String,
        aktiv: bool,
        aktion: Aktion,
    },
}

impl Eintrag {
    /// Die Aktion hinter dem Eintrag — `None` für Text und Trenner.
    ///
    /// Ein Punkt, der ausgegraut ist, gibt **nichts** zurück: Sonst müsste
    /// jede der drei Anbindungen sich selbst daran erinnern, `aktiv` zu
    /// prüfen, und die eine, die es vergisst, schaltet aus Unwissen.
    pub fn aktion(&self) -> Option<&Aktion> {
        match self {
            Eintrag::Haken { aktion, aktiv, .. } | Eintrag::Punkt { aktion, aktiv, .. }
                if *aktiv =>
            {
                Some(aktion)
            }
            _ => None,
        }
    }

    /// Die Beschriftung — für das Zeichnen und für die Breite des Fensters.
    pub fn text(&self) -> Option<&str> {
        match self {
            Eintrag::Zeile(t) => Some(t),
            Eintrag::Haken { text, .. } | Eintrag::Punkt { text, .. } => Some(text),
            Eintrag::Trenner => None,
        }
    }

    /// Kann man darauf klicken?
    pub fn klickbar(&self) -> bool {
        self.aktion().is_some()
    }
}

/// Das Menü, wie es jetzt aussehen soll.
///
/// Die Reihenfolge und die Bedingungen sind die aus v0.5.3, unverändert — nur
/// stehen sie jetzt einmal da statt zweimal:
///
/// 1. Verbindung, Leitung, Lage, Fehler als Text
/// 2. Häkchen „Live-Leitung", dann die Dauern
/// 3. „Leitung aufräumen" **nur im Störfall** (Schalter aus, Prozess läuft)
/// 4. AI Brain öffnen, Protokoll öffnen, Tray beenden
pub fn bauen(kern: &Kern) -> Vec<Eintrag> {
    let a = kern.ansicht();
    let mut eintraege = Vec::new();

    // Zwei Zeilen, nicht eine: Verbindung und Leitung sind zwei Zustände.
    // Die dritte kommt dazu, wenn es etwas zu sagen gibt.
    for text in [Some(a.verbindung), Some(a.leitung), a.lage]
        .into_iter()
        .flatten()
    {
        eintraege.push(Eintrag::Zeile(text));
    }

    if let Some(fehler) = &kern.letzter_fehler {
        eintraege.push(Eintrag::Zeile(format!("Fehler: {fehler}")));
    }

    eintraege.push(Eintrag::Trenner);

    // ## Ein Häkchen, kein Knopf (Stand 27.10.2026)
    //
    // Ein Knopf „Live einschalten" sagt nicht, ob sie gerade an ist — man
    // muss die Zeilen oben lesen, um es zu wissen. Ein Häkchen trägt den
    // Zustand und die Handlung in einem, und es ist das, was ein Menü in
    // jedem anderen Programm dafür anbietet.
    //
    // Ohne Antwort nicht schalten: Ein Klick, der mangels Wissen das
    // Gegenteil des Gewollten tut, wäre der nächste Ärger.
    eintraege.push(Eintrag::Haken {
        text: "Live-Leitung".into(),
        gesetzt: kern.an(),
        aktiv: kern.weiss_etwas(),
        aktion: Aktion::Umschalten,
    });

    for (beschriftung, dauer) in DAUERN {
        eintraege.push(Eintrag::Punkt {
            text: format!("… ab jetzt {beschriftung}"),
            aktiv: true,
            aktion: Aktion::An(dauer.to_string()),
        });
    }

    // ## „Aufräumen" erscheint nur, wenn es etwas aufzuräumen gibt
    //
    // Bis zum 27.10.2026 stand hier ein dauerhaftes „Live ausschalten" mit
    // `enabled: an`. Das war im Störfall falsch — Schalter aus, Prozess läuft
    // weiter, und genau der Punkt, der aufräumt, war ausgegraut.
    //
    // Jetzt trägt das Häkchen den Normalfall, und dieser Punkt bleibt für den
    // Fall, den das Häkchen NICHT abdeckt: Steht der Schalter auf aus,
    // während ein Prozess weiterläuft, ist das Häkchen leer — ein Klick
    // darauf würde einschalten, nicht aufräumen.
    if kern.aufraeumen_noetig() {
        eintraege.push(Eintrag::Punkt {
            text: "Leitung aufräumen (Prozess läuft ohne Schalter)".into(),
            aktiv: true,
            aktion: Aktion::Aus,
        });
    }

    eintraege.push(Eintrag::Trenner);

    // Ohne bekannte Adresse ist der Punkt ausgegraut und sagt damit, woran es
    // liegt; eine erfundene URL wäre schlimmer als ein Klick, der nichts tut.
    eintraege.push(Eintrag::Punkt {
        text: "AI Brain öffnen".into(),
        aktiv: kern.instanz_adresse().is_some(),
        aktion: Aktion::Brain,
    });

    eintraege.push(Eintrag::Punkt {
        text: "Protokoll öffnen".into(),
        aktiv: kern.protokoll().is_some(),
        aktion: Aktion::Protokoll,
    });

    eintraege.push(Eintrag::Punkt {
        text: "Tray beenden".into(),
        aktiv: true,
        aktion: Aktion::Beenden,
    });

    eintraege
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansicht::{Status, Wissen};
    use std::sync::mpsc;

    /// Der Empfänger kommt mit zurück und muss am Leben bleiben: Ein
    /// `Kern`, dessen Gegenstück abgeräumt ist, scheitert bei jedem `send`.
    /// Die Proben hier schalten nichts, aber eine Probe, die am Aufräumen
    /// scheitert, sagt etwas über sich und nicht über den Code.
    fn kern(wissen: Wissen) -> (Kern, mpsc::Receiver<crate::kern::Signal>) {
        let (tx, rx) = mpsc::channel();
        let mut k = Kern::neu(tx);
        k.wissen = wissen;
        (k, rx)
    }

    /// Ohne Antwort ist das Häkchen ausgegraut — und damit ohne Aktion.
    ///
    /// Das ist der Punkt, an dem das geteilte Modell etwas gutmacht: Die
    /// Prüfung „nicht schalten, wenn wir nichts wissen" lag bis v0.5.3 in
    /// jeder Anbindung einzeln (`enabled: self.kern.weiss_etwas()`).
    #[test]
    fn ohne_antwort_ist_das_haekchen_ohne_aktion() {
        let (k, _rx) = kern(Wissen::Unbekannt);
        let eintraege = bauen(&k);

        let haken = eintraege
            .iter()
            .find(|e| matches!(e, Eintrag::Haken { .. }))
            .expect("Häkchen fehlt");

        assert_eq!(haken.aktion(), None);
        assert!(!haken.klickbar());
    }

    #[test]
    fn mit_antwort_schaltet_das_haekchen() {
        let (k, _rx) = kern(Wissen::Da(Status {
            an: true,
            ..Status::default()
        }));
        let eintraege = bauen(&k);

        let haken = eintraege
            .iter()
            .find(|e| matches!(e, Eintrag::Haken { .. }))
            .expect("Häkchen fehlt");

        assert_eq!(haken.aktion(), Some(&Aktion::Umschalten));
        assert!(matches!(haken, Eintrag::Haken { gesetzt: true, .. }));
    }

    /// „Aufräumen" ist der Störfall und darf sonst nicht herumstehen.
    #[test]
    fn aufraeumen_nur_im_stoerfall() {
        let (k, _rx) = kern(Wissen::Da(Status::default()));
        let ohne = bauen(&k);
        assert!(!ohne.iter().any(|e| e.aktion() == Some(&Aktion::Aus)));

        let (k, _rx2) = kern(Wissen::Da(Status {
            an: false,
            prozess: Some(4711),
            ..Status::default()
        }));
        let mit = bauen(&k);
        assert!(mit.iter().any(|e| e.aktion() == Some(&Aktion::Aus)));
    }

    /// Die Dauern aus `DAUERN` kommen vollständig und in derselben Reihenfolge.
    #[test]
    fn jede_dauer_hat_ihren_punkt() {
        let (k, _rx) = kern(Wissen::Da(Status::default()));
        let eintraege = bauen(&k);

        let dauern: Vec<&str> = eintraege
            .iter()
            .filter_map(|e| match e.aktion() {
                Some(Aktion::An(d)) => Some(d.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(dauern, DAUERN.map(|(_, d)| d).to_vec());
    }

    /// Ohne Adresse bleibt „AI Brain öffnen" ausgegraut — kein erfundenes Ziel.
    #[test]
    fn ohne_adresse_kein_brain_klick() {
        let (k, _rx) = kern(Wissen::Da(Status::default()));
        let eintraege = bauen(&k);

        let punkt = eintraege
            .iter()
            .find(|e| e.text() == Some("AI Brain öffnen"))
            .expect("Punkt fehlt");

        assert_eq!(punkt.aktion(), None);
    }

    /// Dasselbe Modell, dieselben Punkte — über JSON hin und zurück.
    ///
    /// Das Menüfenster ist ein eigener Prozess; diese Probe hält fest, dass
    /// der Weg dorthin nichts verliert.
    #[test]
    fn das_modell_uebersteht_den_weg_durch_json() {
        let (k, _rx) = kern(Wissen::Da(Status::default()));
        let eintraege = bauen(&k);

        let text = serde_json::to_string(&eintraege).expect("schreiben");
        let zurueck: Vec<Eintrag> = serde_json::from_str(&text).expect("lesen");

        assert_eq!(eintraege, zurueck);
    }
}
