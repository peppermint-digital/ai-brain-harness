//! Die Farben und Masse des Menüfensters — aus dem Theme, nicht erfunden.
//!
//! ## Woher sie kommen
//!
//! Omarchy legt das aktuelle Theme unter
//! `~/.local/state/omarchy/current/theme/` ab (ein Verweis auf das gewählte
//! Theme). Drei Dateien zählen hier:
//!
//! | Datei | Was |
//! |---|---|
//! | `<theme>/shell.toml` | die **Oberflächen**: `[bar]`, `[popups]`, `[controls]`, `[font]`, `[spacing]`, `[hyprland]` |
//! | `<theme>/colors.toml` | die Grundtöne, auf die `shell.toml` zurückgeht |
//! | `~/.config/omarchy/shell.toml` | die **Nutzer-Überlage**, die jeden Themewechsel überlebt |
//!
//! Die Überlage ist nicht Beiwerk: `omarchy display text size` schreibt
//! genau dorthin, und zwar `[font] base-size`. Daran hängt in Omarchy die
//! **ganze** Grössenrechnung — Schriftstufen *und* Abstände (`scale-with-font`).
//! Wer nur die Themedatei liest, zeichnet auf einem Rechner mit vergrösserter
//! Schrift ein Menü in Normalgrösse neben eine vergrösserte Leiste. Gelesen
//! wird deshalb erst das Theme, dann die Überlage darüber — dieselbe
//! Reihenfolge wie in `Commons/Color.qml` (`mergeShell`).
//!
//! ## Welche Zahlen Omarchys Tray-Menü wirklich benutzt
//!
//! Abgelesen an `Ui/PopupCard.qml` und `plugins/bar/widgets/Tray.qml`
//! (Omarchy 4.0.4-1, quickshell 0.3.1):
//!
//! | Stelle | Omarchy | hier |
//! |---|---|---|
//! | Polster der Karte | `Style.space(8)` | [`Stil::polster`] |
//! | Rahmen | `foreground` @ 0.45, `max(1, space(2))` | [`Stil::rahmen`], [`Stil::rahmenbreite`] |
//! | Radius | `Style.cornerRadius` = Hyprlands `decoration:rounding` | [`Stil::radius`] |
//! | Zeile | `Style.space(30)` | [`Stil::zeilenhoehe`] |
//! | Hover | `hoverFillFor(fg, fg)`, Radius `max(2, radius)` | [`Stil::auswahl`], [`Stil::hover_radius`] |
//! | Trenner | Zeile `space(11)`, 1 px, Einzug `space(10)`, `popups.border` × 0.45 | [`Stil::trennerhoehe`] … |
//! | Häkchenspalte | `space(22)`, Text ab `space(28)` | [`Stil::haken_spalte`], [`Stil::text_einzug`] |
//! | Schrift | `Style.font.bodySmall` = `round(base-size × 0.917)` | [`Stil::schriftgrad`] |
//! | Ausgegraut | ganze Zeile `opacity: 0.45` | [`Stil::matt`] |
//! | Soll-Breite | `space(232)` | [`Stil::breite_soll`] |
//!
//! **Wichtig und auf dem ersten Blick überraschend:** Das Tray-Menü setzt
//! `padding` und die Zeilenhöhe als *Literale* (`Style.space(8)`,
//! `Style.space(30)`) und nicht über die Token `[spacing] popup-padding`
//! bzw. `popup-row-height`. Ein Theme, das diese Token verstellt, ändert
//! Omarchys Tray-Menü also **nicht** — und darf unseres deshalb auch nicht
//! ändern. Hier stand bis v0.5.5 genau das: `popup-row-height` wurde gelesen
//! und hätte das Menü von der Leiste wegbewegt, an der es hängen soll.
//! Was durchkommt, ist `[spacing] scale` und `[font] base-size` — über
//! `space()` gehen beide in jede dieser Zahlen ein.
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
//!
//! ## Ein Themewechsel kommt von selbst mit
//!
//! Hier wacht nichts über eine Datei. Das Menüfenster ist ein eigener
//! Prozess, der bei jedem Klick neu startet und dabei [`Stil::aus_theme`]
//! aufruft: Es gibt keinen Zustand, der veralten könnte. `omarchy theme set`
//! tauscht den Inhalt von `current/theme` (der Pfad bleibt, siehe Wiki
//! *Omarchy: eigene Apps dynamisch ans Theme koppeln*) — das nächste Öffnen
//! liest das Neue. Ein Wächter wäre Aufwand für einen Fall, den die Bauform
//! schon abdeckt.

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
    ///
    /// **Ersetzt** die Deckung, multipliziert sie nicht — genau wie
    /// `Util.alpha` in `Commons/Util.qml`, aus dem jedes `*-alpha` des Themes
    /// kommt. Wer hier multipliziert, bekommt bei einem Theme mit
    /// halbdurchsichtiger Vorgabe eine zweite Abschwächung obendrauf.
    pub fn mit_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Dieselbe Farbe, um `faktor` blasser — für das, was Omarchy mit
    /// `opacity` auf einer ganzen Zeile macht.
    pub fn blasser(self, faktor: f32) -> Self {
        Self {
            a: (self.a * faktor).clamp(0.0, 1.0),
            ..self
        }
    }

    /// `#rrggbb`, `#rgb` oder `#rrggbbaa`.
    ///
    /// Auch `rgba(1a1b26ff)` — so schreibt Hyprland Farben, und `shell.toml`
    /// erlaubt diese Schreibweise für Rahmen samt Verlauf. Vom Verlauf nehmen
    /// wir den ersten Ton: Ein Verlauf auf einem 2-Pixel-Rahmen ist nicht der
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

/// Alles, was das Fenster zum Zeichnen braucht. Alle Masse **logisch**.
#[derive(Clone, Debug)]
pub struct Stil {
    pub hintergrund: Farbe,
    pub rahmen: Farbe,
    pub text: Farbe,
    /// Die Infozeilen oben (Verbindung, Leitung, Lage) — unsere Zugabe, in
    /// Omarchys Tray-Menü gibt es sie nicht.
    pub text_matt: Farbe,
    pub trenner: Farbe,
    /// Die Fläche unter dem Punkt, auf dem die Maus steht.
    pub auswahl: Farbe,
    /// Schriftgrad = `Style.font.bodySmall`.
    pub schriftgrad: f32,
    /// Höhe einer klickbaren Zeile = `Style.space(30)`.
    pub zeilenhoehe: f32,
    /// Höhe einer Infozeile — siehe [`Stil::text_matt`].
    pub infohoehe: f32,
    /// Höhe der Zeile, in deren Mitte der Trennstrich sitzt = `space(11)`.
    pub trennerhoehe: f32,
    /// Einzug des Trennstrichs vom Inhaltsrand = `space(10)`.
    pub trenner_einzug: f32,
    /// Luft zwischen Rahmen und Inhalt = `space(8)`.
    pub polster: f32,
    /// Breite der Spalte links vom Text, in der das Häkchen sitzt = `space(22)`.
    pub haken_spalte: f32,
    /// Wo der Text beginnt, vom Inhaltsrand = `space(28)`.
    pub text_einzug: f32,
    pub rahmenbreite: f32,
    /// Radius der Karte = Hyprlands `decoration:rounding`.
    pub radius: f32,
    /// Radius der Hover-Fläche = `max(2, radius)`.
    pub hover_radius: f32,
    /// Die Breite, die Omarchys Tray-Menü hat = `space(232)`.
    ///
    /// Omarchys Höhendeckel (`space(420)`) steht hier bewusst **nicht**: Er
    /// ist dort nötig, weil ein fremdes Programm ein Menü mit fünfzig
    /// Punkten anmelden kann, und dafür hat die Karte einen Rollbalken.
    /// Unsere Liste ist bekannt und knapp dreihundert Pixel hoch; ein Deckel
    /// ohne Rollbalken würde Punkte abschneiden, statt sie erreichbar zu
    /// halten.
    pub breite_soll: f32,
    /// Abstand zur Leiste und zum Bildschirmrand = Hyprlands
    /// `general:gaps_out` halbiert (`Style.gapsOut`).
    pub rand: f32,
    /// Deckung einer ausgegrauten Zeile.
    pub matt: f32,
}

impl Default for Stil {
    /// Die Vorgabe, wenn kein Theme da ist.
    ///
    /// Bewusst nicht Schwarz auf Weiss: Der Tray hängt in einer Leiste, und
    /// Leisten sind in der überwiegenden Mehrheit dunkel. Ein helles Fenster
    /// auf einem dunklen Schirm ist der unangenehmere Fehlgriff — das war
    /// schon beim Symbol der Punkt („dunkel auf dunkel", siehe `symbol.rs`).
    ///
    /// Die Masse sind Omarchys Zahlen bei `base-size = 12` und
    /// `scale = 1.0`, also das, was `space(px)` dort ohne Theme ergibt.
    fn default() -> Self {
        let fg = Farbe::neu(0xa9, 0xb1, 0xd6, 1.0);

        Self {
            hintergrund: Farbe::neu(0x1a, 0x1b, 0x26, 1.0),
            rahmen: fg.mit_alpha(RAHMEN_ALPHA),
            text: fg,
            text_matt: fg.mit_alpha(0.65),
            trenner: Farbe::neu(0x7a, 0xa2, 0xf7, 1.0).mit_alpha(TRENNER_ALPHA),
            auswahl: fg.mit_alpha(0.08),
            schriftgrad: 11.0,
            zeilenhoehe: 30.0,
            infohoehe: 22.0,
            trennerhoehe: 11.0,
            trenner_einzug: 10.0,
            polster: 8.0,
            haken_spalte: 22.0,
            text_einzug: 28.0,
            rahmenbreite: 2.0,
            radius: 0.0,
            hover_radius: 2.0,
            breite_soll: 232.0,
            rand: 5.0,
            matt: MATT,
        }
    }
}

/// Die Deckung, mit der Omarchys Tray-Menü seinen Rahmen zeichnet.
///
/// Steht wörtlich in `Tray.qml`:
/// `borderColor: Qt.rgba(fg.r, fg.g, fg.b, 0.45)`. Das ist **nicht**
/// `Color.popups.border` — das Tray-Menü überschreibt es ausdrücklich. Bis
/// v0.5.5 nahmen wir `popups.border` (im Tokyo-Night-Theme ein deckendes
/// Blau) und hatten damit einen Rahmen, den kein anderes Flyout der Leiste
/// hat.
const RAHMEN_ALPHA: f32 = 0.45;

/// Mit dieser Deckung liegt der Trennstrich auf — `opacity: 0.45` auf einem
/// Rechteck in `Color.popups.border`.
const TRENNER_ALPHA: f32 = 0.45;

/// `opacity: 0.45` auf einer ganzen ausgegrauten Zeile.
const MATT: f32 = 0.45;

impl Stil {
    /// Den Stil aus dem laufenden Theme lesen.
    ///
    /// Scheitert nie: Was fehlt, bleibt auf der Vorgabe. Ein Menü, das wegen
    /// einer unleserlichen Themedatei gar nicht aufgeht, wäre der schlechtere
    /// Tausch — es geht hier um Farben.
    pub fn aus_theme() -> Self {
        let werte = Themewerte::lesen();
        let mut stil = Self::aus_werten(&werte);

        // Radius und Rand kommen nicht aus dem Theme, sondern von Hyprland —
        // genau wie bei Omarchy (`Style.cornerRadius` aus
        // `decoration:rounding`, `Style.gapsOut` aus `general:gaps_out`,
        // halbiert). Ein Theme kann Hyprlands Rundung mitbringen, gefragt ist
        // aber der laufende Wert: `omarchy toggle window-gaps` ändert ihn
        // ohne Themewechsel.
        if let Some(r) = super::hyprland::rundung() {
            stil.radius = r.clamp(0.0, 64.0);
            stil.hover_radius = stil.radius.max(2.0);
        }

        if let Some(l) = super::hyprland::aussenluft() {
            stil.rand = l.clamp(0.0, 64.0);
        }

        stil
    }

    /// Derselbe Stil, aber aus einem fertigen Wertesatz — damit die Rechnung
    /// prüfbar ist, ohne eine Themedatei auf der Platte zu brauchen.
    fn aus_werten(werte: &Themewerte) -> Self {
        let mut stil = Self::default();

        // ## Die Grössenrechnung, Zeile für Zeile aus `Commons/Style.qml`
        //
        //   fontScale  = base-size / 12
        //   eff        = [spacing] scale × (scale-with-font ? fontScale : 1)
        //   space(px)  = max(1, round(px × eff))
        //   fontPx(m)  = max(1, round(base-size × m))
        //
        // `base-size` ist die eine Zahl, an der alles hängt. Nur ein Boden
        // von 1 Pixel, keine Obergrenze: Omarchy hat bewusst keine („if the
        // shell gets ridiculous, that's their call"), und ein Deckel hier
        // hiesse ein Menü in Normalgrösse neben einer riesigen Leiste.
        let basis = werte
            .zahl("font", "base-size")
            .filter(|v| v.is_finite())
            .map(|v| v.max(1.0))
            .unwrap_or(12.0);

        let schrift_faktor = basis / 12.0;

        let mit_schrift = werte.wahrheit("spacing", "scale-with-font").unwrap_or(true);

        let abstand_faktor = werte
            .zahl("spacing", "scale")
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(1.0)
            * if mit_schrift { schrift_faktor } else { 1.0 };

        let space = |px: f32| (px * abstand_faktor).round().max(1.0);
        let font_px = |faktor: f32| (basis * faktor).round().max(1.0);

        stil.schriftgrad = werte
            .zahl("font", "body-small")
            .filter(|v| v.is_finite() && *v > 0.0)
            .map(f32::round)
            .unwrap_or_else(|| font_px(0.917));

        stil.zeilenhoehe = space(30.0);
        stil.infohoehe = space(22.0);
        stil.trennerhoehe = space(11.0);
        stil.trenner_einzug = space(10.0);
        stil.polster = space(8.0);
        stil.haken_spalte = space(22.0);
        stil.text_einzug = space(28.0);
        stil.rahmenbreite = space(2.0).max(1.0);
        stil.breite_soll = space(232.0);

        // ## Die Farben
        //
        // Der Text des Menüs ist in Omarchy der **Leisten**-Vordergrund
        // (`Tray.qml`: `root.foreground` → `bar.foreground` →
        // `Color.bar.text`), nicht `popups.text`. In den mitgelieferten
        // Themes ist beides derselbe Ton; ein Theme darf sie aber trennen,
        // und dann gehört unser Menü zur Leiste, aus der es aufklappt.
        let vordergrund = werte
            .farbe("bar", "text")
            .or_else(|| werte.farbe("popups", "text"))
            .unwrap_or(stil.text);

        stil.text = vordergrund;
        stil.text_matt = vordergrund.mit_alpha(0.65);
        stil.rahmen = vordergrund.mit_alpha(RAHMEN_ALPHA);

        if let Some(f) = werte.farbe("popups", "background") {
            stil.hintergrund = f.mit_alpha(werte.zahl("popups", "background-alpha").unwrap_or(1.0));
        }

        // Der Trennstrich ist `Color.popups.border` mit 0.45 — und
        // `popups.border` ist seinerseits ein Verweis („hyprland.active-border")
        // samt eigenem Alpha. Die Deckung des Themes fällt dabei weg, weil
        // `opacity` in Qt auf dem gezeichneten Rechteck liegt; wir setzen sie
        // deshalb direkt.
        if let Some(f) = werte.farbe("popups", "border") {
            stil.trenner = f.mit_alpha(TRENNER_ALPHA);
        }

        // Die Hover-Fläche: `Style.hoverFillFor(fg, fg)` — Farbe aus
        // `[controls] hover-cursor-color` (Vorgabe: der Vordergrund), Deckung
        // aus `[controls] hover-cursor-fill-alpha` (Vorgabe 0.08). Das ist
        // dasselbe Token, das jeder Knopf der Leiste beim Überfahren nimmt;
        // bis v0.5.5 stand hier `menu.selected-background`, also die Farbe
        // der *Auswahl* im Startmenü — ein anderer Zustand.
        stil.auswahl = werte
            .farbe("controls", "hover-cursor-color")
            .or_else(|| werte.farbe("style", "hover-cursor-color"))
            .unwrap_or(vordergrund)
            .mit_alpha(
                werte
                    .zahl("controls", "hover-cursor-fill-alpha")
                    .or_else(|| werte.zahl("style", "hover-cursor-fill-alpha"))
                    .unwrap_or(0.08)
                    .clamp(0.0, 1.0),
            );

        stil
    }
}

/// Die Werte aus `shell.toml`, Theme und Nutzer-Überlage zusammengelegt.
struct Themewerte {
    werte: HashMap<(String, String), String>,
}

impl Themewerte {
    /// Erst das Theme, dann `~/.config/omarchy/shell.toml` darüber.
    ///
    /// Die Reihenfolge ist die aus `Color.mergeShell()`: Nutzerschlüssel
    /// gewinnen, damit eine Einstellung wie die Textgrösse einen
    /// Themewechsel überlebt.
    fn lesen() -> Self {
        let mut werte = HashMap::new();

        if let Some(ordner) = theme_ordner() {
            werte.extend(lesen_toml(&ordner.join("shell.toml")));
        }

        if let Some(datei) = nutzer_shell() {
            werte.extend(lesen_toml(&datei));
        }

        Self { werte }
    }

    #[cfg(test)]
    fn aus_text(theme: &str, nutzer: &str) -> Self {
        let mut werte = aus_text_lesen(theme);
        werte.extend(aus_text_lesen(nutzer));

        Self { werte }
    }

    /// Den rohen Wert holen und einen Verweis einmal auflösen.
    ///
    /// `shell.toml` verweist mit „hyprland.active-border" auf einen anderen
    /// Schlüssel in derselben Datei. Einmal auflösen reicht; eine Kette gibt
    /// es dort nicht, und eine Schleife soll es nicht geben.
    fn roh(&self, abschnitt: &str, schluessel: &str) -> Option<String> {
        let wert = self
            .werte
            .get(&(abschnitt.to_string(), schluessel.to_string()))?;
        let roh = wert.trim().trim_matches('"').to_string();

        if roh.starts_with('#') || roh.starts_with("rgb") {
            return Some(roh);
        }

        if let Some((a, s)) = roh.split_once('.') {
            if let Some(ziel) = self.werte.get(&(a.to_string(), s.to_string())) {
                return Some(ziel.trim().trim_matches('"').to_string());
            }
        }

        Some(roh)
    }

    fn zahl(&self, abschnitt: &str, schluessel: &str) -> Option<f32> {
        self.roh(abschnitt, schluessel)?.parse::<f32>().ok()
    }

    fn farbe(&self, abschnitt: &str, schluessel: &str) -> Option<Farbe> {
        Farbe::lesen(&self.roh(abschnitt, schluessel)?)
    }

    /// `true`/`1`/`yes`/`on` und ihre Gegenstücke — wie `Style.boolToken`.
    fn wahrheit(&self, abschnitt: &str, schluessel: &str) -> Option<bool> {
        match self
            .roh(abschnitt, schluessel)?
            .trim()
            .to_lowercase()
            .as_str()
        {
            "true" | "1" | "yes" | "on" => Some(true),
            "false" | "0" | "no" | "off" => Some(false),
            _ => None,
        }
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

/// `~/.config/omarchy/shell.toml` — die Überlage, die Themewechsel überlebt.
fn nutzer_shell() -> Option<std::path::PathBuf> {
    let basis = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| Some(std::path::PathBuf::from(std::env::var_os("HOME")?).join(".config")))?;

    let datei = basis.join("omarchy").join("shell.toml");

    datei.is_file().then_some(datei)
}

/// `[abschnitt]` plus `schluessel = wert`, nach `(abschnitt, schluessel)`.
///
/// Kommentare (`#`) und leere Zeilen fallen weg, Werte behalten ihre
/// Anführungszeichen — die nimmt erst der Leser weg, weil ein Zahlenwert
/// keine hat.
fn lesen_toml(pfad: &std::path::Path) -> HashMap<(String, String), String> {
    let Ok(inhalt) = std::fs::read_to_string(pfad) else {
        return HashMap::new();
    };

    aus_text_lesen(&inhalt)
}

fn aus_text_lesen(inhalt: &str) -> HashMap<(String, String), String> {
    let mut werte = HashMap::new();
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

    /// Das mitgelieferte Omarchy-Theme ergibt genau die Zahlen, die in
    /// `Tray.qml` stehen.
    ///
    /// Das ist der eigentliche Nachweis dieses Moduls: Nicht „irgendein Wert
    /// kommt an", sondern **dieselben** Werte wie bei quickshell. Die Zahlen
    /// rechts sind aus `Ui/PopupCard.qml` und `plugins/bar/widgets/Tray.qml`
    /// abgelesen, bei `base-size = 12` und `scale = 1.0`.
    #[test]
    fn das_omarchy_theme_ergibt_omarchys_masse() {
        let theme = "\
            [bar]\n\
            text = \"#a9b1d6\"\n\
            [popups]\n\
            background = \"#1a1b26\"\n\
            background-alpha = 1.0\n\
            border = \"hyprland.active-border\"\n\
            border-alpha = 1.0\n\
            [hyprland]\n\
            active-border = \"#7aa2f7\"\n\
            [controls]\n\
            hover-cursor-color = \"#a9b1d6\"\n\
            hover-cursor-fill-alpha = 0.08\n\
            [spacing]\n\
            scale = 1.0\n\
            scale-with-font = true\n\
            [font]\n\
            base-size = 12\n";

        let s = Stil::aus_werten(&Themewerte::aus_text(theme, ""));

        // Masse: Style.space(px) bei eff = 1.0, und bodySmall = 12 × 0.917.
        assert_eq!(s.schriftgrad, 11.0, "font.bodySmall");
        assert_eq!(s.zeilenhoehe, 30.0, "space(30)");
        assert_eq!(s.trennerhoehe, 11.0, "space(11)");
        assert_eq!(s.trenner_einzug, 10.0, "space(10)");
        assert_eq!(s.polster, 8.0, "space(8)");
        assert_eq!(s.haken_spalte, 22.0, "space(22)");
        assert_eq!(s.text_einzug, 28.0, "space(28)");
        assert_eq!(s.rahmenbreite, 2.0, "max(1, space(2))");
        assert_eq!(s.breite_soll, 232.0, "space(232)");

        // Farben.
        assert_eq!(s.text, Farbe::neu(0xa9, 0xb1, 0xd6, 1.0), "bar.text");
        assert_eq!(
            s.rahmen,
            Farbe::neu(0xa9, 0xb1, 0xd6, 0.45),
            "Rahmen ist der Vordergrund bei 0.45, NICHT popups.border"
        );
        assert_eq!(
            s.trenner,
            Farbe::neu(0x7a, 0xa2, 0xf7, 0.45),
            "Trenner ist popups.border (hier der Verweis) bei 0.45"
        );
        assert_eq!(
            s.auswahl,
            Farbe::neu(0xa9, 0xb1, 0xd6, 0.08),
            "Hover ist hover-cursor-color bei hover-cursor-fill-alpha"
        );
        assert_eq!(s.hintergrund, Farbe::neu(0x1a, 0x1b, 0x26, 1.0));
    }

    /// Eine grössere Textgrösse zieht Schrift **und** Abstände mit.
    ///
    /// Das ist der Fall, den v0.5.5 verpasst hat: `omarchy display text size`
    /// schreibt `[font] base-size` in die NUTZER-Datei, nicht ins Theme. Wer
    /// nur das Theme liest, zeichnet ein Menü in Normalgrösse an eine
    /// vergrösserte Leiste.
    #[test]
    fn die_nutzer_ueberlage_gewinnt_und_skaliert_mit() {
        let theme = "[font]\nbase-size = 12\n[spacing]\nscale = 1.0\nscale-with-font = true\n";
        let nutzer = "[font]\nbase-size = 18\n";

        let s = Stil::aus_werten(&Themewerte::aus_text(theme, nutzer));

        // 18/12 = 1.5.
        assert_eq!(s.schriftgrad, 17.0, "round(18 × 0.917)");
        assert_eq!(s.zeilenhoehe, 45.0, "round(30 × 1.5)");
        assert_eq!(s.polster, 12.0, "round(8 × 1.5)");
        assert_eq!(s.haken_spalte, 33.0, "round(22 × 1.5)");
        assert_eq!(s.rahmenbreite, 3.0, "round(2 × 1.5)");
    }

    /// `scale-with-font = false` trennt die beiden Faktoren wieder.
    #[test]
    fn abstaende_koennen_sich_von_der_schrift_loesen() {
        let theme = "\
            [font]\nbase-size = 24\n\
            [spacing]\nscale = 1.0\nscale-with-font = false\n";

        let s = Stil::aus_werten(&Themewerte::aus_text(theme, ""));

        assert_eq!(s.schriftgrad, 22.0, "die Schrift wächst");
        assert_eq!(s.zeilenhoehe, 30.0, "die Abstände nicht");
    }

    /// Ein Theme, das `[spacing] popup-row-height` setzt, darf unser Menü
    /// NICHT verstellen — Omarchys Tray-Menü setzt seine Zeilenhöhe als
    /// Literal und liest das Token nicht.
    ///
    /// Bis v0.5.5 las `aus_theme` genau dieses Token. Das sah nach Treue zum
    /// Theme aus und war das Gegenteil: eine Zeilenhöhe, die bei Omarchys
    /// Dropdowns ankommt, bei seinem Tray-Menü aber nicht.
    #[test]
    fn ein_popup_row_height_im_theme_bleibt_ohne_wirkung() {
        let theme = "\
            [font]\nbase-size = 12\n\
            [spacing]\nscale = 1.0\npopup-row-height = 44\npopup-padding = 20\n";

        let s = Stil::aus_werten(&Themewerte::aus_text(theme, ""));

        assert_eq!(s.zeilenhoehe, 30.0);
        assert_eq!(s.polster, 8.0);
    }

    /// Ein gesetztes `[font] body-small` schlägt die abgeleitete Stufe.
    #[test]
    fn eine_gepinnte_schriftstufe_gewinnt() {
        let theme = "[font]\nbase-size = 12\nbody-small = 15\n";

        let s = Stil::aus_werten(&Themewerte::aus_text(theme, ""));

        assert_eq!(s.schriftgrad, 15.0);
    }
}
