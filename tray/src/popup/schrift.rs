//! Die Schrift fürs Menüfenster: finden, laden, Buchstaben rastern.
//!
//! ## Warum `fc-match` und keine Font-Kiste
//!
//! Gesucht ist **die Schrift der Leiste**, nicht irgendeine. Omarchy bindet
//! die Leiste an `monospace` und löst den Namen mit genau diesem Aufruf auf
//! (`Commons/Style.qml`: `fc-match -f "%{family[0]}" monospace`). Wer
//! dieselbe Frage anders stellt, bekommt eine andere Antwort — und ein Menü
//! in einer anderen Schrift als die Leiste darüber sieht aus wie ein
//! Fremdprogramm.
//!
//! `fc-match` liefert mit `%{file}` gleich den Pfad. Eine Kiste, die alle
//! Schriftverzeichnisse selbst einliest, bräuchte dafür einige Dutzend
//! Millisekunden bei jedem Öffnen des Menüs — und `fontconfig` liegt auf
//! jedem Rechner mit Oberfläche.
//!
//! **Ohne `fc-match` geht es trotzdem**: Dann wird eine kurze Liste bekannter
//! Pfade abgeklopft. Findet sich gar keine Schrift, meldet das Fenster das —
//! ein Menü aus leeren Kästchen wäre schlimmer als eine Zeile im Protokoll.
//!
//! ## Was hier NICHT passiert
//!
//! Keine Textformung (kein harfbuzz), keine Ligaturen, kein
//! Zweirichtungssatz. Die Beschriftungen sind deutsche Menütexte in einer
//! Festbreitenschrift; dort ist die Breite eines Wortes die Summe der
//! Buchstabenbreiten. Wer das aufbohrt, soll es tun, wenn eine Beschriftung
//! es braucht — nicht vorher.

use std::collections::HashMap;

/// Ein gerastertes Zeichen.
struct Zeichen {
    breite: usize,
    hoehe: usize,
    /// Deckung je Pixel, 0 bis 255.
    deckung: Vec<u8>,
    /// Verschiebung vom Schreibpunkt, in Pixeln.
    links: i32,
    oben: i32,
    vorschub: f32,
}

pub struct Schrift {
    font: fontdue::Font,
    /// Schriftgrad in **Geräte**-Pixeln, nicht logischen.
    grad: f32,
    /// Gerasterte Zeichen, nach Zeichen. Ein Menü wiederholt seine
    /// Buchstaben reichlich; jedes einmal zu rastern ist der ganze Trick.
    rasterung: HashMap<char, Zeichen>,
    /// Von der Grundlinie bis zur Oberkante der Grossbuchstaben.
    oberkante: f32,
    /// Von der Grundlinie nach unten (als positives Mass).
    unterkante: f32,
}

impl Schrift {
    /// Lädt die Schrift der Leiste in der Grösse `grad` (Gerätepixel).
    pub fn laden(grad: f32) -> Result<Self, String> {
        let pfad = finden().ok_or("Keine Schrift gefunden (fontconfig?).")?;

        let roh = std::fs::read(&pfad)
            .map_err(|e| format!("Schrift {} nicht lesbar: {e}", pfad.display()))?;

        let font = fontdue::Font::from_bytes(roh, fontdue::FontSettings::default())
            .map_err(|e| format!("Schrift {} unlesbar: {e}", pfad.display()))?;

        let masse = font.horizontal_line_metrics(grad).ok_or_else(|| {
            format!(
                "Schrift {} hat keine waagerechten Masse — ist das eine \
                 senkrechte Schrift?",
                pfad.display()
            )
        })?;

        Ok(Self {
            font,
            grad,
            rasterung: HashMap::new(),
            oberkante: masse.ascent,
            // `descent` zaehlt nach unten negativ.
            unterkante: -masse.descent,
        })
    }

    /// Die Grundlinie fuer eine Zeile, die bei `oben` beginnt und
    /// `zeilenhoehe` hoch ist — senkrecht mittig.
    ///
    /// Gerechnet aus den Massen der Schrift und nicht mit einem Faktor auf
    /// den Schriftgrad: Wie viel ueber und unter der Grundlinie liegt, ist
    /// von Schrift zu Schrift verschieden, und ein Faktor, der bei
    /// JetBrainsMono passt, setzt DejaVu einen Hauch zu hoch. Hier stand bis
    /// zum 28.10.2026 `grad * 0.80`.
    pub fn grundlinie(&self, oben: f32, zeilenhoehe: f32) -> f32 {
        let hoehe = self.oberkante + self.unterkante;

        oben + (zeilenhoehe - hoehe) / 2.0 + self.oberkante
    }

    fn zeichen(&mut self, c: char) -> &Zeichen {
        self.rasterung.entry(c).or_insert_with(|| {
            let (masse, deckung) = self.font.rasterize(c, self.grad);

            Zeichen {
                breite: masse.width,
                hoehe: masse.height,
                deckung,
                links: masse.xmin,
                // `ymin` zählt von der Grundlinie nach oben. Für das Malen
                // brauchen wir den Abstand der Oberkante des Kastens von der
                // Grundlinie, nach unten positiv.
                oben: -(masse.height as i32 + masse.ymin),
                vorschub: masse.advance_width,
            }
        })
    }

    /// Wie breit wird dieser Text in Gerätepixeln?
    pub fn breite(&mut self, text: &str) -> f32 {
        text.chars().map(|c| self.zeichen(c).vorschub).sum()
    }

    /// Hat diese Schrift das Zeichen — oder käme ein leeres Kästchen heraus?
    ///
    /// ## Wozu diese Frage überhaupt gestellt wird
    ///
    /// Omarchys Tray-Menü setzt das Häkchen als Zeichen: `""` aus dem
    /// Nerd-Font-Bereich. Auf einer Omarchy-Installation steht dort
    /// „JetBrainsMono Nerd Font", die es hat — und dann ist das **genau** das
    /// Häkchen, das in jedem anderen Menü der Leiste steht.
    ///
    /// Auf einem Rechner ohne Nerd Font ist derselbe Codepunkt nicht belegt.
    /// `fontdue` rastert dann das Ersatzzeichen, und das ist bei den meisten
    /// Schriften ein **leeres Kästchen** — neben „Live-Leitung" liest sich
    /// das wie ein nicht gesetztes Häkchen. Genau falsch, und still falsch.
    ///
    /// Deshalb wird gefragt, statt gehofft: Hat die Schrift das Zeichen,
    /// kommt Omarchys Häkchen; hat sie es nicht, zeichnet `malen::haken` zwei
    /// Striche. `lookup_glyph_index` gibt für ein unbelegtes Zeichen 0 zurück
    /// — das ist in jeder TrueType-Schrift das `.notdef`-Zeichen, also
    /// zugleich der Index, den man NICHT haben will.
    pub fn hat_zeichen(&self, c: char) -> bool {
        self.font.lookup_glyph_index(c) != 0
    }

    /// Schreibt `text` mit der Grundlinie bei `grundlinie`, linke Kante bei
    /// `x`, und ruft für jedes Pixel `setzen(x, y, deckung)`.
    ///
    /// Der Rückruf statt eines Puffers: Das Malen kennt den Puffer, die
    /// Schrift nur Buchstaben. So lässt sich dieselbe Zeile in einen
    /// Fensterpuffer schreiben und in einer Probe zählen.
    pub fn schreiben<F: FnMut(i32, i32, u8)>(
        &mut self,
        text: &str,
        x: f32,
        grundlinie: f32,
        mut setzen: F,
    ) {
        let mut stift = x;

        for c in text.chars() {
            // Der Eintrag wird hier kurz ausgelesen und kopiert, weil
            // `setzen` nebenher auf den Puffer schreibt und `self` dabei
            // nicht geliehen bleiben soll.
            let (breite, hoehe, links, oben, vorschub) = {
                let z = self.zeichen(c);
                (z.breite, z.hoehe, z.links, z.oben, z.vorschub)
            };

            if breite > 0 && hoehe > 0 {
                let x0 = (stift + links as f32).round() as i32;
                let y0 = (grundlinie + oben as f32).round() as i32;

                // Noch einmal holen: Zwischen den beiden Blöcken hat niemand
                // die Rasterung angefasst, aber der Entleiher weiss das nicht.
                let deckung = &self.rasterung[&c].deckung;

                for zy in 0..hoehe {
                    for zx in 0..breite {
                        let d = deckung[zy * breite + zx];

                        if d > 0 {
                            setzen(x0 + zx as i32, y0 + zy as i32, d);
                        }
                    }
                }
            }

            stift += vorschub;
        }
    }
}

/// Der Pfad zur Schrift der Leiste.
///
/// `OMARCHY_MENU_FONT` zuerst: Omarchy bietet diese Variable für Leute an,
/// die im Menü eine andere Familie wollen als in der Leiste
/// (`Commons/Style.qml`). Wer sie gesetzt hat, meint sie.
fn finden() -> Option<std::path::PathBuf> {
    let mut muster: Vec<String> = Vec::new();

    if let Ok(eigen) = std::env::var("OMARCHY_MENU_FONT") {
        if !eigen.trim().is_empty() {
            muster.push(eigen);
        }
    }

    muster.push("monospace".to_string());
    muster.push("sans-serif".to_string());

    for m in &muster {
        if let Some(pfad) = fc_match(m) {
            return Some(pfad);
        }
    }

    // Ohne fontconfig: eine Handvoll Pfade, die es auf den meisten
    // Linux-Rechnern gibt. Keine Suche über den ganzen Baum — ein Menü, das
    // beim Öffnen Verzeichnisse durchläuft, öffnet zu langsam.
    const BEKANNT: [&str; 8] = [
        "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
    ];

    BEKANNT
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.is_file())
}

fn fc_match(muster: &str) -> Option<std::path::PathBuf> {
    let ausgabe = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", muster])
        .output()
        .ok()?;

    if !ausgabe.status.success() {
        return None;
    }

    let pfad = String::from_utf8_lossy(&ausgabe.stdout).trim().to_string();

    if pfad.is_empty() {
        return None;
    }

    let pfad = std::path::PathBuf::from(pfad);

    pfad.is_file().then_some(pfad)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Auf einem Laufzeitsystem ohne eine einzige Schrift läuft diese Probe
    /// nicht — sie sagt dann nichts, statt rot zu werden. Genau das ist der
    /// Zustand auf einem CI-Läufer ohne Oberfläche.
    fn schrift() -> Option<Schrift> {
        Schrift::laden(16.0).ok()
    }

    #[test]
    fn eine_breitere_zeile_ist_breiter() {
        let Some(mut s) = schrift() else {
            return;
        };

        let kurz = s.breite("AI Brain");
        let lang = s.breite("AI Brain öffnen");

        assert!(lang > kurz, "{lang} muss grösser sein als {kurz}");
        assert!(kurz > 0.0);
        assert_eq!(s.breite(""), 0.0);
    }

    /// Die deutschen Umlaute und das Auslassungszeichen kommen in jeder
    /// Beschriftung vor. Eine Schrift, die sie nicht hat, zeichnet leere
    /// Kästchen — das soll auffallen.
    #[test]
    fn umlaute_und_auslassung_werden_gezeichnet() {
        let Some(mut s) = schrift() else {
            return;
        };

        for c in ['ö', 'ä', 'ü', '…', 'L'] {
            let mut pixel = 0usize;
            s.schreiben(&c.to_string(), 0.0, 20.0, |_, _, d| {
                if d > 0 {
                    pixel += 1;
                }
            });

            assert!(pixel > 0, "{c} wurde nicht gezeichnet");
        }
    }

    /// Die Grundlinie sitzt mittig in ihrer Zeile — mit Luft oben und unten.
    #[test]
    fn die_grundlinie_sitzt_mittig() {
        let Some(s) = schrift() else {
            return;
        };

        let g = s.grundlinie(100.0, 40.0);

        assert!(g > 100.0 + s.oberkante - 1.0, "zu hoch: {g}");
        assert!(g < 140.0, "unter der Zeile: {g}");
        // Die Luft oben und unten ist gleich gross.
        let oben = g - s.oberkante - 100.0;
        let unten = 140.0 - (g + s.unterkante);
        assert!((oben - unten).abs() < 0.01, "oben {oben}, unten {unten}");
    }

    /// Der Text landet dort, wo er hingehört: rechts vom Schreibpunkt und
    /// über der Grundlinie.
    #[test]
    fn der_text_steht_am_schreibpunkt() {
        let Some(mut s) = schrift() else {
            return;
        };

        let (mut min_x, mut max_y) = (i32::MAX, i32::MIN);

        // Ein Wort OHNE Unterlängen: `g`, `p` oder `j` hängen unter die
        // Grundlinie, und zwar zu Recht — mit „Leitung" prüfte diese Zeile
        // eine falsche Erwartung statt des Codes.
        s.schreiben("AI Brain", 40.0, 100.0, |x, y, _| {
            min_x = min_x.min(x);
            max_y = max_y.max(y);
        });

        assert!(min_x >= 38, "linke Kante bei {min_x}, erwartet ab 40");
        assert!(max_y <= 101, "Unterkante bei {max_y}, Grundlinie war 100");

        // Und mit Unterlänge geht es tiefer — sonst wäre oben nichts bewiesen.
        let mut tiefer = i32::MIN;
        s.schreiben("g", 40.0, 100.0, |_, y, _| tiefer = tiefer.max(y));
        assert!(tiefer > 101, "g reicht nur bis {tiefer}");
    }
}
