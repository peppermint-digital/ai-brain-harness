//! Die Farben und Masse des Menüfensters — aus dem Theme, nicht erfunden.
//!
//! ## Woher sie kommen
//!
//! Omarchy legt das aktuelle Theme unter
//! `~/.local/state/omarchy/current/theme/` ab (ein Verweis auf das gewählte
//! Theme). Zwei Dateien zählen hier:
//!
//! | Datei | Was |
//! |---|---|
//! | `shell.toml` | die **Oberflächen**: `[popups]`, `[menu]`, `[font]`, `[spacing]`, `[hyprland]` |
//! | `colors.toml` | die Grundtöne, auf die `shell.toml` zurückgeht |
//!
//! Wir lesen genau die Schlüssel, die Omarchys eigene Flyouts lesen
//! (`Commons/Color.qml` → `popups`, `menu`): Hintergrund, Text, Rahmen, die
//! Auswahlfarben und die Alpha-Begleiter. Damit sieht unser Fenster aus wie
//! ein Flyout der Leiste und nicht wie ein Fremdkörper daneben.
//!
//! ## Warum ein eigener Mini-Leser statt der `toml`-Kiste
//!
//! Gebraucht wird `[abschnitt]` plus `schluessel = wert` — sonst nichts, kein
//! Array, keine Tabelle in einer Tabelle, keine mehrzeiligen Zeichenketten.
//! Dafür eine Kiste samt `serde`-Ableitung mitzuschleppen, kostet mehr als es
//! trägt; das hier sind dreissig Zeilen, die genau das können und an einer
//! fremden Datei nicht scheitern, sondern auf die Vorgaben zurückfallen.
//!
//! **Ohne Theme sieht es trotzdem gut aus.** Jede Angabe hat eine Vorgabe
//! (Tokyo-Night-nahes Dunkel), und ein fehlendes Theme ist der Normalfall auf
//! jedem Rechner, der nicht Omarchy ist — waybar, KDE, GNOME.

use std::collections::HashMap;

/// Eine Farbe, wie gezeichnet wird: vorgemultipliziertes ARGB kommt erst beim
/// Malen, hier stehen die Kanäle einzeln.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Farbe {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// 0.0 unsichtbar bis 1.0 deckend.
    pub a: f32,
}

impl Farbe {
    pub const fn neu(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Dieselbe Farbe mit anderer Deckung.
    pub fn mit_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// `#rrggbb`, `#rgb` oder `#rrggbbaa`.
    ///
    /// Auch `rgba(1a1b26ff)` — so schreibt Hyprland Farben, und `shell.toml`
    /// erlaubt diese Schreibweise für Rahmen samt Verlauf. Vom Verlauf nehmen
    /// wir den ersten Ton: Ein Verlauf auf einem 1-Pixel-Rahmen ist nicht der
    /// Unterschied, an dem dieses Fenster hängt.
    pub fn lesen(wert: &str) -> Option<Self> {
        let wert = wert.trim().trim_matches('"').trim();

        if let Some(rest) = wert
            .strip_prefix("rgba(")
            .or_else(|| wert.strip_prefix("rgb("))
        {
            let inhalt = rest.split(')').next()?.trim();
            return Self::aus_hex(inhalt);
        }

        // Ein Verlauf: „#aaa #bbb 45deg" oder „rgba(…) rgba(…) 45deg".
        let erster = wert.split_whitespace().next()?;

        Self::aus_hex(erster.trim_start_matches('#'))
    }

    fn aus_hex(h: &str) -> Option<Self> {
        let h = h.trim().trim_start_matches('#');
        let zahl = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();

        match h.len() {
            3 => {
                let einzeln = |i: usize| {
                    u8::from_str_radix(h.get(i..i + 1)?, 16)
                        .ok()
                        .map(|v| v * 17)
                };
                Some(Self::neu(einzeln(0)?, einzeln(1)?, einzeln(2)?, 1.0))
            }
            6 => Some(Self::neu(zahl(0)?, zahl(2)?, zahl(4)?, 1.0)),
            8 => Some(Self::neu(
                zahl(0)?,
                zahl(2)?,
                zahl(4)?,
                zahl(6)? as f32 / 255.0,
            )),
            _ => None,
        }
    }
}

/// Alles, was das Fenster zum Zeichnen braucht.
#[derive(Clone, Debug)]
pub struct Stil {
    pub hintergrund: Farbe,
    pub rahmen: Farbe,
    pub text: Farbe,
    /// Die Infozeilen und ausgegraute Punkte.
    pub text_matt: Farbe,
    pub trenner: Farbe,
    /// Die Fläche unter dem Punkt, auf dem die Maus steht.
    pub auswahl: Farbe,
    pub auswahl_text: Farbe,
    /// Das Häkchen.
    pub akzent: Farbe,
    /// Schriftgrad in logischen Pixeln.
    pub schriftgrad: f32,
    /// Höhe einer klickbaren Zeile, logisch.
    pub zeilenhoehe: f32,
    /// Luft zwischen Rahmen und Inhalt, logisch.
    pub polster: f32,
    pub rahmenbreite: f32,
    pub radius: f32,
}

impl Default for Stil {
    /// Die Vorgabe, wenn kein Theme da ist.
    ///
    /// Bewusst nicht Schwarz auf Weiss: Der Tray hängt in einer Leiste, und
    /// Leisten sind in der überwiegenden Mehrheit dunkel. Ein helles Fenster
    /// auf einem dunklen Schirm ist der unangenehmere Fehlgriff — das war
    /// schon beim Symbol der Punkt („dunkel auf dunkel", siehe `symbol.rs`).
    fn default() -> Self {
        Self {
            hintergrund: Farbe::neu(0x1a, 0x1b, 0x26, 1.0),
            rahmen: Farbe::neu(0x7a, 0xa2, 0xf7, 1.0),
            text: Farbe::neu(0xa9, 0xb1, 0xd6, 1.0),
            text_matt: Farbe::neu(0xa9, 0xb1, 0xd6, 0.55),
            trenner: Farbe::neu(0xa9, 0xb1, 0xd6, 0.18),
            auswahl: Farbe::neu(0xa9, 0xb1, 0xd6, 0.10),
            auswahl_text: Farbe::neu(0x7a, 0xa2, 0xf7, 1.0),
            akzent: Farbe::neu(0x7a, 0xa2, 0xf7, 1.0),
            schriftgrad: 12.0,
            zeilenhoehe: 28.0,
            polster: 10.0,
            rahmenbreite: 1.0,
            radius: 10.0,
        }
    }
}

impl Stil {
    /// Den Stil aus dem laufenden Theme lesen.
    ///
    /// Scheitert nie: Was fehlt, bleibt auf der Vorgabe. Ein Menü, das wegen
    /// einer unleserlichen Themedatei gar nicht aufgeht, wäre der schlechtere
    /// Tausch — es geht hier um Farben.
    pub fn aus_theme() -> Self {
        let mut stil = Self::default();

        let Some(ordner) = theme_ordner() else {
            return stil;
        };

        let shell = lesen_toml(&ordner.join("shell.toml"));

        // `shell.toml` verweist mit „hyprland.active-border" auf einen anderen
        // Schlüssel in derselben Datei. Einmal auflösen reicht; eine Kette
        // gibt es dort nicht, und eine Schleife soll es nicht geben.
        let hole = |abschnitt: &str, schluessel: &str| -> Option<String> {
            let wert = shell.get(&(abschnitt.to_string(), schluessel.to_string()))?;
            let roh = wert.trim().trim_matches('"').to_string();

            if roh.starts_with('#') || roh.starts_with("rgb") {
                return Some(roh);
            }

            // Ein Verweis: „abschnitt.schluessel".
            if let Some((a, s)) = roh.split_once('.') {
                if let Some(ziel) = shell.get(&(a.to_string(), s.to_string())) {
                    return Some(ziel.trim().trim_matches('"').to_string());
                }
            }

            Some(roh)
        };

        let zahl = |abschnitt: &str, schluessel: &str| -> Option<f32> {
            hole(abschnitt, schluessel)?.parse::<f32>().ok()
        };

        let farbe = |abschnitt: &str, schluessel: &str| -> Option<Farbe> {
            Farbe::lesen(&hole(abschnitt, schluessel)?)
        };

        if let Some(f) = farbe("popups", "background") {
            stil.hintergrund = f.mit_alpha(zahl("popups", "background-alpha").unwrap_or(1.0));
        }

        if let Some(f) = farbe("popups", "border") {
            stil.rahmen = f.mit_alpha(zahl("popups", "border-alpha").unwrap_or(1.0));
        }

        if let Some(f) = farbe("popups", "text") {
            stil.text = f;
            stil.text_matt = f.mit_alpha(0.55);
            stil.trenner = f.mit_alpha(0.18);
        }

        if let Some(f) = farbe("menu", "selected-background") {
            stil.auswahl = f.mit_alpha(
                zahl("menu", "selected-background-alpha")
                    .or_else(|| zahl("controls", "hover-cursor-fill-alpha"))
                    .unwrap_or(0.10),
            );
        }

        if let Some(f) = farbe("menu", "selected-text") {
            stil.auswahl_text = f;
        }

        if let Some(f) = farbe("hyprland", "active-border") {
            stil.akzent = f;
        }

        if let Some(v) = zahl("font", "base-size") {
            // Der Schirm darf gross sein, die Schrift nicht beliebig: Unter
            // 8 Pixel ist nichts mehr lesbar, über 24 passt kein Menü mehr
            // auf einen Laptop-Schirm.
            stil.schriftgrad = v.clamp(8.0, 24.0);
        }

        if let Some(v) = zahl("spacing", "popup-row-height") {
            stil.zeilenhoehe = v.clamp(16.0, 64.0);
        } else {
            // Omarchys Zeilenhöhe wächst mit dem Schriftgrad
            // (`scale-with-font`). 28 bei 12 ist das Verhältnis aus der
            // Vorgabedatei.
            stil.zeilenhoehe = (stil.schriftgrad * 28.0 / 12.0).clamp(16.0, 64.0);
        }

        if let Some(v) = zahl("spacing", "popup-padding") {
            stil.polster = v.clamp(2.0, 32.0);
        }

        if let Some(v) = zahl("popups", "border-width") {
            stil.rahmenbreite = v.clamp(0.0, 6.0);
        }

        stil
    }
}

/// Der Ordner des laufenden Themes — oder nichts.
///
/// Der Pfad steht so in `Commons/Color.qml`: `$HOME/.local/state` plus
/// `/omarchy/current/theme`. `XDG_STATE_HOME` respektieren wir trotzdem; wer
/// es setzt, hat einen Grund, und Omarchy selbst liest es nur deshalb nicht,
/// weil es den Wert fest zusammensetzt.
fn theme_ordner() -> Option<std::path::PathBuf> {
    let basis = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            Some(
                std::path::PathBuf::from(std::env::var_os("HOME")?)
                    .join(".local")
                    .join("state"),
            )
        })?;

    let ordner = basis.join("omarchy").join("current").join("theme");

    ordner.is_dir().then_some(ordner)
}

/// `[abschnitt]` plus `schluessel = wert`, nach `(abschnitt, schluessel)`.
///
/// Kommentare (`#`) und leere Zeilen fallen weg, Werte behalten ihre
/// Anführungszeichen — die nimmt erst der Leser weg, weil ein Zahlenwert
/// keine hat.
fn lesen_toml(pfad: &std::path::Path) -> HashMap<(String, String), String> {
    let mut werte = HashMap::new();

    let Ok(inhalt) = std::fs::read_to_string(pfad) else {
        return werte;
    };

    let mut abschnitt = String::new();

    for zeile in inhalt.lines() {
        let zeile = zeile.trim();

        if zeile.is_empty() || zeile.starts_with('#') {
            continue;
        }

        if let Some(name) = zeile.strip_prefix('[').and_then(|z| z.strip_suffix(']')) {
            abschnitt = name.trim().to_string();
            continue;
        }

        if let Some((schluessel, wert)) = zeile.split_once('=') {
            werte.insert(
                (abschnitt.clone(), schluessel.trim().to_string()),
                ohne_kommentar(wert).to_string(),
            );
        }
    }

    werte
}

/// Einen Kommentar hinter dem Wert abschneiden — aber keine Farbe.
///
/// ## Die Falle, und sie hat beim ersten Lauf zugeschnappt
///
/// Hier stand `wert.split('#').next()`, und das ist in einer Datei voller
/// Farben genau falsch: Aus `"#1a1b26"` wurde `"`. Jede Farbe des Themes
/// wäre unlesbar geworden — und zwar **lautlos**, weil `Farbe::lesen` dann
/// `None` gibt und `Stil::aus_theme` auf die Vorgabe zurückfällt. Das Menü
/// hätte funktioniert und in jedem Theme gleich ausgesehen.
///
/// Zwei Regeln, und beide sind an `shell.toml` abgelesen:
///
/// * Steht der Wert in Anführungszeichen, endet er am schliessenden — was
///   danach kommt, ist Kommentar.
/// * Sonst zählt ein `#` nur als Kommentarbeginn, wenn **Leerraum davor**
///   steht. `#1a1b26` ohne Anführungszeichen bleibt damit heil, und
///   `0.9   # mit Kommentar` verliert seinen Kommentar.
fn ohne_kommentar(wert: &str) -> &str {
    let wert = wert.trim();

    if let Some(rest) = wert.strip_prefix('"') {
        if let Some(ende) = rest.find('"') {
            // Die Anführungszeichen bleiben dran; weg nimmt sie erst der
            // Leser, weil ein Zahlenwert keine hat.
            return &wert[..ende + 2];
        }
    }

    // `false`, nicht `true`: Ein `#` am Anfang des WERTES ist eine Farbe.
    // Eine Kommentarzeile fängt mit `#` an, aber die hat der Leser schon
    // weggelassen, bevor er hierher kommt — hier steht nur, was rechts vom
    // `=` stand.
    let mut vorher_leer = false;

    for (i, c) in wert.char_indices() {
        if c == '#' && vorher_leer {
            return wert[..i].trim_end();
        }

        vorher_leer = c.is_whitespace();
    }

    wert
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die Farbe bleibt heil, der Kommentar fällt weg.
    ///
    /// Diese Probe ist der Grund, warum `ohne_kommentar` eine eigene Funktion
    /// ist: Ein `split('#')` mitten im Leser sah richtig aus und hätte jede
    /// Themefarbe verschluckt.
    #[test]
    fn ein_kommentar_faellt_weg_eine_farbe_nicht() {
        assert_eq!(ohne_kommentar(" \"#111222\" "), "\"#111222\"");
        assert_eq!(ohne_kommentar(" \"#111222\"  # Kommentar"), "\"#111222\"");
        assert_eq!(ohne_kommentar("#111222"), "#111222");
        assert_eq!(ohne_kommentar("#111222 # weg"), "#111222");
        assert_eq!(ohne_kommentar("0.9   # mit Kommentar"), "0.9");
        assert_eq!(ohne_kommentar("1.0"), "1.0");
        assert_eq!(
            ohne_kommentar("\"rgba(1a1b26ff) #aabbcc 45deg\""),
            "\"rgba(1a1b26ff) #aabbcc 45deg\""
        );
    }

    #[test]
    fn farben_in_allen_schreibweisen() {
        assert_eq!(Farbe::lesen("#1a1b26"), Some(Farbe::neu(26, 27, 38, 1.0)));
        assert_eq!(
            Farbe::lesen("  \"#1a1b26\" "),
            Some(Farbe::neu(26, 27, 38, 1.0))
        );
        assert_eq!(Farbe::lesen("#abc"), Some(Farbe::neu(170, 187, 204, 1.0)));
        assert_eq!(
            Farbe::lesen("rgba(1a1b26ff)"),
            Some(Farbe::neu(26, 27, 38, 1.0))
        );

        // Halbdurchsichtig über die letzten zwei Stellen.
        let halb = Farbe::lesen("#1a1b2680").expect("acht Stellen");
        assert!((halb.a - 0.5).abs() < 0.01, "{:?}", halb);

        // Vom Verlauf der erste Ton.
        assert_eq!(
            Farbe::lesen("#7aa2f7 #bb9af7 45deg"),
            Some(Farbe::neu(0x7a, 0xa2, 0xf7, 1.0))
        );

        assert_eq!(Farbe::lesen("hyprland.active-border"), None);
        assert_eq!(Farbe::lesen(""), None);
    }

    #[test]
    fn der_mini_leser_findet_abschnitt_und_schluessel() {
        let datei = std::env::temp_dir().join(format!(
            "ai-brain-tray-probe-{}-{}.toml",
            std::process::id(),
            "stil"
        ));

        std::fs::write(
            &datei,
            "# Kopf\n\
             [popups]\n\
             background       = \"#111222\"\n\
             background-alpha = 0.9   # mit Kommentar\n\
             \n\
             [menu]\n\
             selected-text = \"#333444\"\n",
        )
        .expect("schreiben");

        let werte = lesen_toml(&datei);
        let _ = std::fs::remove_file(&datei);

        assert_eq!(
            werte
                .get(&("popups".into(), "background".into()))
                .map(String::as_str),
            Some("\"#111222\"")
        );
        assert_eq!(
            werte
                .get(&("popups".into(), "background-alpha".into()))
                .map(String::as_str),
            Some("0.9")
        );
        assert_eq!(
            werte
                .get(&("menu".into(), "selected-text".into()))
                .map(String::as_str),
            Some("\"#333444\"")
        );
        // Ein Schlüssel ohne Abschnitt wäre ein Treffer in „popups" — er ist
        // keiner.
        assert!(!werte.contains_key(&("".into(), "background".into())));
    }

    /// Die Vorgabe muss **ohne** Theme vollständig sein.
    ///
    /// Das ist keine Formsache: Auf jedem Rechner, der nicht Omarchy ist,
    /// gibt es das Theme nicht, und ein Fenster mit durchsichtigem
    /// Hintergrund und unsichtbarem Text wäre dort das Ergebnis.
    #[test]
    fn ohne_theme_ist_der_stil_lesbar() {
        let s = Stil::default();

        assert!(s.hintergrund.a > 0.9, "Hintergrund muss deckend sein");
        assert!(s.text.a > 0.9, "Text muss deckend sein");
        assert!(s.schriftgrad >= 8.0 && s.zeilenhoehe > s.schriftgrad);
        // Text und Hintergrund dürfen nicht derselbe Ton sein.
        assert_ne!(
            (s.text.r, s.text.g, s.text.b),
            (s.hintergrund.r, s.hintergrund.g, s.hintergrund.b)
        );
    }
}
