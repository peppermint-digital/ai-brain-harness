//! Das eigene Menüfenster des Trays — und der Prozess, der es hält.
//!
//! ## Warum ein eigener Prozess und kein Faden (#9008)
//!
//! Der Tray startet sich selbst noch einmal: `ai-brain-tray --menue`. Die
//! Einträge gehen als JSON auf `stdin` hinein, die gewählte Aktion kommt als
//! JSON auf `stdout` heraus. Drei Gründe, und jeder trägt allein:
//!
//! 1. **Der offene Prozess IST der Zustand „Menü offen".** Der Tray muss
//!    nichts mitführen, was mit der Wirklichkeit auf dem Schirm synchron
//!    bleiben müsste: Läuft das Kind, ist das Menü offen; ist es weg, ist es
//!    zu. Beim Umschalten gibt es daher keinen Fall, in dem beide Seiten
//!    verschiedener Meinung sind.
//! 2. **Ein Fehler im Fenster darf die Leiste nicht mitnehmen.** Der Tray
//!    hält die Verbindung zur Leiste und zeigt den Zustand der Live-Leitung;
//!    ein Absturz beim Zeichnen eines Menüs wäre ein verschwundenes Symbol.
//!    Umgekehrt genauso: Endet das Kind unsanft, nimmt der Compositor seine
//!    Flächen weg — der Schirm bleibt nicht verdeckt.
//! 3. **Es ist von aussen prüfbar.** `… --menue < menue.json` zeigt dasselbe
//!    Fenster, das ein Klick zeigt, und sagt auf `stdout`, was getroffen
//!    wurde. Genau so ist das Umschalten am echten Hyprland nachgewiesen
//!    worden, ohne einen Screenshot zu deuten.
//!
//! ## Die Geduld
//!
//! Ein Menü, das offen bleibt, weil niemand mehr hinsieht, nimmt die
//! Tastatur mit (der Auffänger tippt ausschliesslich, damit Esc ankommt).
//! Deshalb endet das Fenster nach [`GEDULD`] von selbst. Das ist kein
//! Zeitgeber für den Normalfall — ein Menü wird in Sekunden bedient —,
//! sondern der Riegel gegen den Fall, in dem etwas hängt.

use std::io::Read;

mod farben;
mod flaeche;
mod hyprland;
mod malen;
mod schrift;

pub use flaeche::moeglich;

use crate::menue::{Aktion, Eintrag};

/// Nach dieser Zeit ohne Entscheidung macht das Fenster von selbst zu.
pub const GEDULD: std::time::Duration = std::time::Duration::from_secs(60);

/// Der Unterbefehl `--menue`: Einträge von `stdin`, Aktion auf `stdout`.
///
/// Gibt `Ok(())` zurück, wenn nichts gewählt wurde — weggeklickt ist kein
/// Fehler. Auf `stdout` steht dann nichts.
pub fn als_prozess() -> Result<(), String> {
    let mut hinein = String::new();

    std::io::stdin()
        .read_to_string(&mut hinein)
        .map_err(|e| format!("Menü nicht von stdin lesbar: {e}"))?;

    let eintraege: Vec<Eintrag> = serde_json::from_str(hinein.trim())
        .map_err(|e| format!("Menü nicht lesbar ({e}). Erwartet wird die Liste aus `menue`."))?;

    if eintraege.is_empty() {
        return Err("Leeres Menü — da gäbe es nichts zu zeigen.".into());
    }

    geduld_wecken();

    let gewaehlt = flaeche::zeigen(eintraege)?;

    if let Some(aktion) = gewaehlt {
        let zeile =
            serde_json::to_string(&aktion).map_err(|e| format!("Aktion nicht schreibbar: {e}"))?;

        println!("{zeile}");
    }

    Ok(())
}

/// Der Unterbefehl `--menue-bild`: Einträge von `stdin`, Bild in eine Datei.
///
/// ## Warum es das gibt
///
/// „Sieht unser Menü aus wie Omarchys?" ist eine Frage an ein **Bild**, und
/// bis v0.5.6 gab es keinen Weg, dieses Bild zu bekommen, ohne an einem
/// Schirm zu sitzen. Auf der codingmachine hängt `grim` (dort ist Screencopy
/// gesperrt), auf einem CI-Läufer gibt es gar keine Sitzung — und beides
/// heisst: Die Darstellung war die einzige Eigenschaft dieses Fensters, die
/// niemand nachsehen konnte.
///
/// Hier kommt genau der Puffer heraus, den das Fenster zeigt: dieselbe
/// [`malen::Aufbau::rechnen`]-Rechnung, dieselben Themefarben, dieselbe
/// Schrift. Was dieser Weg **nicht** beantwortet, ist die Lage auf dem Schirm
/// — die hängt an der Leiste und wird mit `hyprctl layers` gemessen.
///
/// Der Hintergrund des Kastens darf halbdurchsichtig sein
/// (`popups.background-alpha`), ein PPM kennt aber kein Alpha. Deshalb wird
/// gegen ein Schachbrett gelegt: So ist zu sehen, wo etwas durchscheint,
/// statt es für einen Farbton zu halten.
pub fn als_bild(pfad: Option<&str>) -> Result<(), String> {
    let pfad = pfad.ok_or("Wohin? `--menue-bild <datei.ppm>`")?;

    let mut hinein = String::new();

    std::io::stdin()
        .read_to_string(&mut hinein)
        .map_err(|e| format!("Menü nicht von stdin lesbar: {e}"))?;

    let eintraege: Vec<Eintrag> = serde_json::from_str(hinein.trim())
        .map_err(|e| format!("Menü nicht lesbar ({e}). Erwartet wird die Liste aus `menue`."))?;

    if eintraege.is_empty() {
        return Err("Leeres Menü — da gäbe es nichts zu zeigen.".into());
    }

    // Die Skalierung über eine Variable, damit sich derselbe Kasten in
    // Gerätegrösse eines skalierten Schirms ansehen lässt — dort fallen
    // Rundungsfehler auf, die bei 1.0 nicht vorkommen.
    let skalierung: f32 = std::env::var("AI_BRAIN_TRAY_MENUE_SKALIERUNG")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v: &f32| *v >= 0.5 && *v <= 4.0)
        .unwrap_or(1.0);

    // Die Zeile, auf der die Maus steht: ohne Angabe die erste klickbare,
    // damit die Hervorhebung auf dem Bild zu sehen ist. `-1` lässt sie weg.
    let hervor = std::env::var("AI_BRAIN_TRAY_MENUE_HERVOR")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .map_or_else(
            || malen::Aufbau::erster_klickbarer(&eintraege, false),
            |i| usize::try_from(i).ok(),
        );

    let stil = farben::Stil::aus_theme();
    let mut schrift = schrift::Schrift::laden(stil.schriftgrad * skalierung)?;

    let aufbau = malen::Aufbau::rechnen(
        &eintraege,
        &stil,
        &mut schrift,
        skalierung,
        malen::Anker {
            maus_x: 400.0,
            maus_y: 13.0,
            leiste: None,
        },
        1920,
        1080,
    );

    let mut puffer = vec![0u32; aufbau.pixel()];
    aufbau.malen(&mut puffer, &eintraege, &mut schrift, hervor);

    let (breite, hoehe) = (aufbau.kasten.breite, aufbau.kasten.hoehe);
    let mut ppm = format!("P6\n{breite} {hoehe}\n255\n").into_bytes();

    for y in 0..hoehe {
        for x in 0..breite {
            let p = puffer[(y * breite + x) as usize];

            // ARGB8888, vorgemultipliziert: Die Farbkanäle sind schon mit
            // Alpha verrechnet, der Rest des Pixels gehört dem Untergrund.
            let a = ((p >> 24) & 0xff) as u32;
            let rest = 255 - a;

            // Schachbrett, 8 Pixel: zwei gut unterscheidbare Graustufen.
            let grund = if ((x / 8) + (y / 8)) % 2 == 0 { 96 } else { 64 };
            let auf = |kanal: u32| (kanal + grund * rest / 255).min(255) as u8;

            ppm.push(auf((p >> 16) & 0xff));
            ppm.push(auf((p >> 8) & 0xff));
            ppm.push(auf(p & 0xff));
        }
    }

    std::fs::write(pfad, ppm).map_err(|e| format!("{pfad} nicht schreibbar: {e}"))?;

    println!("{pfad}: {breite}×{hoehe} bei Skalierung {skalierung}");

    Ok(())
}

/// Ein Faden, der den Prozess nach [`GEDULD`] beendet.
///
/// `exit(0)` und nicht ein Signal an die Ereignisschleife: Dieser Prozess ist
/// zum Wegwerfen da, und „Ende ohne Wahl" ist genau das, was ein Ablauf
/// bedeuten soll. Ein Weg über die Schleife hiesse, sie unterbrechbar zu
/// machen — mehr Aufwand für dasselbe Ergebnis, und ein Weg mehr, auf dem sie
/// hängen bleibt.
fn geduld_wecken() {
    std::thread::spawn(|| {
        std::thread::sleep(GEDULD);
        std::process::exit(0);
    });
}

/// Der Griff auf ein laufendes Menüfenster, aus Sicht des Trays.
pub struct Fenster {
    kind: Option<std::process::Child>,
}

impl Fenster {
    pub fn keins() -> Self {
        Self { kind: None }
    }

    /// Steht das Menü gerade offen?
    ///
    /// Gefragt wird das Betriebssystem, nicht ein eigener Merker: `try_wait`
    /// sagt, ob das Kind noch läuft. Ein Merker wäre genau dann falsch, wenn
    /// es darauf ankommt — nämlich nachdem der Nutzer das Fenster
    /// weggeklickt hat und der Tray davon nichts mitbekommen hat.
    pub fn offen(&mut self) -> bool {
        let Some(kind) = self.kind.as_mut() else {
            return false;
        };

        match kind.try_wait() {
            Ok(None) => true,
            _ => {
                self.kind = None;
                false
            }
        }
    }

    /// Das Menü zumachen, falls es offen ist.
    pub fn schliessen(&mut self) {
        if let Some(mut kind) = self.kind.take() {
            let _ = kind.kill();
            let _ = kind.wait();
        }
    }

    /// Das Menü öffnen. Die gewählte Aktion kommt später über `signalweg`.
    ///
    /// Der Rückkanal läuft über einen Faden und nicht über einen Rückgabewert:
    /// `Activate` kommt im D-Bus-Faden an und muss sofort beantwortet werden —
    /// solange der Nutzer sich das Menü ansieht, darf die Leiste nicht auf
    /// eine Antwort warten.
    pub fn oeffnen(
        &mut self,
        eintraege: &[Eintrag],
        signalweg: std::sync::mpsc::Sender<crate::kern::Signal>,
    ) -> Result<(), String> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        self.schliessen();

        let selbst =
            std::env::current_exe().map_err(|e| format!("Eigenen Pfad nicht gefunden: {e}"))?;

        let text =
            serde_json::to_string(eintraege).map_err(|e| format!("Menü nicht schreibbar: {e}"))?;

        let mut kind = Command::new(&selbst)
            .arg("--menue")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Menüfenster nicht startbar: {e}"))?;

        // Der Schreibzeiger muss weg, sonst liest das Kind bis zum Sankt
        // Nimmerleinstag auf das Ende der Eingabe.
        {
            let mut hinein = kind
                .stdin
                .take()
                .ok_or("Menüfenster nimmt keine Eingabe an.")?;

            hinein
                .write_all(text.as_bytes())
                .map_err(|e| format!("Menü nicht übergebbar: {e}"))?;
        }

        let heraus = kind.stdout.take();
        self.kind = Some(kind);

        std::thread::spawn(move || {
            let Some(mut heraus) = heraus else {
                return;
            };

            let mut antwort = String::new();

            if heraus.read_to_string(&mut antwort).is_err() {
                return;
            }

            let antwort = antwort.trim();

            if antwort.is_empty() {
                // Weggeklickt. Kein Signal — es gibt nichts zu tun, und ein
                // „Wecken" würde den Zustand ohne Anlass neu lesen.
                return;
            }

            match serde_json::from_str::<Aktion>(antwort) {
                Ok(aktion) => {
                    let _ = signalweg.send(crate::kern::Signal::Gewaehlt(aktion));
                }
                Err(_) => {
                    // Unlesbare Antwort: nichts tun. Eine Aktion zu raten
                    // wäre schlimmer als eine, die ausbleibt — hier hängen
                    // „Leitung an" und „Tray beenden" am selben Kanal.
                }
            }
        });

        Ok(())
    }
}

impl Drop for Fenster {
    /// Endet der Tray, geht auch sein Menü.
    ///
    /// Sonst stünde eine bildschirmgrosse Fläche über der Leiste, die auf
    /// einen Klick für einen Tray wartet, den es nicht mehr gibt.
    fn drop(&mut self) {
        self.schliessen();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ein Fenster, das nie geöffnet wurde, ist zu — und bleibt es auch beim
    /// zweiten Fragen.
    #[test]
    fn ohne_kind_ist_nichts_offen() {
        let mut f = Fenster::keins();

        assert!(!f.offen());
        assert!(!f.offen());

        // Und Schliessen darf trotzdem nicht schiefgehen.
        f.schliessen();
        assert!(!f.offen());
    }

    /// `offen()` fragt das Betriebssystem, nicht einen Merker.
    ///
    /// Geprüft mit einem Prozess, der sofort endet: Danach muss `offen()`
    /// `false` sagen, ohne dass irgendwer es ihm gesagt hat. Genau daran
    /// hängt das Umschalten — der Tray erfährt vom Wegklicken nur so.
    #[test]
    fn ein_beendetes_kind_gilt_als_zu() {
        let kind = std::process::Command::new("true").spawn();

        let Ok(kind) = kind else {
            // Kein `/bin/true` — dann sagt diese Probe nichts.
            return;
        };

        let mut f = Fenster { kind: Some(kind) };

        // Dem Kind einen Moment geben; `true` ist schnell, aber nicht sofort.
        for _ in 0..100 {
            if !f.offen() {
                return;
            }

            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        panic!("Ein längst beendetes Kind gilt immer noch als offen");
    }

    /// Die Geduld ist lang genug fürs Bedienen und kurz genug zum Aufräumen.
    #[test]
    fn die_geduld_liegt_im_sinnvollen_bereich() {
        assert!(GEDULD >= std::time::Duration::from_secs(20));
        assert!(GEDULD <= std::time::Duration::from_secs(300));
    }
}
