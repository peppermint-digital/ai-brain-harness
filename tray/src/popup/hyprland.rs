//! Was nur Hyprland weiss: Mausstelle, Rundung, Aussenluft, die Leiste.
//!
//! Vier Auskünfte, alle über denselben Sockel, alle **Zusatz** — fehlt eine,
//! fällt der Aufrufer auf eine Vorgabe zurück:
//!
//! | [`zeigerstelle`] | wo der Zeiger steht, ausgabelokal |
//! | [`rundung`] | `decoration:rounding` — Omarchys `Style.cornerRadius` |
//! | [`aussenluft`] | `general:gaps_out` halbiert — Omarchys `Style.gapsOut` |
//! | die Leiste | die Layer-Fläche, auf deren Symbol geklickt wurde ([`Stelle::leiste`]) |
//!
//! Die letzten drei sind seit v0.5.6 dabei, damit das Menü aussieht wie
//! Omarchys eigene Flyouts und an derselben Stelle hängt: unter der Leiste,
//! mit demselben Radius und demselben Abstand.
//!
//! ## Warum das hier stehen muss
//!
//! Ein Wayland-Client kann die Zeigerposition nicht abfragen; er erfährt sie
//! nur aus `wl_pointer.enter` und `motion`. Das ist Absicht in der
//! Spezifikation und für ein Menü an der Maus normalerweise genau richtig:
//! Spannt man eine Fläche unter dem Zeiger auf, schickt der Compositor ein
//! `enter` mit flächenlokalen Koordinaten.
//!
//! **Gemessen am 28.10.2026 auf Hyprland 0.56.2 tut er das nicht.** Das
//! `enter` kommt erst, wenn der Zeiger sich bewegt. Ein Klick aufs
//! Tray-Symbol bewegt ihn aber nicht — er steht ja schon darauf. Ergebnis:
//! Der Rückfall griff jedes Mal, und das Menü erschien in der Ecke statt
//! unter dem Symbol, auf einer Multimonitor-Anordnung sogar auf dem falschen
//! Schirm.
//!
//! Also fragen wir Hyprland selbst. Das ist compositor-eigen und bleibt
//! deshalb **ein Zusatz**: Gibt es die Auskunft nicht, wartet
//! `flaeche::zeigen` weiter auf das `enter` und fällt zuletzt auf die Ecke
//! zurück. Kein Pfad verlässt sich allein hierauf.
//!
//! ## Über den Sockel, nicht über `hyprctl`
//!
//! `hyprctl` ist ein Programm, das genau diesen Sockel anspricht. Es
//! aufzurufen hiesse: einen Prozess starten, auf ihn warten, und davon
//! abhängen, dass er im `PATH` eines Autostart-Eintrags liegt — der ohne die
//! Shell des Nutzers startet. Der Sockel kostet dreissig Zeilen und keine
//! dieser drei Annahmen.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

/// Wo der Zeiger steht: Name der Ausgabe, die Stelle **auf dieser Ausgabe**
/// (logisch) — und die Leiste, in der er steht, wenn er in einer steht.
#[derive(Clone, Debug, PartialEq)]
pub struct Stelle {
    pub ausgabe: String,
    pub x: f64,
    pub y: f64,
    /// Die Layer-Fläche unter dem Zeiger, **ausgabelokal** — bei einem Klick
    /// aufs Tray-Symbol also die Leiste. `None`, wenn dort keine liegt (dann
    /// kam der Aufruf nicht von einem Klick in der Leiste, etwa aus einem
    /// Prüflauf von Hand).
    pub leiste: Option<Flaeche>,
}

/// Fragt Hyprland, wo der Zeiger steht — und was dort liegt.
///
/// `None` heisst: Das ist kein Hyprland, oder es antwortet nicht so, wie wir
/// es lesen können. Beides ist kein Fehler — es gibt noch zwei Wege.
///
/// ## Warum alles in einem Aufruf
///
/// Die Umrechnung „global → ausgabelokal" braucht den Ursprung der Ausgabe,
/// und den liefert `j/monitors`. `hyprctl layers` antwortet in **globalen**
/// logischen Koordinaten, `set_margin` einer Layer-Fläche zählt aber von der
/// Kante ihrer Ausgabe. Beide Umrechnungen mit demselben Ursprung an
/// derselben Stelle zu machen ist der Unterschied zwischen „stimmt immer" und
/// „stimmt auf dem ersten Schirm".
pub fn zeigerstelle() -> Option<Stelle> {
    let stelle = antwort("cursorpos")?;
    let (x, y) = stelle_lesen(&stelle)?;

    let monitore = antwort("j/monitors")?;
    let (ausgabe, mx, my) = ausgabe_finden(&monitore, x, y)?;
    let (ab, ah) = ausgabe_groesse(&monitore, &ausgabe).unwrap_or((0.0, 0.0));

    // Scheitert das, bleibt die Stelle brauchbar — nur die Ausrichtung an
    // der Leiste fällt dann weg.
    let leiste = antwort("j/layers")
        .and_then(|json| flaeche_finden(&json, &ausgabe, x, y, ab, ah))
        .map(|f| Flaeche {
            x: f.x - mx,
            y: f.y - my,
            ..f
        });

    Some(Stelle {
        ausgabe,
        x: x - mx,
        y: y - my,
        leiste,
    })
}

/// Eine Layer-Fläche in **logischen**, ausgabelokalen Massen.
///
/// Gebraucht für die Leiste: Omarchys Popup hängt `target.height + margin`
/// unter seinem Symbol, und „target.height" ist die Höhe der Leiste. Die
/// kennen wir nur so.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flaeche {
    pub x: f64,
    pub y: f64,
    pub breite: f64,
    pub hoehe: f64,
}

impl Flaeche {
    fn enthaelt(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.breite && y >= self.y && y < self.y + self.hoehe
    }
}

/// Hyprlands `decoration:rounding` — der Radius, mit dem Omarchy seine Karten
/// zeichnet (`Style.cornerRadius`).
///
/// Dass dieser Wert **auch 0 sein kann**, ist der Punkt: Auf diesem
/// Prüfrechner ist er 0, Omarchys Flyouts haben also eckige Ecken. Ein
/// eigener Vorgabewert von 10 (so bis v0.5.5) ist dann nicht „auch schön",
/// sondern sichtbar fremd.
pub fn rundung() -> Option<f32> {
    zahl_aus_option(&antwort("j/getoption decoration:rounding")?, "int")
}

/// Hyprlands `general:gaps_out`, **halbiert** — Omarchys `Style.gapsOut`.
///
/// Die Halbierung ist nicht unsere Idee, sie steht in `Commons/Style.qml`:
/// Der Fensterabstand von Hyprland taugt als Fenster-zu-Fenster-Mass, wirkt
/// als Abstand einer Karte zur Bildschirmkante aber zu weit, deshalb nimmt
/// die Shell die Hälfte. Wer hier den ganzen Wert nimmt, hängt sein Menü
/// doppelt so weit unter der Leiste wie jedes andere Flyout.
///
/// `gaps_out` antwortet als CSS-Viererliste („10 10 10 10"); gezählt wird die
/// erste Zahl, wie in `applyGapsOutJson`.
pub fn aussenluft() -> Option<f32> {
    aussenluft_lesen(&antwort("j/getoption general:gaps_out")?)
}

/// Die Rechnung aus [`aussenluft`], getrennt zum Prüfen.
fn aussenluft_lesen(json: &str) -> Option<f32> {
    let wert: serde_json::Value = serde_json::from_str(json).ok()?;

    let erste = wert
        .get("css")
        .and_then(serde_json::Value::as_str)
        .and_then(|css| {
            css.split_whitespace()
                .next()
                .and_then(|z| z.parse::<f32>().ok())
        })
        .or_else(|| {
            wert.get("int")
                .and_then(serde_json::Value::as_f64)
                .map(|v| v as f32)
        })?;

    (erste >= 0.0).then(|| (erste / 2.0).round())
}

/// Eine Zahl aus einer `getoption`-Antwort.
fn zahl_aus_option(json: &str, feld: &str) -> Option<f32> {
    let wert: serde_json::Value = serde_json::from_str(json).ok()?;
    let zahl = wert.get(feld)?.as_f64()?;

    (zahl.is_finite() && zahl >= 0.0).then_some(zahl as f32)
}

/// Die Layer-Fläche auf `ausgabe`, die `(x, y)` enthält — die Leiste, auf
/// deren Symbol gerade geklickt wurde. `(x, y)` und das Ergebnis sind
/// **global** logisch; umgerechnet wird in [`zeigerstelle`].
///
/// Getrennt von [`zeigerstelle`], damit die Suche ohne Compositor prüfbar
/// ist — sie ist der Teil, der falsch sein kann.
///
/// ## Warum über „enthält den Klick" und nicht über den Namen
///
/// Omarchys Leiste heisst `omarchy-bar`, waybars heisst `waybar`, und eine
/// Liste bekannter Namen wäre eine Liste, die altert — jede fremde Leiste
/// fiele durch. Die Frage, die hier zählt, ist eine andere: **Welche Fläche
/// hat den Klick bekommen?** Das Symbol steht in der Leiste, also ist die
/// Fläche unter dem Zeiger die Leiste.
///
/// ## Was eine Leiste von einer Decke unterscheidet — gemessen, nicht gedacht
///
/// „Die oberste Fläche, die den Punkt enthält" allein ist **falsch**, und das
/// hat am 28.10.2026 am laufenden Compositor sofort zugeschlagen: Der Kasten
/// landete bei `y = 1777` statt bei `y = 31`. Auf diesem Schirm liegen vier
/// Layer-Flächen, und drei davon sind bildschirmgross:
///
/// | Fläche | Ebene | Masse |
/// |---|---|---|
/// | `omarchy-background` | 0 | 3072×2048 |
/// | `omarchy-bar` | 2 | 3072×**26** |
/// | `omarchy-notifications` | 3 | 3072×2048 |
/// | `ai-brain-tray-menue-auffaenger` | 3 | 3072×2048 |
///
/// Jede der drei grossen enthält jeden Punkt des Schirms. Die Ebenen zu
/// sortieren hilft nicht: Die Benachrichtigungsfläche liegt **über** der
/// Leiste, und unser eigener Auffänger steht schon, wenn gefragt wird.
/// Herausgekommen ist „die Leiste ist 2048 hoch", und ein Menü „unter einer
/// 2048 hohen Leiste" landet geklemmt am unteren Bildschirmrand.
///
/// Also wird gefragt, was eine Leiste tatsächlich ausmacht: **Sie ist
/// dünn — sie überlässt den Schirm den Fenstern.** Eine Fläche, die in
/// beiden Richtungen mehr als die Hälfte der Ausgabe einnimmt, ist eine
/// Decke (Hintergrund, Benachrichtigungen, Sperrschirm, unser Auffänger) und
/// keine Leiste. Omarchys Leiste nimmt 1,3 % der Höhe ein, eine senkrechte
/// 0,9 % der Breite — der Abstand zur Grenze ist also kein Haaresbreite-Fall,
/// an dem die Regel wackelt.
///
/// Dazu, ausdrücklich: **unsere eigenen Flächen werden übersprungen.** Die
/// Dünn-Regel deckt sie schon ab, aber sich beim Ausrichten auf sich selbst
/// zu beziehen ist der Fehler, der beim nächsten Umbau wiederkommt — der
/// Namensraum ist frei wählbar, genau damit man ihn wiedererkennt.
///
/// Gesucht wird dann von der obersten Ebene nach unten, und innerhalb einer
/// Ebene die kleinste passende Fläche.
///
/// `j/layers` ist nach Ausgabe geschlüsselt und darunter nach Ebene
/// (`levels`), wobei der Schlüssel eine **Zeichenkette** ist („0", „2"). Die
/// Masse sind logisch — dieselben, in denen `set_margin` zählt.
fn flaeche_finden(
    json: &str,
    ausgabe: &str,
    x: f64,
    y: f64,
    ausgabe_breite: f64,
    ausgabe_hoehe: f64,
) -> Option<Flaeche> {
    let wurzel: serde_json::Value = serde_json::from_str(json).ok()?;
    let ebenen = wurzel.get(ausgabe)?.get("levels")?.as_object()?;

    // Nach Ebene absteigend: die oberste passende Fläche hat den Klick
    // bekommen.
    let mut sortiert: Vec<(i64, &serde_json::Value)> = ebenen
        .iter()
        .filter_map(|(k, v)| Some((k.parse::<i64>().ok()?, v)))
        .collect();

    sortiert.sort_by_key(|(ebene, _)| std::cmp::Reverse(*ebene));

    for (_, liste) in sortiert {
        let mut beste: Option<Flaeche> = None;

        for f in liste.as_array()? {
            let namensraum = f.get("namespace").and_then(serde_json::Value::as_str);

            if namensraum.is_some_and(|n| n.starts_with(EIGENER_NAMENSRAUM)) {
                continue;
            }

            let kandidat = Flaeche {
                x: f.get("x")?.as_f64()?,
                y: f.get("y")?.as_f64()?,
                breite: f.get("w")?.as_f64()?,
                hoehe: f.get("h")?.as_f64()?,
            };

            if kandidat.breite <= 0.0 || kandidat.hoehe <= 0.0 || !kandidat.enthaelt(x, y) {
                continue;
            }

            if !ist_leiste(&kandidat, ausgabe_breite, ausgabe_hoehe) {
                continue;
            }

            // Die kleinste passende: Liegen in einer Ebene zwei Leisten
            // übereinander, ist die schmalere gemeint.
            let flaecheninhalt = kandidat.breite * kandidat.hoehe;

            if beste.is_none_or(|b| flaecheninhalt < b.breite * b.hoehe) {
                beste = Some(kandidat);
            }
        }

        if beste.is_some() {
            return beste;
        }
    }

    None
}

/// Der Namensraum, mit dem unsere eigenen Flächen anfangen.
///
/// Steht hier und nicht in `flaeche`, damit dieses Modul nicht von dort
/// abhängt; die beiden Konstanten dort (`NAMENSRAUM_KASTEN`,
/// `NAMENSRAUM_FAENGER`) fangen beide damit an, und eine Probe in `flaeche`
/// hält das fest.
pub const EIGENER_NAMENSRAUM: &str = "ai-brain-tray-menue";

/// Ist diese Fläche dünn genug, um eine Leiste zu sein?
///
/// Die Begründung steht bei [`flaeche_finden`]. Hier nur die Zahl: höchstens
/// die Hälfte der Ausgabe in **einer** der beiden Richtungen. Ohne bekannte
/// Ausgabengrösse wird nicht geraten, sondern zugestimmt — dann ist die
/// Ebenenreihenfolge das Beste, was wir haben.
fn ist_leiste(f: &Flaeche, ausgabe_breite: f64, ausgabe_hoehe: f64) -> bool {
    if ausgabe_breite <= 0.0 || ausgabe_hoehe <= 0.0 {
        return true;
    }

    f.breite <= ausgabe_breite / 2.0 || f.hoehe <= ausgabe_hoehe / 2.0
}

/// Eine Anfrage an den Hyprland-Sockel.
fn antwort(befehl: &str) -> Option<String> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    let kennung = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;

    let pfad = std::path::PathBuf::from(runtime)
        .join("hypr")
        .join(kennung)
        .join(".socket.sock");

    let mut sockel = UnixStream::connect(pfad).ok()?;

    // Die Zeitgrenzen sind kein Beiwerk: Ein Menü, das auf einen hängenden
    // Compositor wartet, geht gar nicht auf. Lieber die Ecke als nichts.
    let frist = std::time::Duration::from_millis(150);
    sockel.set_read_timeout(Some(frist)).ok()?;
    sockel.set_write_timeout(Some(frist)).ok()?;

    sockel.write_all(befehl.as_bytes()).ok()?;
    sockel.flush().ok()?;

    let mut text = String::new();
    sockel.read_to_string(&mut text).ok()?;

    (!text.trim().is_empty()).then_some(text)
}

/// `cursorpos` antwortet mit „1234, 56".
fn stelle_lesen(text: &str) -> Option<(f64, f64)> {
    let (x, y) = text.trim().split_once(',')?;

    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// Die **logische** Grösse einer Ausgabe: `width/scale` × `height/scale`.
///
/// Die Falle steht bei [`ausgabe_finden`]: `width`/`height` sind in
/// Gerätepixeln, `x`/`y` logisch.
fn ausgabe_groesse(json: &str, name: &str) -> Option<(f64, f64)> {
    let monitore: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;

    for m in &monitore {
        if m.get("name")?.as_str()? != name {
            continue;
        }

        let skalierung = m
            .get("scale")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(1.0);

        if skalierung <= 0.0 {
            return None;
        }

        return Some((
            m.get("width")?.as_f64()? / skalierung,
            m.get("height")?.as_f64()? / skalierung,
        ));
    }

    None
}

/// Welche Ausgabe enthält diese Stelle — und wo liegt ihre linke obere Ecke?
///
/// `j/monitors` gibt `x`/`y` **logisch**, `width`/`height` aber in
/// Gerätepixeln; die logische Grösse ist der Quotient mit `scale`. Das zu
/// verwechseln verschiebt auf einem skalierten Schirm alles — bei 1.25 um ein
/// Viertel.
fn ausgabe_finden(json: &str, x: f64, y: f64) -> Option<(String, f64, f64)> {
    let monitore: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;

    for m in &monitore {
        let name = m.get("name")?.as_str()?;
        let mx = m.get("x")?.as_f64()?;
        let my = m.get("y")?.as_f64()?;
        let breite = m.get("width")?.as_f64()?;
        let hoehe = m.get("height")?.as_f64()?;
        let skalierung = m
            .get("scale")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(1.0);

        if skalierung <= 0.0 {
            continue;
        }

        let (lb, lh) = (breite / skalierung, hoehe / skalierung);

        // Nach Hyprlands `transform` wären Breite und Höhe getauscht. Das
        // fragen wir nicht ab: Bei einer gedrehten Ausgabe stimmt dann die
        // Zuordnung eventuell nicht, und dann greift der Weg über
        // `wl_pointer.enter` oder die Ecke. Falsch raten wäre schlechter als
        // nicht zu wissen.
        if x >= mx && x < mx + lb && y >= my && y < my + lh {
            return Some((name.to_string(), mx, my));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursorpos_wird_gelesen() {
        assert_eq!(stelle_lesen("1234, 56"), Some((1234.0, 56.0)));
        assert_eq!(stelle_lesen("  22,11\n"), Some((22.0, 11.0)));
        assert_eq!(stelle_lesen("0, 0"), Some((0.0, 0.0)));

        assert_eq!(stelle_lesen("22"), None);
        assert_eq!(stelle_lesen(""), None);
        assert_eq!(stelle_lesen("links, oben"), None);
    }

    /// Die Anordnung dieses Prüfrechners, wörtlich aus `hyprctl -j monitors`:
    /// HDMI-A-1 mit 3840×2560 bei Skalierung 1.25 (logisch 3072×2048) und
    /// daneben HEADLESS-1.
    ///
    /// Genau hier lag der Fehler, den diese Probe festhält: Mit `width` statt
    /// `width/scale` wäre HDMI-A-1 3840 logisch breit, und jede Stelle auf
    /// HEADLESS-1 (ab 3072) läge noch auf HDMI-A-1.
    const ECHT: &str = r#"[
      {"name":"HDMI-A-1","x":0,"y":0,"width":3840,"height":2560,"scale":1.25},
      {"name":"HEADLESS-1","x":3072,"y":0,"width":1920,"height":1080,"scale":1.25}
    ]"#;

    #[test]
    fn die_ausgabe_unter_dem_zeiger_wird_gefunden() {
        assert_eq!(
            ausgabe_finden(ECHT, 22.0, 11.0),
            Some(("HDMI-A-1".into(), 0.0, 0.0))
        );

        // Knapp vor der Kante von HEADLESS-1 …
        assert_eq!(
            ausgabe_finden(ECHT, 3071.0, 500.0),
            Some(("HDMI-A-1".into(), 0.0, 0.0))
        );

        // … und knapp dahinter. Das ist die Stelle, an der die
        // Skalierungsfalle zuschlägt.
        assert_eq!(
            ausgabe_finden(ECHT, 3072.0, 11.0),
            Some(("HEADLESS-1".into(), 3072.0, 0.0))
        );
        assert_eq!(
            ausgabe_finden(ECHT, 4200.0, 60.0),
            Some(("HEADLESS-1".into(), 3072.0, 0.0))
        );

        // Unter HDMI-A-1 geht es logisch bis 2048, nicht bis 2560.
        assert_eq!(
            ausgabe_finden(ECHT, 500.0, 2047.0),
            Some(("HDMI-A-1".into(), 0.0, 0.0))
        );
        assert_eq!(ausgabe_finden(ECHT, 500.0, 2048.0), None);

        // Neben allem.
        assert_eq!(ausgabe_finden(ECHT, 9000.0, 60.0), None);
        assert_eq!(ausgabe_finden(ECHT, -1.0, 60.0), None);
    }

    #[test]
    fn unlesbare_antworten_ergeben_nichts_statt_irgendwas() {
        assert_eq!(ausgabe_finden("kein json", 0.0, 0.0), None);
        assert_eq!(ausgabe_finden("[]", 0.0, 0.0), None);
        assert_eq!(ausgabe_finden(r#"[{"name":"X"}]"#, 0.0, 0.0), None);
        // Skalierung 0 würde durch Null teilen.
        assert_eq!(
            ausgabe_finden(
                r#"[{"name":"X","x":0,"y":0,"width":100,"height":100,"scale":0}]"#,
                10.0,
                10.0
            ),
            None
        );
    }

    /// `j/layers` dieses Prüfrechners, wörtlich — gekürzt auf die Felder, die
    /// wir lesen, und ergänzt um die zwei Flächen, die am 28.10.2026 den
    /// Fehlgriff ausgelöst haben.
    ///
    /// **Drei** der vier Flächen auf HDMI-A-1 sind bildschirmgross: der
    /// Hintergrund (Ebene 0), die Benachrichtigungsfläche (Ebene 3) und unser
    /// eigener Auffänger (Ebene 3). Nur `omarchy-bar` ist 26 Pixel hoch. Wer
    /// bloss nach Ebenen sortiert, greift die Benachrichtigungsfläche — sie
    /// liegt ÜBER der Leiste.
    const LAYERS: &str = r#"{
      "HDMI-A-1": { "levels": {
        "0": [ {"x":0,"y":0,"w":3072,"h":2048,"namespace":"omarchy-background"} ],
        "2": [ {"x":0,"y":0,"w":3072,"h":26,"namespace":"omarchy-bar"} ],
        "3": [ {"x":0,"y":0,"w":3072,"h":2048,"namespace":"omarchy-notifications"},
               {"x":0,"y":0,"w":3072,"h":2048,"namespace":"ai-brain-tray-menue-auffaenger"} ]
      }},
      "HEADLESS-1": { "levels": {
        "2": [ {"x":3072,"y":0,"w":1536,"h":26,"namespace":"omarchy-bar"} ]
      }}
    }"#;

    /// Die logische Grösse von HDMI-A-1 — 3840×2560 bei Skalierung 1.25.
    const AUSGABE: (f64, f64) = (3072.0, 2048.0);

    fn suchen(ausgabe: &str, x: f64, y: f64) -> Option<Flaeche> {
        flaeche_finden(LAYERS, ausgabe, x, y, AUSGABE.0, AUSGABE.1)
    }

    /// Der Klick in der Leiste findet die Leiste.
    #[test]
    fn der_klick_in_der_leiste_findet_die_leiste() {
        assert_eq!(
            suchen("HDMI-A-1", 2400.0, 13.0),
            Some(Flaeche {
                x: 0.0,
                y: 0.0,
                breite: 3072.0,
                hoehe: 26.0
            })
        );

        // Die Leiste des zweiten Schirms kommt in GLOBALEN Massen — das
        // Umrechnen macht `zeigerstelle`, nicht diese Funktion.
        assert_eq!(
            suchen("HEADLESS-1", 3500.0, 10.0),
            Some(Flaeche {
                x: 3072.0,
                y: 0.0,
                breite: 1536.0,
                hoehe: 26.0
            })
        );

        // Eine Ausgabe, die es nicht gibt, und eine Stelle neben allem.
        assert_eq!(suchen("DP-9", 10.0, 10.0), None);
        assert_eq!(suchen("HEADLESS-1", 10.0, 10.0), None);
        assert_eq!(
            flaeche_finden("kein json", "HDMI-A-1", 0.0, 0.0, 3072.0, 2048.0),
            None
        );
        assert_eq!(
            flaeche_finden("{}", "HDMI-A-1", 0.0, 0.0, 3072.0, 2048.0),
            None
        );
    }

    /// **Der Befund vom 28.10.2026.** Ein Klick MITTEN auf dem Schirm findet
    /// keine Leiste — weder den Hintergrund noch die Benachrichtigungsfläche
    /// noch unseren eigenen Auffänger.
    ///
    /// Vorher kam hier eine „Leiste" von 3072×2048 heraus, und der Kasten
    /// landete bei `y = 1777` statt bei `y = 31`: Er hing unter einer 2048
    /// Pixel hohen Leiste und wurde an den unteren Bildschirmrand geklemmt.
    /// Gemessen am laufenden Hyprland, nicht überlegt.
    #[test]
    fn eine_bildschirmgrosse_decke_ist_keine_leiste() {
        assert_eq!(
            suchen("HDMI-A-1", 1920.0, 1280.0),
            None,
            "Hintergrund, Benachrichtigungen und der eigene Auffänger sind \
             keine Leisten"
        );

        // Auch direkt unter der Leiste, wo alle drei Decken liegen.
        assert_eq!(suchen("HDMI-A-1", 2400.0, 400.0), None);
    }

    /// Die eigenen Flächen werden übersprungen — auch wenn sie dünn wären.
    #[test]
    fn die_eigenen_flaechen_zaehlen_nicht_als_leiste() {
        let json = r#"{"M": { "levels": { "3": [
          {"x":0,"y":0,"w":3000,"h":26,"namespace":"ai-brain-tray-menue"},
          {"x":0,"y":0,"w":3000,"h":26,"namespace":"ai-brain-tray-menue-auffaenger"}
        ]}}}"#;

        assert_eq!(flaeche_finden(json, "M", 100.0, 10.0, 3000.0, 2000.0), None);
    }

    /// Liegen in derselben Ebene zwei Leisten übereinander, gewinnt die
    /// schmalere.
    #[test]
    fn in_einer_ebene_gewinnt_die_kleinere_flaeche() {
        let json = r#"{"M": { "levels": { "2": [
          {"x":0,"y":0,"w":3000,"h":200,"namespace":"dick"},
          {"x":0,"y":0,"w":3000,"h":26,"namespace":"leiste"}
        ]}}}"#;

        let f = flaeche_finden(json, "M", 100.0, 10.0, 3000.0, 2000.0).expect("Treffer");

        assert_eq!(f.hoehe, 26.0);
    }

    /// Eine senkrechte Leiste ist dünn in der Breite.
    #[test]
    fn eine_senkrechte_leiste_gilt_auch_als_leiste() {
        let json = r#"{"M": { "levels": { "2": [
          {"x":0,"y":0,"w":28,"h":2000,"namespace":"omarchy-bar"}
        ]}}}"#;

        let f = flaeche_finden(json, "M", 14.0, 1000.0, 3000.0, 2000.0).expect("Treffer");

        assert_eq!(f.breite, 28.0);
    }

    /// Die logische Grösse einer Ausgabe ist `width/scale`, nicht `width`.
    #[test]
    fn die_ausgabengroesse_ist_logisch() {
        assert_eq!(ausgabe_groesse(ECHT, "HDMI-A-1"), Some((3072.0, 2048.0)));
        assert_eq!(ausgabe_groesse(ECHT, "HEADLESS-1"), Some((1536.0, 864.0)));
        assert_eq!(ausgabe_groesse(ECHT, "DP-9"), None);
        assert_eq!(ausgabe_groesse("kein json", "HDMI-A-1"), None);
    }

    /// `decoration:rounding` wird gelesen — **auch die 0**.
    ///
    /// Das ist keine Spitzfindigkeit: Auf diesem Rechner ist der Wert 0, und
    /// ein Leser, der 0 für „nicht gesetzt" nimmt, lässt genau dort die
    /// Vorgabe stehen, wo Omarchy eckig zeichnet.
    #[test]
    fn die_rundung_wird_gelesen_auch_die_null() {
        assert_eq!(
            zahl_aus_option(
                r#"{"option":"decoration:rounding","int":0,"set":true}"#,
                "int"
            ),
            Some(0.0)
        );
        assert_eq!(
            zahl_aus_option(
                r#"{"option":"decoration:rounding","int":12,"set":true}"#,
                "int"
            ),
            Some(12.0)
        );
        assert_eq!(zahl_aus_option(r#"{"int":-3}"#, "int"), None);
        assert_eq!(zahl_aus_option(r#"{"float":3.0}"#, "int"), None);
        assert_eq!(zahl_aus_option("kein json", "int"), None);
    }

    /// `gaps_out` wird halbiert — wie `Style.applyGapsOutJson`.
    #[test]
    fn die_aussenluft_ist_die_haelfte_der_fensterluft() {
        // Der Wert dieses Rechners: 10 → Style.gapsOut = 5.
        assert_eq!(
            aussenluft_lesen(r#"{"option":"general:gaps_out","css":"10 10 10 10","set":true}"#),
            Some(5.0)
        );
        // Vier verschiedene Werte: die erste Zahl zaehlt.
        assert_eq!(aussenluft_lesen(r#"{"css":"8 20 20 20"}"#), Some(4.0));
        // Ohne `css` der `int`.
        assert_eq!(aussenluft_lesen(r#"{"int":6}"#), Some(3.0));
        // Ungerade halbiert sich auf eine ganze Zahl, wie Math.round.
        assert_eq!(aussenluft_lesen(r#"{"css":"5 5 5 5"}"#), Some(3.0));
        assert_eq!(aussenluft_lesen(r#"{"css":"0 0 0 0"}"#), Some(0.0));

        assert_eq!(aussenluft_lesen(r#"{"css":"-4 0 0 0"}"#), None);
        assert_eq!(aussenluft_lesen("{}"), None);
        assert_eq!(aussenluft_lesen("kein json"), None);
    }

    /// Die Stelle wird auf die Ausgabe umgerechnet, nicht global geliefert.
    ///
    /// Das ist der ganze Zweck: `set_margin` einer Layer-Fläche zählt von der
    /// Kante ihrer Ausgabe, nicht von der Ecke der Anordnung. Ein globaler
    /// Wert landet auf dem ersten Schirm und wird dort an den Rand geklemmt —
    /// genau der Befund vom 28.10.2026.
    #[test]
    fn die_stelle_ist_ausgabelokal() {
        let (ausgabe, mx, my) = ausgabe_finden(ECHT, 4200.0, 60.0).expect("Ausgabe");

        assert_eq!(ausgabe, "HEADLESS-1");
        assert_eq!((4200.0 - mx, 60.0 - my), (1128.0, 60.0));
    }
}
