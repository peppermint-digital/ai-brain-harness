//! Den Menükasten rechnen und malen — ohne Wayland, also überall prüfbar.
//!
//! ## Die Trennung, und warum sie hier liegt
//!
//! Dieses Modul kennt keinen Compositor, keine Fläche, keinen Puffer von
//! aussen: Es bekommt Einträge, einen Stil und eine Schrift, sagt wie gross
//! der Kasten wird, welcher Eintrag unter einem Punkt liegt — und malt in ein
//! `Vec<u32>`. Damit sind Aufbau und Treffergrenzen in gewöhnlichen Tests
//! nachweisbar, und das ist der Teil, an dem ein Menü wirklich falsch sein
//! kann: ein Klick, der eine Zeile daneben landet.
//!
//! ## Logisch und Gerät
//!
//! Zwei Masse, und sie zu verwechseln ist der häufigste Fehler auf einem
//! Schirm mit Skalierung:
//!
//! | | |
//! |---|---|
//! | **logisch** | was Wayland spricht: Flächengrösse, Ränder, Mausstelle |
//! | **Gerät** | was gemalt wird: Pixel im Puffer |
//!
//! `skalierung` ist der Faktor dazwischen (1.0, 1.25, 2.0 …). Hier drin wird
//! in **Gerätepixeln** gerechnet, weil dort auch die Schrift vermessen wird;
//! nach aussen gibt [`Aufbau::logisch`] die Werte, die Wayland hören will.
//! Umgerechnet wird an genau diesen zwei Stellen und sonst nirgends.
//!
//! ## Das Pixelformat
//!
//! `ARGB8888` mit **vorgemultipliziertem** Alpha — so verlangt es `wl_shm`.
//! Wer die Multiplikation vergisst, bekommt an halbdurchsichtigen Kanten
//! helle Säume; das ist der klassische Fehler an dieser Stelle.

use crate::menue::Eintrag;

use super::farben::{Farbe, Stil};
use super::schrift::Schrift;

/// Ein Kasten in **Gerätepixeln**.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kasten {
    pub x: i32,
    pub y: i32,
    pub breite: i32,
    pub hoehe: i32,
}

/// Der gerechnete Aufbau: Kasten, Zeilen, Trefferflächen.
pub struct Aufbau {
    /// Der Kasten in Gerätepixeln, Ursprung ist die linke obere Ecke der
    /// Ausgabe.
    pub kasten: Kasten,
    pub skalierung: f32,
    /// Je Eintrag: Index, Oberkante und Höhe — **relativ zum Kasten**, in
    /// Gerätepixeln.
    zeilen: Vec<(usize, i32, i32)>,
    stil: Stil,
    grad: f32,
    polster: f32,
    rahmen: f32,
    radius: f32,
    haken_spalte: f32,
}

/// Die weiteste Beschriftung darf den Schirm nicht sprengen.
///
/// „Leitung aufräumen (Prozess läuft ohne Schalter)" ist die längste Zeile,
/// die vorkommt, und sie darf vollständig lesbar sein. Eine Fehlerzeile aus
/// dem Harness kann aber beliebig lang sein — die wird dann abgeschnitten,
/// weil ein Menü, das über den halben Schirm läuft, kein Menü mehr ist.
const BREITE_MAX_LOGISCH: f32 = 460.0;

/// Abstand zur Maus und zum Bildschirmrand, logisch.
const LUFT_LOGISCH: f32 = 6.0;

impl Aufbau {
    /// Rechnet den Kasten für `eintraege` an der Mausstelle
    /// `(maus_x, maus_y)` auf einer Fläche von `flaeche_breite` ×
    /// `flaeche_hoehe` — **alle vier logisch**.
    ///
    /// `schrift` muss in Gerätepixeln geladen sein (also mit
    /// `stil.schriftgrad * skalierung`); hier wird nur noch vermessen.
    #[allow(clippy::too_many_arguments)]
    pub fn rechnen(
        eintraege: &[Eintrag],
        stil: &Stil,
        schrift: &mut Schrift,
        skalierung: f32,
        maus_x: f64,
        maus_y: f64,
        flaeche_breite: i32,
        flaeche_hoehe: i32,
    ) -> Self {
        let s = |logisch: f32| (logisch * skalierung).round().max(0.0);

        let polster = s(stil.polster);
        let rahmen = s(stil.rahmenbreite);
        let zeilenhoehe = s(stil.zeilenhoehe).max(8.0);
        let infohoehe = s(stil.zeilenhoehe * 0.78).max(8.0);
        let trennerhoehe = s(9.0).max(3.0);
        let radius = s(stil.radius);
        let haken_spalte = s(stil.schriftgrad * 1.6);

        // Breite: die weiteste Beschriftung plus Häkchenspalte und Luft.
        let mut text_breite: f32 = 0.0;

        for e in eintraege {
            if let Some(t) = e.text() {
                text_breite = text_breite.max(schrift.breite(t));
            }
        }

        // `ceil`, nicht `round`, und ein Pixel Zugabe: Die Breite wird aus
        // genau derselben Textbreite gerechnet, die beim Malen wieder
        // abgezogen wird. Rundet man dabei ab, fehlt ein halbes Pixel — und
        // `kuerzen` macht daraus ein „…". Gemessen am 28.10.2026: „Verbunden
        // mit AI Brain (laptop)" kam als „Verbunden mit AI Brain (lapto…"
        // heraus, obwohl genau diese Zeile die Breite bestimmt hatte.
        let breite = (text_breite + haken_spalte + 2.0 * polster + 2.0 * rahmen + 1.0)
            .ceil()
            .min(s(BREITE_MAX_LOGISCH))
            .max(s(160.0));

        // Höhe: Zeile für Zeile aufsummieren, und dabei gleich festhalten, wo
        // jede sitzt. Zwei Durchläufe über dieselbe Liste wären zwei
        // Gelegenheiten, verschieden zu rechnen.
        let mut lauf = rahmen + polster;
        let mut zeilen = Vec::with_capacity(eintraege.len());

        for (i, e) in eintraege.iter().enumerate() {
            let h = match e {
                Eintrag::Trenner => trennerhoehe,
                Eintrag::Zeile(_) => infohoehe,
                _ => zeilenhoehe,
            };

            zeilen.push((i, lauf.round() as i32, h.round() as i32));
            lauf += h;
        }

        let hoehe = (lauf + polster + rahmen).round();

        // ## Wohin der Kasten kommt
        //
        // Unter den Mauszeiger, denn dort ist das Symbol — und in der Leiste
        // heisst das: direkt unter die Leiste.
        //
        // Waagerecht wird er an der Maus ausgerichtet, aber zum
        // **Bildschirmrand hin** gedreht: Steht das Symbol rechts — und
        // Tray-Symbole stehen rechts —, läuft ein linksbündiger Kasten über
        // den Rand hinaus. Deshalb rechte Kante an die Maus, wenn die Maus in
        // der rechten Hälfte steht.
        let luft = s(LUFT_LOGISCH);
        let flaeche_breite_dev = (flaeche_breite as f32 * skalierung).round();
        let flaeche_hoehe_dev = (flaeche_hoehe as f32 * skalierung).round();
        let maus_x_dev = (maus_x as f32 * skalierung).round();
        let maus_y_dev = (maus_y as f32 * skalierung).round();

        let mut x = if maus_x_dev > flaeche_breite_dev / 2.0 {
            maus_x_dev - breite + s(12.0)
        } else {
            maus_x_dev - s(12.0)
        };

        let mut y = maus_y_dev + luft;

        // Senkrecht kippen, wenn unten kein Platz ist — bei einer Leiste am
        // unteren Bildschirmrand ist das der Normalfall, nicht die Ausnahme.
        if y + hoehe > flaeche_hoehe_dev - luft {
            let oben = maus_y_dev - luft - hoehe;

            y = if oben >= luft {
                oben
            } else {
                // Weder unten noch oben Platz: an den Rand, lieber verschoben
                // als abgeschnitten.
                (flaeche_hoehe_dev - hoehe - luft).max(luft)
            };
        }

        // Und in jedem Fall in die Fläche hinein.
        x = x.clamp(luft, (flaeche_breite_dev - breite - luft).max(luft));
        y = y.clamp(luft, (flaeche_hoehe_dev - hoehe - luft).max(luft));

        Self {
            kasten: Kasten {
                x: x.round() as i32,
                y: y.round() as i32,
                breite: breite as i32,
                hoehe: hoehe as i32,
            },
            skalierung,
            zeilen,
            stil: stil.clone(),
            grad: s(stil.schriftgrad),
            polster,
            rahmen,
            radius,
            haken_spalte,
        }
    }

    /// Der Kasten in **logischen** Masse: `(links, oben, breite, hoehe)`.
    ///
    /// Das sind die vier Zahlen, die die Fläche braucht: `set_margin` für die
    /// ersten zwei, `set_size` für die letzten zwei. Aufgerundet, nicht
    /// gerundet: Eine Fläche, die ein halbes logisches Pixel zu klein ist,
    /// schneidet den Rahmen ab.
    pub fn logisch(&self) -> (i32, i32, i32, i32) {
        let t = |dev: i32| (dev as f32 / self.skalierung).round() as i32;
        let auf = |dev: i32| (dev as f32 / self.skalierung).ceil() as i32;

        (
            t(self.kasten.x),
            t(self.kasten.y),
            auf(self.kasten.breite).max(1),
            auf(self.kasten.hoehe).max(1),
        )
    }

    /// Wie viele Pixel hat der Puffer?
    pub fn pixel(&self) -> usize {
        self.kasten.breite as usize * self.kasten.hoehe as usize
    }

    /// Liegt `(x, y)` überhaupt auf dem Kasten — **kastenlokal**, logisch?
    ///
    /// Gebraucht, um zwei Klicks zu unterscheiden, die beide „kein Treffer"
    /// sind und entgegengesetzt gemeint: auf dem Rahmen (Menü bleibt offen)
    /// und daneben (Menü geht zu). Dass die Frage überhaupt am Kasten
    /// ankommt, liegt an Hyprlands Zeigerumleitung — siehe den Kopf von
    /// `flaeche`.
    pub fn im_kasten(&self, x: f64, y: f64) -> bool {
        let x_dev = (x as f32 * self.skalierung).round() as i32;
        let y_dev = (y as f32 * self.skalierung).round() as i32;

        x_dev >= 0 && y_dev >= 0 && x_dev < self.kasten.breite && y_dev < self.kasten.hoehe
    }

    /// Welcher Eintrag liegt unter `(x, y)` — **kastenlokal**, logisch?
    ///
    /// Gibt den Index in der Eintragsliste zurück. Trenner und Infozeilen
    /// liefern `None`, auch wenn man genau auf ihnen steht: Sie sind keine
    /// Punkte, und eine Hervorhebung auf einer Zeile, die auf keinen Klick
    /// reagiert, ist eine Zusage, die nicht eingelöst wird.
    pub fn treffer(&self, eintraege: &[Eintrag], x: f64, y: f64) -> Option<usize> {
        if !self.im_kasten(x, y) {
            return None;
        }

        let y_dev = (y as f32 * self.skalierung).round() as i32;

        self.zeilen
            .iter()
            .find(|(_, oben, hoehe)| y_dev >= *oben && y_dev < *oben + *hoehe)
            .map(|(i, _, _)| *i)
            .filter(|i| eintraege.get(*i).is_some_and(Eintrag::klickbar))
    }

    /// Der erste bzw. letzte anklickbare Eintrag — für die Tastatur.
    pub fn erster_klickbarer(eintraege: &[Eintrag], rueckwaerts: bool) -> Option<usize> {
        let mut kandidaten: Vec<usize> = eintraege
            .iter()
            .enumerate()
            .filter(|(_, e)| e.klickbar())
            .map(|(i, _)| i)
            .collect();

        if rueckwaerts {
            kandidaten.reverse();
        }

        kandidaten.first().copied()
    }

    /// Der nächste anklickbare Eintrag nach `von`, mit Umlauf.
    pub fn naechster_klickbarer(
        eintraege: &[Eintrag],
        von: Option<usize>,
        rueckwaerts: bool,
    ) -> Option<usize> {
        let Some(von) = von else {
            return Self::erster_klickbarer(eintraege, rueckwaerts);
        };

        let anzahl = eintraege.len();

        for schritt in 1..=anzahl {
            let i = if rueckwaerts {
                (von + anzahl - schritt % anzahl) % anzahl
            } else {
                (von + schritt) % anzahl
            };

            if eintraege[i].klickbar() {
                return Some(i);
            }
        }

        None
    }

    /// Malt das Menü in `puffer` (ARGB8888, vorgemultipliziert).
    ///
    /// `puffer` ist genau `kasten.breite × kasten.hoehe` gross, und alle
    /// Koordinaten darin sind kastenlokal — die Fläche, die den Klick daneben
    /// fängt, ist eine **andere** und bleibt leer.
    pub fn malen(
        &self,
        puffer: &mut [u32],
        eintraege: &[Eintrag],
        schrift: &mut Schrift,
        unter_maus: Option<usize>,
    ) {
        let breite = self.kasten.breite;
        let hoehe = self.kasten.hoehe;

        puffer.iter_mut().for_each(|p| *p = 0);

        // ## Hintergrund und Rahmen in einem Durchgang
        //
        // Zwei Deckungen je Pixel: die äussere Fläche (Kasten mit runden
        // Ecken) und die innere (dieselbe Fläche, um die Rahmenbreite
        // eingerückt). Der Rahmen ist die **Differenz** der beiden, nicht
        // „die äusseren N Pixel" — nur so bleibt er in der Rundung gleich
        // dick.
        //
        // Als Differenz statt übereinander, weil der Hintergrund
        // halbdurchsichtig sein darf (`background-alpha`): Läge die
        // Rahmenfarbe darunter, würde sie durchscheinen und die Fläche
        // einfärben.
        let r = self.rahmen.round() as i32;

        for y in 0..hoehe {
            for x in 0..breite {
                let aussen = deckung_ecke(x, y, breite, hoehe, self.radius);

                if aussen <= 0.0 {
                    continue;
                }

                let innen = if r > 0 {
                    deckung_ecke(
                        x - r,
                        y - r,
                        breite - 2 * r,
                        hoehe - 2 * r,
                        (self.radius - r as f32).max(0.0),
                    )
                } else {
                    aussen
                };

                let auf_rahmen = (aussen - innen).clamp(0.0, 1.0);

                if auf_rahmen > 0.0 {
                    mischen(puffer, breite, hoehe, x, y, self.stil.rahmen, auf_rahmen);
                }

                if innen > 0.0 {
                    mischen(puffer, breite, hoehe, x, y, self.stil.hintergrund, innen);
                }
            }
        }

        for (i, oben, zeilenhoehe) in &self.zeilen {
            let eintrag = &eintraege[*i];

            if let Eintrag::Trenner = eintrag {
                let y = oben + zeilenhoehe / 2;
                let x0 = (self.rahmen + self.polster / 2.0).round() as i32;
                let x1 = breite - x0;
                let dicke = self.rahmen.max(1.0) as i32;

                for yy in y..y + dicke {
                    for xx in x0..x1 {
                        mischen(puffer, breite, hoehe, xx, yy, self.stil.trenner, 1.0);
                    }
                }

                continue;
            }

            let hervor = unter_maus == Some(*i);

            if hervor {
                let x0 = (self.rahmen + self.polster / 3.0).round() as i32;
                let x1 = breite - x0;
                let ecke = (self.radius / 2.0).min(*zeilenhoehe as f32 / 2.0);

                for yy in *oben..oben + zeilenhoehe {
                    for xx in x0..x1 {
                        let d = deckung_ecke(xx - x0, yy - oben, x1 - x0, *zeilenhoehe, ecke);

                        if d > 0.0 {
                            mischen(puffer, breite, hoehe, xx, yy, self.stil.auswahl, d);
                        }
                    }
                }
            }

            let farbe = match eintrag {
                Eintrag::Zeile(_) => self.stil.text_matt,
                _ if !eintrag.klickbar() => self.stil.text_matt,
                _ if hervor => self.stil.auswahl_text,
                _ => self.stil.text,
            };

            let text_x = self.rahmen + self.polster + self.haken_spalte;
            let grundlinie = schrift.grundlinie(*oben as f32, *zeilenhoehe as f32);

            if let Some(t) = eintrag.text() {
                // Zu lange Beschriftungen abschneiden, statt über den Rahmen
                // zu malen.
                let platz = breite as f32 - 2.0 * (self.rahmen + self.polster) - self.haken_spalte;
                let t = kuerzen(t, platz, schrift);

                schrift.schreiben(&t, text_x, grundlinie, |x, y, d| {
                    mischen(puffer, breite, hoehe, x, y, farbe, d as f32 / 255.0);
                });
            }

            // Das Häkchen in die Spalte links vom Text.
            if let Eintrag::Haken { gesetzt: true, .. } = eintrag {
                let mitte_x = self.rahmen + self.polster + self.haken_spalte / 2.0;
                let mitte_y = *oben as f32 + *zeilenhoehe as f32 / 2.0;
                let farbe = if eintrag.klickbar() {
                    self.stil.akzent
                } else {
                    self.stil.text_matt
                };

                haken(
                    puffer,
                    breite,
                    hoehe,
                    mitte_x,
                    mitte_y,
                    self.grad * 0.62,
                    (self.grad / 7.0).max(1.4),
                    farbe,
                );
            }
        }
    }
}

/// Text so weit kürzen, dass er in `platz` Gerätepixel passt.
///
/// Mit `…` am Ende, nicht hart abgeschnitten: Ein Wort, das mitten im
/// Buchstaben endet, liest sich wie ein Darstellungsfehler, und ein Fehler in
/// der Anzeige eines Fehlers verdeckt den Fehler.
fn kuerzen(text: &str, platz: f32, schrift: &mut Schrift) -> String {
    if platz <= 0.0 || schrift.breite(text) <= platz {
        return text.to_string();
    }

    let punkte = schrift.breite("…");
    let mut gekuerzt = String::new();

    for c in text.chars() {
        let versuch = schrift.breite(&gekuerzt) + schrift.breite(&c.to_string()) + punkte;

        if versuch > platz {
            break;
        }

        gekuerzt.push(c);
    }

    gekuerzt.push('…');
    gekuerzt
}

/// Deckung eines Pixels im Kasten mit abgerundeten Ecken: 0 aussen, 1 innen,
/// Zwischenwerte an der Kante (damit die Rundung nicht treppt).
fn deckung_ecke(x: i32, y: i32, breite: i32, hoehe: i32, radius: f32) -> f32 {
    if x < 0 || y < 0 || x >= breite || y >= hoehe || breite <= 0 || hoehe <= 0 {
        return 0.0;
    }

    if radius <= 0.5 {
        return 1.0;
    }

    let r = radius.min(breite as f32 / 2.0).min(hoehe as f32 / 2.0);
    let px = x as f32 + 0.5;
    let py = y as f32 + 0.5;

    // Abstand zur nächsten Ecken-Mitte, aber nur in der Ecke selbst.
    let dx = if px < r {
        r - px
    } else if px > breite as f32 - r {
        px - (breite as f32 - r)
    } else {
        0.0
    };

    let dy = if py < r {
        r - py
    } else if py > hoehe as f32 - r {
        py - (hoehe as f32 - r)
    } else {
        0.0
    };

    if dx == 0.0 || dy == 0.0 {
        return 1.0;
    }

    let abstand = (dx * dx + dy * dy).sqrt();

    (r + 0.5 - abstand).clamp(0.0, 1.0)
}

/// Ein Häkchen aus zwei Strichen.
///
/// Gezeichnet, nicht gesetzt: Ob eine Schrift „✓" (U+2713) enthält, ist nicht
/// zugesagt — in einer Festbreitenschrift ohne Symbolabdeckung kommt ein
/// leeres Kästchen heraus, und ein leeres Kästchen neben „Live-Leitung" liest
/// sich wie ein **nicht** gesetztes Häkchen. Genau falsch.
#[allow(clippy::too_many_arguments)]
fn haken(
    puffer: &mut [u32],
    breite: i32,
    hoehe: i32,
    mitte_x: f32,
    mitte_y: f32,
    groesse: f32,
    dicke: f32,
    farbe: Farbe,
) {
    let h = groesse / 2.0;

    // Vom linken Drittel nach unten in die Spitze, dann nach rechts oben.
    let striche = [
        (
            mitte_x - h,
            mitte_y + h * 0.05,
            mitte_x - h * 0.25,
            mitte_y + h * 0.75,
        ),
        (
            mitte_x - h * 0.25,
            mitte_y + h * 0.75,
            mitte_x + h,
            mitte_y - h * 0.7,
        ),
    ];

    for (x0, y0, x1, y1) in striche {
        strich(puffer, breite, hoehe, x0, y0, x1, y1, dicke, farbe);
    }
}

/// Eine Linie mit weichen Kanten — Abstand Punkt/Strecke je Pixel.
#[allow(clippy::too_many_arguments)]
fn strich(
    puffer: &mut [u32],
    breite: i32,
    hoehe: i32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    dicke: f32,
    farbe: Farbe,
) {
    let halb = dicke / 2.0;
    let von_x = (x0.min(x1) - halb - 1.0).floor().max(0.0) as i32;
    let bis_x = (x0.max(x1) + halb + 1.0).ceil().min(breite as f32) as i32;
    let von_y = (y0.min(y1) - halb - 1.0).floor().max(0.0) as i32;
    let bis_y = (y0.max(y1) + halb + 1.0).ceil().min(hoehe as f32) as i32;

    let dx = x1 - x0;
    let dy = y1 - y0;
    let laenge2 = dx * dx + dy * dy;

    for y in von_y..bis_y {
        for x in von_x..bis_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            let t = if laenge2 > 0.0 {
                (((px - x0) * dx + (py - y0) * dy) / laenge2).clamp(0.0, 1.0)
            } else {
                0.0
            };

            let nx = x0 + t * dx;
            let ny = y0 + t * dy;
            let abstand = ((px - nx).powi(2) + (py - ny).powi(2)).sqrt();
            let deckung = (halb + 0.5 - abstand).clamp(0.0, 1.0);

            if deckung > 0.0 {
                mischen(puffer, breite, hoehe, x, y, farbe, deckung);
            }
        }
    }
}

/// Eine Farbe mit `deckung` über das Pixel legen — ARGB8888,
/// vorgemultipliziert.
fn mischen(
    puffer: &mut [u32],
    breite: i32,
    hoehe: i32,
    x: i32,
    y: i32,
    farbe: Farbe,
    deckung: f32,
) {
    if x < 0 || y < 0 || x >= breite || y >= hoehe {
        return;
    }

    let i = y as usize * breite as usize + x as usize;

    let Some(ziel) = puffer.get_mut(i) else {
        return;
    };

    let a = (farbe.a * deckung).clamp(0.0, 1.0);

    if a <= 0.0 {
        return;
    }

    // Der Untergrund liegt vorgemultipliziert vor, also `src + dst * (1 - a)`.
    let alt = *ziel;
    let alt_a = ((alt >> 24) & 0xff) as f32 / 255.0;
    let alt_r = ((alt >> 16) & 0xff) as f32 / 255.0;
    let alt_g = ((alt >> 8) & 0xff) as f32 / 255.0;
    let alt_b = (alt & 0xff) as f32 / 255.0;

    let rest = 1.0 - a;
    let neu_a = a + alt_a * rest;
    let neu_r = (farbe.r as f32 / 255.0) * a + alt_r * rest;
    let neu_g = (farbe.g as f32 / 255.0) * a + alt_g * rest;
    let neu_b = (farbe.b as f32 / 255.0) * a + alt_b * rest;

    let acht = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;

    *ziel = (acht(neu_a) << 24) | (acht(neu_r) << 16) | (acht(neu_g) << 8) | acht(neu_b);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menue::Aktion;

    fn eintraege() -> Vec<Eintrag> {
        vec![
            Eintrag::Zeile("Verbunden mit AI Brain".into()),
            Eintrag::Zeile("Leitung aus".into()),
            Eintrag::Trenner,
            Eintrag::Haken {
                text: "Live-Leitung".into(),
                gesetzt: false,
                aktiv: true,
                aktion: Aktion::Umschalten,
            },
            Eintrag::Punkt {
                text: "… ab jetzt 30 min".into(),
                aktiv: true,
                aktion: Aktion::An("30m".into()),
            },
            Eintrag::Trenner,
            Eintrag::Punkt {
                text: "AI Brain öffnen".into(),
                aktiv: false,
                aktion: Aktion::Brain,
            },
            Eintrag::Punkt {
                text: "Tray beenden".into(),
                aktiv: true,
                aktion: Aktion::Beenden,
            },
        ]
    }

    /// Ohne eine einzige Schrift auf dem Rechner sagen diese Proben nichts,
    /// statt rot zu werden — das ist der Zustand auf einem CI-Läufer ohne
    /// Oberfläche.
    fn schrift(skalierung: f32) -> Option<Schrift> {
        Schrift::laden(Stil::default().schriftgrad * skalierung).ok()
    }

    fn aufbau(
        liste: &[Eintrag],
        s: &mut Schrift,
        skalierung: f32,
        maus: (f64, f64),
        flaeche: (i32, i32),
    ) -> Aufbau {
        Aufbau::rechnen(
            liste,
            &Stil::default(),
            s,
            skalierung,
            maus.0,
            maus.1,
            flaeche.0,
            flaeche.1,
        )
    }

    /// Jeder klickbare Punkt wird in seiner eigenen Zeile getroffen.
    ///
    /// Das ist die Probe, die den gefährlichsten Fehler fängt: ein Klick, der
    /// eine Zeile daneben landet und „Tray beenden" trifft, wo „Protokoll
    /// öffnen" stand.
    #[test]
    fn jeder_punkt_wird_in_seiner_zeile_getroffen() {
        let Some(mut s) = schrift(1.0) else {
            return;
        };

        let liste = eintraege();
        let a = aufbau(&liste, &mut s, 1.0, (1800.0, 20.0), (1920, 1080));

        let mitte_x = (a.kasten.breite / 2) as f64;

        for (i, e) in liste.iter().enumerate() {
            let (_, oben, hoehe) = a.zeilen.iter().find(|(j, _, _)| *j == i).expect("Zeile");
            let y = (oben + hoehe / 2) as f64;

            let getroffen = a.treffer(&liste, mitte_x, y);

            if e.klickbar() {
                assert_eq!(getroffen, Some(i), "Eintrag {i} ({:?})", e.text());
            } else {
                assert_eq!(getroffen, None, "Eintrag {i} ({:?})", e.text());
            }
        }
    }

    /// Ein Punkt neben dem Kasten trifft nichts — und das ist der Weg, auf
    /// dem das Menü zugeht (#1124, Fall 3).
    #[test]
    fn neben_dem_kasten_trifft_nichts() {
        let Some(mut s) = schrift(1.0) else {
            return;
        };

        let liste = eintraege();
        let a = aufbau(&liste, &mut s, 1.0, (1800.0, 20.0), (1920, 1080));

        for (x, y) in [
            (-1.0, 10.0),
            (a.kasten.breite as f64 + 1.0, 10.0),
            (10.0, -1.0),
            (10.0, a.kasten.hoehe as f64 + 1.0),
        ] {
            assert_eq!(a.treffer(&liste, x, y), None, "({x},{y})");
        }
    }

    /// Der Kasten bleibt auf dem Schirm — an jeder Ecke, bei jeder
    /// Skalierung, auf jeder Grösse.
    ///
    /// Der Fall, der das verlangt: Bastians Symbol sitzt oben **rechts**, und
    /// ein linksbündiger Kasten wäre dort halb draussen. Geprüft werden
    /// zusätzlich die drei anderen Ecken und eine Leiste unten — und 1.25,
    /// weil genau das auf dem Prüfrechner eingestellt ist.
    #[test]
    fn der_kasten_bleibt_auf_dem_schirm() {
        let liste = eintraege();

        for (fb, fh) in [
            (1920, 1080),
            (3072, 2048),
            (1536, 864),
            (1366, 768),
            (800, 600),
        ] {
            for skalierung in [1.0, 1.25, 1.5, 2.0] {
                let Some(mut s) = schrift(skalierung) else {
                    return;
                };

                for (mx, my) in [
                    (fb - 30, 11),
                    (20, 11),
                    (fb - 30, fh - 11),
                    (20, fh - 11),
                    (fb / 2, fh / 2),
                    (0, 0),
                    (fb - 1, fh - 1),
                ] {
                    let a = aufbau(&liste, &mut s, skalierung, (mx as f64, my as f64), (fb, fh));

                    let wo = format!("{fb}x{fh}@{skalierung}, Maus ({mx},{my}): {:?}", a.kasten);

                    assert!(a.kasten.x >= 0 && a.kasten.y >= 0, "links/oben raus — {wo}");

                    // Die Prüfung gegen die Fläche gehört in LOGISCHE Masse:
                    // Das ist das, was Wayland sieht, und dort entscheidet
                    // sich, ob etwas abgeschnitten wird.
                    let (lx, ly, lb, lh) = a.logisch();

                    assert!(lx >= 0 && ly >= 0, "logisch links/oben raus — {wo}");
                    assert!(lx + lb <= fb, "logisch rechts raus ({lx}+{lb}>{fb}) — {wo}");
                    assert!(ly + lh <= fh, "logisch unten raus ({ly}+{lh}>{fh}) — {wo}");
                }
            }
        }
    }

    /// Bei einer Leiste am oberen Rand hängt der Kasten UNTER der Maus — und
    /// bei einer Leiste am unteren Rand darüber.
    #[test]
    fn der_kasten_klappt_nach_oben_wenn_unten_kein_platz_ist() {
        let Some(mut s) = schrift(1.0) else {
            return;
        };

        let liste = eintraege();

        let oben = aufbau(&liste, &mut s, 1.0, (1800.0, 14.0), (1920, 1080));
        assert!(oben.kasten.y > 14, "unter der Maus: {:?}", oben.kasten);

        let unten = aufbau(&liste, &mut s, 1.0, (1800.0, 1070.0), (1920, 1080));
        assert!(
            unten.kasten.y + unten.kasten.hoehe < 1070,
            "über der Maus: {:?}",
            unten.kasten
        );
    }

    /// Steht die Maus rechts, wird der Kasten nach links gedreht — sonst
    /// klebte er am Rand und wäre nicht an seinem Symbol.
    #[test]
    fn rechts_oben_haengt_der_kasten_unter_seinem_symbol() {
        let Some(mut s) = schrift(1.0) else {
            return;
        };

        let liste = eintraege();
        let a = aufbau(&liste, &mut s, 1.0, (1890.0, 11.0), (1920, 1080));
        let (lx, _, lb, _) = a.logisch();

        // Die rechte Kante liegt dicht an der Maus, nicht am Bildrand …
        assert!(
            lx + lb >= 1880 && lx + lb <= 1914,
            "rechte Kante bei {}",
            lx + lb
        );
        // … und die linke weit links davon.
        assert!(lx < 1890 - 100, "linke Kante bei {lx}");
    }

    /// Der Kasten ist innen vollständig gemalt — kein durchsichtiges Loch.
    ///
    /// Durchsichtige Pixel sind erlaubt, wo sie hingehören: in den vier
    /// abgerundeten Ecken. Sonst nirgends, denn ein Menü, durch das man den
    /// Schreibtisch sieht, ist nicht zu lesen.
    #[test]
    fn der_kasten_hat_kein_loch() {
        let Some(mut s) = schrift(1.0) else {
            return;
        };

        let liste = eintraege();
        let a = aufbau(&liste, &mut s, 1.0, (700.0, 20.0), (1920, 1080));

        let mut puffer = vec![0u32; a.pixel()];
        a.malen(&mut puffer, &liste, &mut s, Some(3));

        let ecke = (a.radius.ceil() as i32).max(1);

        for y in 0..a.kasten.hoehe {
            for x in 0..a.kasten.breite {
                let in_der_ecke = (x < ecke || x >= a.kasten.breite - ecke)
                    && (y < ecke || y >= a.kasten.hoehe - ecke);

                if in_der_ecke {
                    continue;
                }

                let alpha = puffer[(y * a.kasten.breite + x) as usize] >> 24;

                assert!(alpha > 0, "Loch bei ({x},{y}), alpha {alpha}");
            }
        }
    }

    /// Die Hervorhebung liegt in der Zeile, über der die Maus steht.
    #[test]
    fn die_hervorhebung_liegt_in_ihrer_zeile() {
        let Some(mut s) = schrift(1.0) else {
            return;
        };

        let liste = eintraege();
        let a = aufbau(&liste, &mut s, 1.0, (700.0, 20.0), (1920, 1080));

        let mut ohne = vec![0u32; a.pixel()];
        a.malen(&mut ohne, &liste, &mut s, None);

        // Eintrag 4 ist „… ab jetzt 30 min".
        let (_, oben, hoehe) = *a.zeilen.iter().find(|(i, _, _)| *i == 4).expect("Zeile");

        let mut mit = vec![0u32; a.pixel()];
        a.malen(&mut mit, &liste, &mut s, Some(4));

        let mut anders_in_der_zeile = 0usize;
        let mut anders_woanders = 0usize;

        for y in 0..a.kasten.hoehe {
            for x in 0..a.kasten.breite {
                let i = (y * a.kasten.breite + x) as usize;

                if mit[i] == ohne[i] {
                    continue;
                }

                if y >= oben && y < oben + hoehe {
                    anders_in_der_zeile += 1;
                } else {
                    anders_woanders += 1;
                }
            }
        }

        assert!(anders_in_der_zeile > 100, "{anders_in_der_zeile} Pixel");
        assert_eq!(anders_woanders, 0, "die Hervorhebung läuft aus ihrer Zeile");
    }

    /// Die längste Beschriftung bestimmt die Breite — und passt dann auch
    /// hinein.
    ///
    /// Klingt nach einer Selbstverständlichkeit und war am 28.10.2026 keine:
    /// Breite und Platz wurden aus derselben Zahl gerechnet, einmal mit
    /// `round` nach unten. Die Zeile, die den Kasten aufgespannt hatte, kam
    /// als „Verbunden mit AI Brain (lapto…" heraus.
    #[test]
    fn die_laengste_zeile_passt_in_ihren_eigenen_kasten() {
        for skalierung in [1.0, 1.25, 1.5, 2.0] {
            let Some(mut s) = schrift(skalierung) else {
                return;
            };

            let liste = vec![
                Eintrag::Zeile("Verbunden mit AI Brain (laptop)".into()),
                Eintrag::Zeile("Leitung aus".into()),
                Eintrag::Punkt {
                    text: "Tray beenden".into(),
                    aktiv: true,
                    aktion: Aktion::Beenden,
                },
            ];

            let a = aufbau(&liste, &mut s, skalierung, (900.0, 20.0), (1920, 1080));
            let platz = a.kasten.breite as f32 - 2.0 * (a.rahmen + a.polster) - a.haken_spalte;

            for e in &liste {
                let t = e.text().expect("Text");
                assert_eq!(
                    kuerzen(t, platz, &mut s),
                    t,
                    "bei Skalierung {skalierung} gekürzt: {t}"
                );
            }
        }
    }

    /// Eine überlange Zeile sprengt den Kasten nicht.
    #[test]
    fn eine_lange_fehlerzeile_wird_gekuerzt() {
        let Some(mut s) = schrift(1.0) else {
            return;
        };

        let lang = "Fehler: ".to_string() + &"sehr lange Begruendung ".repeat(30);
        let liste = vec![Eintrag::Zeile(lang.clone())];

        let a = aufbau(&liste, &mut s, 1.0, (400.0, 20.0), (1920, 1080));

        assert!(
            (a.kasten.breite as f32) <= BREITE_MAX_LOGISCH + 1.0,
            "Kasten {} Pixel breit",
            a.kasten.breite
        );

        let platz = a.kasten.breite as f32 - 2.0 * (a.rahmen + a.polster) - a.haken_spalte;
        let gekuerzt = kuerzen(&lang, platz, &mut s);

        assert!(s.breite(&gekuerzt) <= platz, "passt nicht in {platz}");
        assert!(gekuerzt.ends_with('…'));
        assert!(gekuerzt.starts_with("Fehler: "));
    }

    /// Ein gesetztes Häkchen sieht anders aus als ein leeres.
    ///
    /// Gezählt wird der Unterschied zwischen beiden Bildern, nicht eine
    /// Farbe: Welcher Ton herauskommt, hängt am Theme — DASS ein Häkchen da
    /// ist, darf nicht daran hängen. Und es muss in seiner Spalte liegen:
    /// Ein „Häkchen", das über der Beschriftung landet, wäre auch ein
    /// Unterschied.
    #[test]
    fn ein_gesetztes_haekchen_ist_sichtbar() {
        let Some(mut s) = schrift(1.0) else {
            return;
        };

        let malen = |gesetzt: bool, s: &mut Schrift| -> (Vec<u32>, Aufbau) {
            let liste = vec![Eintrag::Haken {
                text: "Live-Leitung".into(),
                gesetzt,
                aktiv: true,
                aktion: Aktion::Umschalten,
            }];

            let a = Aufbau::rechnen(&liste, &Stil::default(), s, 1.0, 300.0, 20.0, 1920, 1080);

            let mut puffer = vec![0u32; a.pixel()];
            a.malen(&mut puffer, &liste, s, None);

            (puffer, a)
        };

        let (mit, a) = malen(true, &mut s);
        let (ohne, _) = malen(false, &mut s);

        let spalte_bis = (a.rahmen + a.polster + a.haken_spalte).round() as i32;

        let mut anders_in_der_spalte = 0usize;
        let mut anders_daneben = 0usize;

        for y in 0..a.kasten.hoehe {
            for x in 0..a.kasten.breite {
                let i = (y * a.kasten.breite + x) as usize;

                if mit[i] == ohne[i] {
                    continue;
                }

                if x < spalte_bis {
                    anders_in_der_spalte += 1;
                } else {
                    anders_daneben += 1;
                }
            }
        }

        assert!(
            anders_in_der_spalte > 20,
            "nur {anders_in_der_spalte} Pixel Unterschied in der Häkchenspalte"
        );
        assert_eq!(
            anders_daneben, 0,
            "das Häkchen malt ausserhalb seiner Spalte"
        );
    }

    /// Die Tastaturfolge überspringt Trenner, Infozeilen und ausgegraute
    /// Punkte — und läuft um.
    #[test]
    fn die_tastatur_laeuft_nur_ueber_klickbare_punkte() {
        let liste = eintraege();

        let klickbare: Vec<usize> = liste
            .iter()
            .enumerate()
            .filter(|(_, e)| e.klickbar())
            .map(|(i, _)| i)
            .collect();

        assert_eq!(klickbare, vec![3, 4, 7], "die Vorlage hat sich geändert");

        let mut wo = Aufbau::erster_klickbarer(&liste, false);
        assert_eq!(wo, Some(3));

        for erwartet in [4, 7, 3, 4] {
            wo = Aufbau::naechster_klickbarer(&liste, wo, false);
            assert_eq!(wo, Some(erwartet));
        }

        let mut wo = Aufbau::erster_klickbarer(&liste, true);
        assert_eq!(wo, Some(7));

        for erwartet in [4, 3, 7] {
            wo = Aufbau::naechster_klickbarer(&liste, wo, true);
            assert_eq!(wo, Some(erwartet));
        }

        // Ein Menü ohne einen einzigen klickbaren Punkt gibt es nicht — aber
        // wenn, dann darf hier keine Endlosschleife stehen.
        let nur_text = vec![Eintrag::Zeile("nichts".into()), Eintrag::Trenner];
        assert_eq!(Aufbau::erster_klickbarer(&nur_text, false), None);
        assert_eq!(
            Aufbau::naechster_klickbarer(&nur_text, Some(0), false),
            None
        );
    }
}
