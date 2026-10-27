//! Das Symbol wird selbst gezeichnet, nicht aus dem Theme geholt.
//!
//! ## Warum nicht `icon_name` (Stand 27.10.2026)
//!
//! Bis zum 27.10.2026 nannte der Tray Theme-Symbole (`network-offline`,
//! `network-transmit-receive`). Das war weniger Code und sah unter einem
//! hellen Theme gut aus — unter Bastians dunklem Omarchy-Theme kam es als
//! **dunkel auf dunkel** heraus und war nicht zu finden. Ein Theme liefert
//! eine Form, aber keine Zusage über den Kontrast zur Leiste.
//!
//! Der naheliegende Ausweg wäre, das Theme zu erkennen
//! (`~/.local/state/omarchy/current/`) und zwei Symbolsätze zu führen. Das
//! löst den Fall Omarchy und keinen anderen: KDE, GNOME und jede Leiste mit
//! halbdurchsichtigem Hintergrund bleiben offen, und macOS und Windows haben
//! gar kein Omarchy-Theme.
//!
//! Deshalb zeichnen wir selbst — und zwar mit **beiden** Kontrastkanten:
//! innen die Farbe des Zustands, darum ein weisser Saum, darum eine dunkle
//! Kante. Auf dunkler Leiste trägt der weisse Saum, auf heller die dunkle
//! Kante. Damit braucht es kein Theme-Wissen, und dasselbe Bild geht auch an
//! die Leisten von macOS und Windows.
//!
//! ## Was die Form sagt
//!
//! * **Farbe** — die Verbindung: grau ohne Zugang, blau verbunden, grün
//!   überträgt, gelb unterwegs, rot gestört.
//! * **Füllung** — die Live-Leitung: gefüllt heisst an, hohl heisst aus.
//!
//! Zwei Aussagen in einem Symbol, ohne Text. Welche Farbe was heisst, steht
//! ausserdem im Menü — das Symbol ist die Abkürzung, nicht die einzige Quelle.

use crate::ansicht::Farbe;

/// Ein gezeichnetes Symbol, RGBA, 8 Bit je Kanal, nicht vormultipliziert.
pub struct Bild {
    pub kante: u32,
    pub rgba: Vec<u8>,
}

impl Bild {
    /// Dasselbe Bild als ARGB32 in Netzwerk-Byte-Reihenfolge — so will es
    /// StatusNotifierItem.
    pub fn argb(&self) -> Vec<u8> {
        let mut aus = Vec::with_capacity(self.rgba.len());

        for p in self.rgba.as_chunks::<4>().0 {
            aus.extend_from_slice(&[p[3], p[0], p[1], p[2]]);
        }

        aus
    }
}

/// Wie fein geglättet wird. 4×4 Proben je Pixel reichen bei 22 Pixeln Kante
/// und kosten nichts, was auffällt: gezeichnet wird nur, wenn sich der
/// Zustand ändert.
const PROBEN: u32 = 4;

/// Die Ringe, von aussen nach innen, als Anteil der Kantenlänge.
const R_KANTE: f32 = 0.48;
const R_SAUM: f32 = 0.44;
const R_FARBE: f32 = 0.40;
const R_LOCH: f32 = 0.26;

pub fn zeichnen(farbe: Farbe, gefuellt: bool, kante: u32) -> Bild {
    let (fr, fg, fb) = farbe.rgb();
    let mitte = kante as f32 / 2.0;
    let k = kante as f32;

    let mut rgba = Vec::with_capacity((kante * kante * 4) as usize);

    for y in 0..kante {
        for x in 0..kante {
            // Vormultipliziert aufsummieren, sonst zieht eine Kante mit
            // Alpha 0 ihre Farbe (Schwarz) in die Mischung.
            let mut summe = [0.0f32; 3];
            let mut alpha = 0.0f32;

            for py in 0..PROBEN {
                for px in 0..PROBEN {
                    let fx = x as f32 + (px as f32 + 0.5) / PROBEN as f32;
                    let fy = y as f32 + (py as f32 + 0.5) / PROBEN as f32;
                    let r = ((fx - mitte).powi(2) + (fy - mitte).powi(2)).sqrt() / k;

                    let (pr, pg, pb, pa) = schicht(r, (fr, fg, fb), gefuellt);

                    summe[0] += pr as f32 * pa;
                    summe[1] += pg as f32 * pa;
                    summe[2] += pb as f32 * pa;
                    alpha += pa;
                }
            }

            let proben = (PROBEN * PROBEN) as f32;

            if alpha <= 0.0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }

            rgba.extend_from_slice(&[
                (summe[0] / alpha).round().clamp(0.0, 255.0) as u8,
                (summe[1] / alpha).round().clamp(0.0, 255.0) as u8,
                (summe[2] / alpha).round().clamp(0.0, 255.0) as u8,
                (alpha / proben * 255.0).round().clamp(0.0, 255.0) as u8,
            ]);
        }
    }

    Bild { kante, rgba }
}

/// Welche Farbe liegt an diesem Abstand (Anteil der Kante)?
fn schicht(r: f32, farbe: (u8, u8, u8), gefuellt: bool) -> (u8, u8, u8, f32) {
    let (fr, fg, fb) = farbe;

    if r > R_KANTE {
        return (0, 0, 0, 0.0);
    }

    if r > R_SAUM {
        // Dunkle Kante: trägt auf heller Leiste.
        return (0x10, 0x14, 0x18, 0.62);
    }

    if r > R_FARBE {
        // Weisser Saum: trägt auf dunkler Leiste.
        return (0xff, 0xff, 0xff, 0.94);
    }

    if r > R_LOCH || gefuellt {
        return (fr, fg, fb, 1.0);
    }

    // Das Loch bleibt frei — so unterscheidet man „Leitung aus" von „an",
    // ohne eine zweite Farbe zu verbrauchen.
    (0, 0, 0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(b: &Bild, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let i = ((y * b.kante + x) * 4) as usize;
        (b.rgba[i], b.rgba[i + 1], b.rgba[i + 2], b.rgba[i + 3])
    }

    #[test]
    fn groesse_passt() {
        let b = zeichnen(Farbe::Blau, false, 22);
        assert_eq!(b.rgba.len(), 22 * 22 * 4);
        assert_eq!(b.argb().len(), 22 * 22 * 4);
    }

    /// Der Kern unterscheidet an von aus — das ist die ganze Füllung.
    #[test]
    fn hohl_hat_ein_loch_gefuellt_nicht() {
        let hohl = zeichnen(Farbe::Blau, false, 22);
        let voll = zeichnen(Farbe::Gruen, true, 22);

        assert_eq!(pixel(&hohl, 11, 11).3, 0);
        assert_eq!(pixel(&voll, 11, 11).3, 255);
    }

    /// Die Zustandsfarbe muss im Ring wirklich ankommen.
    #[test]
    fn der_ring_traegt_die_farbe() {
        let b = zeichnen(Farbe::Rot, false, 22);
        let (r, g, _, a) = pixel(&b, 11, 4); // Abstand ≈ 0.32 der Kante

        assert_eq!(a, 255);
        assert!(r > 200 && g < 120, "erwartet rot, ist ({r},{g})");
    }

    /// Beide Kontrastkanten müssen da sein, sonst ist das Symbol wieder nur
    /// auf einem der beiden Leistentypen zu sehen.
    ///
    /// Geprüft wird die ganze Spalte durch die Mitte, nicht ein berechneter
    /// Pixel: An einer Schichtgrenze mischt die Glättung beide Farben, und
    /// ein Test, der auf eine Grenze zeigt, misst die Glättung statt der
    /// Kante — und schlägt beim nächsten Verschieben eines Radius fehl, ohne
    /// dass etwas kaputt wäre.
    #[test]
    fn weisser_saum_und_dunkle_kante() {
        let b = zeichnen(Farbe::Blau, true, 44);
        let mut saum = false;
        let mut kante = false;

        for y in 0..22 {
            let (r, g, bl, a) = pixel(&b, 22, y);

            if r > 230 && g > 230 && bl > 230 && a > 200 {
                saum = true;
            }

            if r < 90 && g < 90 && bl < 90 && a > 80 {
                kante = true;
            }
        }

        assert!(saum, "kein weisser Saum — auf dunkler Leiste unsichtbar");
        assert!(kante, "keine dunkle Kante — auf heller Leiste unsichtbar");
    }

    /// Aussen muss durchsichtig bleiben — ein Symbol mit Hintergrundkasten
    /// sieht in jeder Leiste falsch aus.
    #[test]
    fn die_ecken_bleiben_frei() {
        let b = zeichnen(Farbe::Gruen, true, 22);
        assert_eq!(pixel(&b, 0, 0).3, 0);
        assert_eq!(pixel(&b, 21, 21).3, 0);
    }

    #[test]
    fn argb_dreht_die_kanaele_richtig() {
        let b = Bild {
            kante: 1,
            rgba: vec![1, 2, 3, 4],
        };

        assert_eq!(b.argb(), vec![4, 1, 2, 3]);
    }
}
