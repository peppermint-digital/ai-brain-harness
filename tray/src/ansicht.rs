//! Was der Tray anzeigt — rein, ohne D-Bus, ohne Fenster, ohne Uhr.
//!
//! Hier steht die ganze Entscheidung „was sieht der Mensch". Sie ist deshalb
//! von der Leiste getrennt: Unter macOS und Windows ist die Leiste eine
//! andere, die Anzeige soll aber dieselbe sein — und ein Modul ohne
//! Plattformbezug ist das einzige, das sich auf jedem Rechner testen lässt.

use serde::{Deserialize, Deserializer};

/// Was `ai-brain-harness leitung status --json` liefert.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Status {
    pub an: bool,
    pub bis: Option<i64>,
    pub prozess: Option<u32>,
    pub lage: Option<Lage>,
    pub protokoll: Option<String>,
    /// Zwei Schichten mit Absicht (#8914):
    ///
    /// * `None` — das Feld fehlt, der Harness ist älter als diese Auskunft.
    /// * `Some(None)` — der Harness kennt es und sagt: kein Zugang hinterlegt.
    /// * `Some(Some(_))` — verbunden, mit Instanz und Gerätenamen.
    ///
    /// Ein einfaches `Option` könnte „weiss ich nicht" nicht von „nein"
    /// unterscheiden, und genau diese Verwechslung war die Beschwerde: Der
    /// Tray sah aus wie „nicht verbunden", während er nur nichts wusste.
    ///
    /// `deserialize_with` ist nötig, nicht Zierde: Ein blosses
    /// `Option<Option<_>>` macht serde aus einem ausdrücklichen `null`
    /// dasselbe wie aus einem fehlenden Feld — genau die Unterscheidung, um
    /// die es hier geht, fällt also weg. Mit dem Umweg über ein einfaches
    /// `Option`, das in ein `Some` gewickelt wird, bleibt sie erhalten.
    #[serde(default, deserialize_with = "doppel_option")]
    pub verbindung: Option<Option<Verbindung>>,
}

fn doppel_option<'de, D>(gibt: D) -> Result<Option<Option<Verbindung>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::deserialize(gibt).map(Some)
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Verbindung {
    pub instanz: String,
    pub geraet: String,
    #[serde(default)]
    pub basis: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Lage {
    pub stand: String,
    pub grund: Option<String>,
    pub pane: Option<String>,
}

/// Was der Tray gerade weiss.
#[derive(Debug)]
pub enum Wissen {
    /// Noch nicht gefragt.
    Unbekannt,
    Da(Status),
    /// Der Harness antwortete nicht oder unlesbar.
    Fehler(String),
}

/// Die Farbe des Symbols. Sie sagt etwas über die VERBINDUNG; dass die
/// Leitung läuft, sagt die Füllung.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Farbe {
    /// Kein Zugang hinterlegt, oder wir wissen es nicht.
    Grau,
    /// Verbunden, Leitung aus — der Ruhezustand, und kein Fehler.
    Blau,
    /// Leitung läuft und überträgt.
    Gruen,
    /// Unterwegs: verbindet, oder noch keine Antwort.
    Gelb,
    /// Etwas stimmt nicht.
    Rot,
}

impl Farbe {
    /// RGB, wie gezeichnet wird. Kräftige Töne: Das Symbol ist 22 Pixel groß
    /// und steht neben fremden Symbolen in fremden Themes.
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            Farbe::Grau => (0x8a, 0x8f, 0x98),
            Farbe::Blau => (0x3b, 0x82, 0xf6),
            Farbe::Gruen => (0x22, 0xc5, 0x5e),
            Farbe::Gelb => (0xf5, 0x9e, 0x0b),
            Farbe::Rot => (0xef, 0x44, 0x44),
        }
    }
}

/// Wie die Live-Leitung in der Anzeige heisst — mit dem, was sie bedeutet.
const LEITUNG_WAS: &str = "Live-Leitung (AI Brain sieht deine Panes)";

/// Die ganze Anzeige in einem Stück.
///
/// **Zwei Zeilen, nicht eine.** „aus" allein hat wie „nicht verbunden"
/// ausgesehen, obwohl der Rechner an seiner Instanz hing. Verbindung und
/// Leitung sind zwei Zustände und stehen deshalb getrennt da.
#[derive(Debug, PartialEq)]
pub struct Ansicht {
    /// Zeile 1: der Zugang zur Instanz.
    pub verbindung: String,
    /// Zeile 2: die Live-Leitung.
    pub leitung: String,
    /// Zeile 3, wenn es etwas zu sagen gibt: wie es der Leitung geht.
    pub lage: Option<String>,
    /// Woher der Zugang kommt (Rechnername der Instanz) — für den Tooltip,
    /// nicht für die erste Zeile: Dort stünde er im Weg.
    pub quelle: Option<String>,
    pub farbe: Farbe,
    /// Gefülltes Symbol heisst: die Leitung ist an.
    pub gefuellt: bool,
}

impl Ansicht {
    /// Eine Zeile für Tooltip und Fenstertitel.
    ///
    /// Unter Linux hat die Leiste ein mehrzeiliges Tooltip-Feld, unter macOS
    /// und Windows nur eine Zeile — gebraucht wird das hier also dort.
    #[allow(dead_code)]
    pub fn kurz(&self) -> String {
        match &self.lage {
            Some(l) => format!("{} · {} · {}", self.verbindung, self.leitung, l),
            None => format!("{} · {}", self.verbindung, self.leitung),
        }
    }
}

pub fn ansicht(wissen: &Wissen, jetzt: i64) -> Ansicht {
    let status = match wissen {
        Wissen::Unbekannt => {
            return Ansicht {
                verbindung: "Verbindung: wird gelesen …".into(),
                leitung: format!("{LEITUNG_WAS}: …"),
                lage: None,
                quelle: None,
                farbe: Farbe::Gelb,
                gefuellt: false,
            }
        }
        Wissen::Fehler(grund) => {
            return Ansicht {
                verbindung: "Harness antwortet nicht".into(),
                leitung: format!("{LEITUNG_WAS}: unbekannt"),
                lage: Some(grund.clone()),
                quelle: None,
                farbe: Farbe::Rot,
                gefuellt: false,
            }
        }
        Wissen::Da(s) => s,
    };

    // ## „AI Brain", nicht der Instanzname
    //
    // Der Instanzname kommt aus `APP_NAME` und lautet auf einer Instanz, an
    // der das niemand gesetzt hat, „Laravel" — gemessen am 27.10.2026 auf der
    // codingmachine. „Mit Laravel verbunden" sagt dem Menschen in der Leiste
    // nichts und sieht nach einem Fehler aus. Der Produktname ist hier
    // zuverlässiger als ein Feld, das jede Instanz selbst füllt; woher der
    // Zugang kommt, steht im Tooltip.
    let (verbindung, verbunden, quelle) = match &status.verbindung {
        Some(Some(v)) => (
            format!("Mit AI Brain verbunden (Gerät {})", v.geraet),
            true,
            v.basis.as_deref().map(rechnername),
        ),
        Some(None) => ("Nicht mit AI Brain verbunden".into(), false, None),
        // Der Harness ist älter als dieses Feld. Nicht „nicht verbunden"
        // behaupten — das wäre dieselbe Verwechslung noch einmal, nur
        // andersherum.
        None => ("Verbindung: unbekannt (Harness zu alt)".into(), false, None),
    };

    let an_bis = status.bis.filter(|_| status.an);

    // ## Warum der Zusatz in Klammern dasteht (Stand 27.10.2026)
    //
    // „Live-Leitung: aus" allein versteht nur, wer weiss, was die Leitung
    // tut. Gemeldet am 27.10.2026: Die Zeile sagt nicht, worueber sie
    // spricht — und ein Schalter, dessen Wirkung man nicht kennt, wird
    // entweder nie oder versehentlich benutzt.
    //
    // Der Zusatz ist laenger als schoen. Das ist der Preis: Die Zeile steht
    // in einem Menue, das man selten oeffnet, und dort ist Verstaendlichkeit
    // mehr wert als Kuerze.
    let leitung = match an_bis {
        Some(bis) => format!("{LEITUNG_WAS}: an, noch {}", restzeit(bis - jetzt)),
        None => format!("{LEITUNG_WAS}: aus"),
    };

    let (lage, stoerung, laeuft) = lage_text(status, an_bis.is_some());

    let farbe = if stoerung {
        Farbe::Rot
    } else if !verbunden {
        Farbe::Grau
    } else if laeuft {
        Farbe::Gruen
    } else if an_bis.is_some() {
        // An, aber noch nicht übertragend.
        Farbe::Gelb
    } else {
        Farbe::Blau
    };

    Ansicht {
        verbindung,
        leitung,
        lage,
        quelle,
        farbe,
        gefuellt: an_bis.is_some(),
    }
}

/// Die dritte Zeile — und ob sie eine Störung meldet.
///
/// Rückgabe: (Text, Störung, überträgt gerade).
fn lage_text(status: &Status, an: bool) -> (Option<String>, bool, bool) {
    if !an {
        // Ist die Leitung aus, sagt eine alte Lage nichts mehr. Ausser sie
        // erklärt, warum aufgelegt wurde — das will man noch lesen.
        return match &status.lage {
            Some(l) if l.stand == "aufgelegt" => (
                Some(format!(
                    "Zuletzt aufgelegt: {}",
                    l.grund.as_deref().unwrap_or("Grund im Protokoll")
                )),
                false,
                false,
            ),
            _ => (None, false, false),
        };
    }

    match (&status.lage, status.prozess) {
        (Some(l), _) if l.stand == "aufgelegt" => (
            Some(format!(
                "Aufgelegt: {}",
                l.grund.as_deref().unwrap_or("Grund im Protokoll")
            )),
            true,
            false,
        ),
        (_, None) => (
            Some("Leitung läuft nicht, Grund im Protokoll".into()),
            true,
            false,
        ),
        (Some(l), Some(_)) if l.stand == "verbunden" => (
            Some(match &l.pane {
                Some(pane) => format!("Verbunden, zeigt {pane}"),
                None => "Verbunden, zeigt nichts".into(),
            }),
            false,
            true,
        ),
        (Some(l), Some(_)) if l.stand == "getrennt" => (
            Some(format!(
                "Getrennt: {}",
                l.grund.as_deref().unwrap_or("versucht es wieder")
            )),
            true,
            false,
        ),
        (_, Some(_)) => (Some("Verbindet …".into()), false, false),
    }
}

/// Aus `https://brain.example.com/pfad` wird `brain.example.com`.
///
/// Das Schema trägt keine Auskunft, und ein Pfad hat in einer Leiste keinen
/// Platz. Fällt nichts ab, bleibt die Adresse, wie sie ist — lieber zu viel
/// zeigen als nichts.
fn rechnername(basis: &str) -> String {
    let ohne_schema = basis
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(basis);

    let host = ohne_schema.split('/').next().unwrap_or(ohne_schema).trim();

    if host.is_empty() {
        basis.trim().to_string()
    } else {
        host.to_string()
    }
}

pub fn restzeit(sekunden: i64) -> String {
    let minuten = (sekunden.max(0) + 59) / 60;

    match (minuten / 60, minuten % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn da(json: &str) -> Wissen {
        Wissen::Da(serde_json::from_str(json).unwrap())
    }

    #[test]
    fn verbunden_mit_leitung_aus_ist_kein_fehler() {
        let a = ansicht(
            &da(r#"{"an":false,"bis":null,"prozess":null,"lage":null,
                    "verbindung":{"instanz":"AI Brain","geraet":"laptop","basis":"https://x"}}"#),
            0,
        );

        assert_eq!(a.verbindung, "Mit AI Brain verbunden (Gerät laptop)");
        assert_eq!(a.quelle.as_deref(), Some("x"));
        assert_eq!(a.leitung, "Live-Leitung (AI Brain sieht deine Panes): aus");
        assert_eq!(a.farbe, Farbe::Blau);
        assert!(!a.gefuellt);
    }

    #[test]
    fn ohne_zugang_grau() {
        let a = ansicht(
            &da(r#"{"an":false,"bis":null,"prozess":null,"lage":null,"verbindung":null}"#),
            0,
        );

        assert_eq!(a.verbindung, "Nicht mit AI Brain verbunden");
        assert_eq!(a.farbe, Farbe::Grau);
    }

    #[test]
    fn fehlendes_feld_ist_nicht_dasselbe_wie_kein_zugang() {
        let a = ansicht(
            &da(r#"{"an":false,"bis":null,"prozess":null,"lage":null}"#),
            0,
        );

        assert_eq!(a.verbindung, "Verbindung: unbekannt (Harness zu alt)");
    }

    #[test]
    fn laufende_leitung_ist_gruen_und_gefuellt() {
        let a = ansicht(
            &da(r#"{"an":true,"bis":7200,"prozess":7,
                    "lage":{"stand":"verbunden","seit":0,"pane":"bau/2"},
                    "verbindung":{"instanz":"AI Brain","geraet":"laptop"}}"#),
            0,
        );

        assert_eq!(
            a.leitung,
            "Live-Leitung (AI Brain sieht deine Panes): an, noch 2 h"
        );
        assert_eq!(a.lage.as_deref(), Some("Verbunden, zeigt bau/2"));
        assert_eq!(a.farbe, Farbe::Gruen);
        assert!(a.gefuellt);
    }

    #[test]
    fn aufgelegt_zeigt_den_grund_auch_ohne_prozess() {
        let a = ansicht(
            &da(r#"{"an":true,"bis":600,"prozess":null,
                    "lage":{"stand":"aufgelegt","seit":0,"grund":"AI Brain verweigert (403)."},
                    "verbindung":{"instanz":"AI Brain","geraet":"laptop"}}"#),
            0,
        );

        assert_eq!(
            a.lage.as_deref(),
            Some("Aufgelegt: AI Brain verweigert (403).")
        );
        assert_eq!(a.farbe, Farbe::Rot);
    }

    #[test]
    fn an_ohne_prozess_ist_ein_fehler() {
        let a = ansicht(
            &da(r#"{"an":true,"bis":600,"prozess":null,"lage":null,
                    "verbindung":{"instanz":"AI Brain","geraet":"laptop"}}"#),
            0,
        );

        assert_eq!(a.farbe, Farbe::Rot);
    }

    /// Eine Störung bleibt rot, auch wenn gar kein Zugang hinterlegt ist: Der
    /// Fehler ist die dringendere Auskunft.
    #[test]
    fn stoerung_schlaegt_grau() {
        let a = ansicht(
            &da(r#"{"an":true,"bis":600,"prozess":null,"lage":null,"verbindung":null}"#),
            0,
        );

        assert_eq!(a.farbe, Farbe::Rot);
    }

    #[test]
    fn bei_ausgeschalteter_leitung_bleibt_der_grund_lesbar() {
        let a = ansicht(
            &da(r#"{"an":false,"bis":null,"prozess":null,
                    "lage":{"stand":"aufgelegt","seit":0,"grund":"Schalter gefallen."},
                    "verbindung":{"instanz":"AI Brain","geraet":"laptop"}}"#),
            0,
        );

        assert_eq!(
            a.lage.as_deref(),
            Some("Zuletzt aufgelegt: Schalter gefallen.")
        );
        assert_eq!(a.farbe, Farbe::Blau);
    }

    #[test]
    fn harness_stumm_ist_rot() {
        let a = ansicht(&Wissen::Fehler("nicht startbar".into()), 0);

        assert_eq!(a.verbindung, "Harness antwortet nicht");
        assert_eq!(a.farbe, Farbe::Rot);
    }

    /// Der Instanzname wird NICHT angezeigt — er heisst auf echten Instanzen
    /// „Laravel", und das stand vorher in der ersten Zeile.
    #[test]
    fn der_instanzname_steht_nicht_in_der_zeile() {
        let a = ansicht(
            &da(r#"{"an":false,"bis":null,"prozess":null,"lage":null,
                    "verbindung":{"instanz":"Laravel","geraet":"laptop",
                                  "basis":"https://brain.example.com/"}}"#),
            0,
        );

        assert!(!a.verbindung.contains("Laravel"));
        assert_eq!(a.quelle.as_deref(), Some("brain.example.com"));
    }

    #[test]
    fn rechnername_kuerzt_die_adresse() {
        assert_eq!(
            rechnername("https://brain.example.com/x"),
            "brain.example.com"
        );
        assert_eq!(rechnername("brain.example.com"), "brain.example.com");
        assert_eq!(rechnername("https://"), "https://");
    }

    /// Der Zusatz muss in JEDEM Zustand dastehen, auch im Fehlerfall — sonst
    /// erklaert sich die Zeile genau dann nicht, wenn man sie liest.
    #[test]
    fn die_leitung_erklaert_sich_in_jedem_zustand() {
        let faelle = [
            ansicht(&Wissen::Unbekannt, 0),
            ansicht(&Wissen::Fehler("stumm".into()), 0),
            ansicht(
                &da(r#"{"an":false,"bis":null,"prozess":null,"lage":null,"verbindung":null}"#),
                0,
            ),
        ];

        for a in faelle {
            assert!(
                a.leitung.contains("AI Brain sieht deine Panes"),
                "ohne Erklaerung: {}",
                a.leitung
            );
        }
    }

    #[test]
    fn restzeit_rundet_auf() {
        assert_eq!(restzeit(1), "1 min");
        assert_eq!(restzeit(60 * 90), "1 h 30 min");
        assert_eq!(restzeit(8 * 3600), "8 h");
        assert_eq!(restzeit(-5), "0 min");
    }
}
