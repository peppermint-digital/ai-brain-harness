//! Die Mausstelle von Hyprland erfragen — weil Wayland sie nicht hergibt.
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

/// Wo der Zeiger steht: Name der Ausgabe und die Stelle **auf dieser
/// Ausgabe**, logisch.
#[derive(Clone, Debug, PartialEq)]
pub struct Stelle {
    pub ausgabe: String,
    pub x: f64,
    pub y: f64,
}

/// Fragt Hyprland, wo der Zeiger steht.
///
/// `None` heisst: Das ist kein Hyprland, oder es antwortet nicht so, wie wir
/// es lesen können. Beides ist kein Fehler — es gibt noch zwei Wege.
pub fn zeigerstelle() -> Option<Stelle> {
    let stelle = antwort("cursorpos")?;
    let (x, y) = stelle_lesen(&stelle)?;

    let monitore = antwort("j/monitors")?;

    ausgabe_finden(&monitore, x, y).map(|(ausgabe, mx, my)| Stelle {
        ausgabe,
        x: x - mx,
        y: y - my,
    })
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
