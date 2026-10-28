//! Das Menüfenster unter Wayland: zwei Arten von Flächen, eine Antwort.
//!
//! ## Warum überhaupt ein eigenes Fenster (#1124, #9008)
//!
//! Bastian will, dass ein zweiter Linksklick das Tray-Menü wieder
//! **schliesst** — und dass ein Klick daneben es auch schliesst, auch auf der
//! Leiste selbst. Über StatusNotifierItem ist das nicht zu machen: Das Menü
//! ist dort das Fenster des Hosts, wir liefern nur den Inhalt, und es gibt
//! kein „schliess mein Menü" in der Spezifikation. Omarchys Leiste schaltet
//! von sich aus nicht um (`openTrayMenu()` endet immer mit
//! `trayMenuOpen = true`).
//!
//! Also zeichnen wir das Menü selbst. Dann liegt das Umschalten vollständig
//! bei uns — und zwar ohne einen Zustand, der mit der Leiste synchron bleiben
//! müsste. Siehe unten.
//!
//! ## Zwei Flächen, und die erste ist der eigentliche Trick
//!
//! | Fläche | Grösse | Wozu |
//! |---|---|---|
//! | **Auffänger**, je Ausgabe | ganzer Schirm, durchsichtig | fängt jeden Klick daneben |
//! | **Kasten**, einmal | so gross wie das Menü | das Menü selbst |
//!
//! Der Auffänger liegt in der `Overlay`-Ebene, also **über** der Leiste.
//! Damit lösen sich alle drei Fälle aus Bastians Satz von selbst:
//!
//! 1. *Linksklick aufs Symbol öffnet* — `Activate` kommt an, der Tray startet
//!    dieses Fenster.
//! 2. *Zweiter Linksklick schliesst* — der Klick trifft den Auffänger, nicht
//!    mehr das Symbol. Wir enden, das Fenster ist weg. Beim **dritten** Klick
//!    gibt es kein Fenster mehr, also kommt `Activate` wieder beim Symbol an
//!    und öffnet. Ein Umschalter ohne Schalter: Es gibt keinen Zustand „Menü
//!    offen", den zwei Programme gleich sehen müssten — die Fläche IST der
//!    Zustand.
//! 3. *Klick irgendwo daneben schliesst* — dasselbe, an jeder anderen Stelle
//!    des Schirms. Und auf jedem Schirm: Der Auffänger wird je Ausgabe
//!    angelegt. Welche Ausgabe „die aktive" ist, weiss ein Wayland-Client
//!    nicht, und ein Klick auf dem zweiten Schirm muss genauso schliessen.
//!
//! ## Woher die Mausstelle kommt
//!
//! Nicht von `hyprctl` — sondern von Wayland selbst. Sobald der Auffänger
//! liegt, schickt der Compositor ein `wl_pointer.enter` **mit flächenlokalen
//! Koordinaten**, weil der Zeiger ja darüber steht. Der Auffänger ist genau
//! die Ausgabe gross, also sind das die Koordinaten auf dieser Ausgabe — und
//! zwar auf der richtigen, ohne Monitorliste, ohne Rechnen mit globalen
//! Positionen, ohne compositor-eigenes Werkzeug. Das trägt auf jedem
//! wlroots-Compositor, nicht nur auf Hyprland.
//!
//! ## Was dieses Modul NICHT tut
//!
//! Es rechnet nicht, wie das Menü aussieht (`malen`), es kennt die Farben
//! nicht (`farben`), es weiss nicht, was ein Menüpunkt bedeutet (`menue`,
//! `kern`). Es bindet an Wayland — und nur das.

use std::fs::File;
use std::io::Result as IoResult;
use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;

use wayland_client::protocol::{
    wl_buffer::WlBuffer,
    wl_compositor::WlCompositor,
    wl_keyboard::{self, WlKeyboard},
    wl_output::{self, WlOutput},
    wl_pointer::{self, WlPointer},
    wl_region::WlRegion,
    wl_registry::{self, WlRegistry},
    wl_seat::{self, WlSeat},
    wl_shm::{Format, WlShm},
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
};
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1},
};

use crate::menue::{Aktion, Eintrag};

use super::farben::Stil;
use super::hyprland;
use super::malen::{Anker, Aufbau};
use super::schrift::Schrift;

/// Die Namen, unter denen die Flächen in `hyprctl layers` auftauchen.
///
/// Nicht Zierde: Das ist der Weg, auf dem sich von aussen nachweisen lässt,
/// DASS das Menü offen ist — ohne einen Screenshot zu deuten. Die Probe am
/// echten Hyprland liest genau diese Namen.
pub const NAMENSRAUM_KASTEN: &str = "ai-brain-tray-menue";
pub const NAMENSRAUM_FAENGER: &str = "ai-brain-tray-menue-auffaenger";

/// So lange wird auf das `wl_pointer.enter` gewartet, bevor der Kasten ohne
/// Mausstelle aufgeht.
///
/// 400 ms ist mit Absicht reichlich: Die Maus meldet sich im Alltag nach
/// wenigen Millisekunden, und der Rückfall ist ein Notausgang, kein Zweitweg.
/// Lieber einmal 400 ms gewartet, als das Menü auf dem falschen Bildschirm zu
/// zeigen.
const GEDULD_ZEIGER: std::time::Duration = std::time::Duration::from_millis(400);

/// Schreibt jedes Wayland-Ereignis nach stderr, wenn gesetzt.
///
/// Das ist nicht Entwicklerkram, der liegengeblieben ist: Am 28.10.2026 hat
/// genau diese Zeile den Unterschied gemacht zwischen „der Klick kommt nicht
/// an" und „der Klick kommt auf der falschen Flaeche an". Von aussen sahen
/// beide gleich aus, und ein Compositor laesst sich nicht befragen.
fn laut() -> bool {
    std::env::var_os("AI_BRAIN_TRAY_MENUE_LAUT").is_some()
}

macro_rules! sagen {
    ($($t:tt)*) => {
        if laut() {
            eprintln!("[menue] {}", format!($($t)*));
        }
    };
}

/// ## Wer die Tastatur bekommt — und warum das den Zeiger entscheidet
///
/// Gemessen am 28.10.2026 auf Hyprland 0.56.2, mit zwei Ausgaben:
///
/// | `keyboard_interactivity` | Zeigerereignisse gehen an | Esc |
/// |---|---|---|
/// | `none` überall | die Fläche **unter** dem Zeiger, flächenlokal | kommt nicht an |
/// | `exclusive` auf dem Kasten | **immer an den Kasten**, kastenlokal (auch ausserhalb) | kommt an |
/// | `exclusive` auf den Auffängern | immer an den **ersten** Auffänger, Koordinaten der ganzen Anordnung | kommt an |
/// | `on_demand` | wie `exclusive` | kommt an |
///
/// **Hyprland leitet den Zeiger auf jede Layer-Fläche um, deren
/// `keyboard_interactivity` nicht `none` ist.** Daran sind drei Anläufe
/// gescheitert: Mit `exclusive` auf den Auffängern kam die Mausstelle als
/// Koordinate der ganzen Anordnung, und der Kasten landete auf dem falschen
/// Bildschirm — die Maus stand auf dem zweiten Schirm, das Menü erschien auf
/// dem ersten.
///
/// Daraus die Bauform:
///
/// * **Auffänger `None`.** Sie sollen den Klick da fangen, wo er hinfällt.
/// * **Kasten `Exclusive`.** Dann kommt Esc an. Der umgeleitete Zeiger ist
///   dabei kein Verlust, sondern ein **zweiter** Weg zum Schliessen: Ein Klick
///   neben das Menü erreicht uns dann als Kastenereignis mit Koordinaten
///   ausserhalb des Kastens — also „daneben", und das heisst zu. Auf einem
///   Compositor ohne diese Umleitung fängt ihn der Auffänger. Eine Regel, zwei
///   Welten (siehe [`Zustand::daneben_geklickt`]).
///
/// Der Kasten entsteht erst, **nachdem** die Mausstelle bekannt ist — sonst
/// würde seine Umleitung genau die Auskunft verschlucken, die wir brauchen.
/// Evdev-Tastenkennungen, wie `wl_keyboard.key` sie schickt.
///
/// Roh, nicht über xkbcommon: Für vier Tasten eine C-Bibliothek einzubinden —
/// und sie damit zur Voraussetzung des Trays zu machen, der heute ohne eine
/// einzige auskommt — steht in keinem Verhältnis. Diese Kennungen sind
/// Linux-Eingabecodes und ändern sich nicht; `Esc` ist 1, seit es evdev gibt.
/// Für **Buchstaben** wäre derselbe Weg falsch, weil dort das Tastaturlayout
/// entscheidet. Hier kommt kein Buchstabe vor.
mod taste {
    pub const ESC: u32 = 1;
    pub const EINGABE: u32 = 28;
    pub const EINGABE_ZIFFERNBLOCK: u32 = 96;
    pub const HOCH: u32 = 103;
    pub const RUNTER: u32 = 108;
}

/// Ein Auffänger — eine bildschirmgrosse, durchsichtige Fläche.
struct Faenger {
    /// `None` nur, wenn der Compositor uns keine Ausgabe genannt hat; dann
    /// wählt er selbst, welche es wird.
    output: Option<WlOutput>,
    /// Wie der Compositor diese Ausgabe nennt — für die Zuordnung zu
    /// Hyprlands Monitorliste.
    name: Option<String>,
    surface: WlSurface,
    layer: ZwlrLayerSurfaceV1,
    viewport: Option<WpViewport>,
    /// Logische Grösse, aus dem `configure`.
    breite: i32,
    hoehe: i32,
    /// Skalierung in 120steln — so zählt Wayland fraktionale Skalierung.
    skalierung_120: Option<u32>,
    /// Die ganzzahlige der Ausgabe, als Rückfall.
    ganzzahlig: i32,
    /// Liegt schon ein Puffer darauf?
    liegt: bool,
    /// Hält die Dateizeiger am Leben, solange die Fläche steht.
    _ablage: Option<(File, WlShmPool, WlBuffer)>,
}

/// Die Fläche mit dem Menü darauf.
struct Kasten {
    surface: WlSurface,
    layer: ZwlrLayerSurfaceV1,
    viewport: Option<WpViewport>,
    aufbau: Aufbau,
    datei: File,
    _pool: WlShmPool,
    puffer: WlBuffer,
    /// Hat der Compositor die Grösse bestätigt? Vorher darf nichts gemalt
    /// werden — ein Puffer ohne beantwortetes `configure` ist ein
    /// Protokollfehler.
    bestaetigt: bool,
}

/// Wem ein `configure` oder eine Skalierung gilt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wer {
    Faenger(usize),
    Kasten,
}

struct Zustand {
    compositor: Option<WlCompositor>,
    shm: Option<WlShm>,
    schichten: Option<ZwlrLayerShellV1>,
    viewporter: Option<WpViewporter>,
    bruchteile: Option<WpFractionalScaleManagerV1>,
    outputs: Vec<WlOutput>,
    /// Die Namen der Ausgaben, auch der noch nicht zugeordneten: Das
    /// `name`-Ereignis kann vor dem Anlegen der Auffänger eintreffen.
    ausgabe_namen: Vec<(wayland_client::backend::ObjectId, String)>,

    faenger: Vec<Faenger>,
    kasten: Option<Kasten>,

    eintraege: Vec<Eintrag>,
    stil: Stil,
    schrift: Option<Schrift>,

    /// Auf welcher Fläche steht der Zeiger, und wo darauf (logisch)?
    zeiger: Option<(Wer, f64, f64)>,
    /// Welcher Eintrag ist hervorgehoben?
    hervor: Option<usize>,

    gewaehlt: Option<Aktion>,
    fertig: bool,
    fehler: Option<String>,
}

/// Zeigt das Menü und gibt zurück, was gewählt wurde.
///
/// `Ok(None)` heisst: weggeklickt, Esc, oder der Compositor hat die Fläche
/// zurückgenommen — also „kein Punkt gewählt". Für den Tray ist das der
/// Normalfall und kein Fehler: Ein zugeklapptes Menü ist genau das, was
/// Bastian beim zweiten Klick sehen will.
pub fn zeigen(eintraege: Vec<Eintrag>) -> Result<Option<Aktion>, String> {
    let verbindung =
        Connection::connect_to_env().map_err(|e| format!("Keine Wayland-Verbindung: {e}"))?;

    let mut schlange = verbindung.new_event_queue();
    let qh = schlange.handle();

    let _registry = verbindung.display().get_registry(&qh, ());

    let mut z = Zustand {
        compositor: None,
        shm: None,
        schichten: None,
        viewporter: None,
        bruchteile: None,
        outputs: Vec::new(),
        ausgabe_namen: Vec::new(),
        faenger: Vec::new(),
        kasten: None,
        eintraege,
        stil: Stil::aus_theme(),
        schrift: None,
        zeiger: None,
        hervor: None,
        gewaehlt: None,
        fertig: false,
        fehler: None,
    };

    // Zwei Runden: die erste bringt die Globals, die zweite die Ereignisse
    // der Objekte, die daraus entstehen (Sitzplatz → Zeiger und Tastatur,
    // Ausgabe → Skalierung).
    for _ in 0..2 {
        schlange
            .roundtrip(&mut z)
            .map_err(|e| format!("Wayland antwortet nicht: {e}"))?;
    }

    if z.schichten.is_none() {
        return Err(
            "Diese Oberfläche kennt wlr-layer-shell nicht — hier bleibt das Menü der Leiste."
                .into(),
        );
    }

    z.faenger_anlegen(&qh)?;

    // ## Wo steht die Maus? Drei Wege, in dieser Reihenfolge
    //
    // 1. **Hyprland fragen** (`popup::hyprland`). Kostet eine Verbindung auf
    //    einen Unix-Sockel und ist sofort da.
    // 2. **Auf `wl_pointer.enter` warten.** Der reine Wayland-Weg, und auf
    //    einem Compositor, der ein `enter` für eine neu aufgespannte Fläche
    //    schickt, ist er der richtige.
    // 3. **Die Ecke.** Oben rechts, wo Tray-Symbole sitzen.
    //
    // Warum nicht Weg 2 zuerst, wo er doch der portable ist: Gemessen am
    // 28.10.2026 schickt Hyprland 0.56.2 **kein** `enter`, solange der Zeiger
    // stillsteht — und beim Klick auf ein Tray-Symbol steht er still, er ist
    // ja schon dort. Weg 2 liefe also auf jedem Klick in seine Zeitgrenze und
    // würde dann falsch antworten. Wer die Auskunft hat, soll sie nehmen.
    if let Some(stelle) = hyprland::zeigerstelle() {
        sagen!(
            "Hyprland: Zeiger auf {} bei {},{}",
            stelle.ausgabe,
            stelle.x,
            stelle.y
        );

        // Bis die Auffänger ihre Grösse haben — ohne sie wäre nicht zu sagen,
        // ob der Kasten noch auf den Schirm passt.
        let beginn = std::time::Instant::now();

        while !z.faenger.iter().all(|f| f.breite > 0) && beginn.elapsed() < GEDULD_ZEIGER {
            schlange
                .roundtrip(&mut z)
                .map_err(|e| format!("Wayland antwortet nicht: {e}"))?;
        }

        let i = z
            .faenger
            .iter()
            .position(|f| f.name.as_deref() == Some(stelle.ausgabe.as_str()));

        if let Some(i) = i {
            let anker = Anker {
                maus_x: stelle.x,
                maus_y: stelle.y,
                leiste: stelle.leiste,
            };

            if let Err(grund) = z.kasten_anlegen(&qh, i, anker) {
                z.abraeumen();
                return Err(grund);
            }
        } else {
            sagen!(
                "Ausgabe {} ist nicht unsere — weiter mit enter",
                stelle.ausgabe
            );
        }
    }

    // Weg 2: auf das `enter` warten — nach der Uhr, nicht nach Runden.
    //
    // Hier standen bis zur ersten Probe am echten Hyprland „sechs Runden",
    // und das war falsch gedacht: Ein `roundtrip` ist ein Umlauf zum
    // Compositor und zurück, sechs davon sind in Mikrosekunden durch. Ein
    // `enter` braucht dagegen einen Durchlauf von dessen Schleife.
    let beginn = std::time::Instant::now();

    while z.kasten.is_none() && !z.fertig && beginn.elapsed() < GEDULD_ZEIGER {
        schlange
            .roundtrip(&mut z)
            .map_err(|e| format!("Wayland antwortet nicht: {e}"))?;

        if z.kasten.is_some() || z.fertig {
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    if let Some(grund) = z.fehler.take() {
        z.abraeumen();
        return Err(grund);
    }

    if z.kasten.is_none() && !z.fertig {
        // Weg 3. Kein Zeiger zu finden — das Menü soll trotzdem aufgehen.
        let (b, h) = z
            .faenger
            .first()
            .map(|f| (f.breite.max(1), f.hoehe.max(1)))
            .unwrap_or((1920, 1080));

        sagen!("kein Zeiger zu finden, Kasten in die Ecke");

        let anker = Anker {
            maus_x: (b - 24) as f64,
            maus_y: 14.0_f64.min(h as f64),
            leiste: None,
        };

        if let Err(grund) = z.kasten_anlegen(&qh, 0, anker) {
            z.abraeumen();
            return Err(grund);
        }
    }

    while !z.fertig {
        if schlange.blocking_dispatch(&mut z).is_err() {
            break;
        }
    }

    let fehler = z.fehler.take();

    // Die Flächen abräumen, bevor der Prozess endet: Der Compositor sieht
    // dann ein ordentliches Zerstören statt eines Verbindungsabbruchs. Das
    // ist dieselbe Höflichkeit wie die Atempause beim Beenden des Trays — ein
    // Abbruch landet im Protokoll der Leiste und sieht dort aus wie ein
    // Fehler.
    z.abraeumen();
    let _ = verbindung.roundtrip();

    match fehler {
        Some(grund) => Err(grund),
        None => Ok(z.gewaehlt),
    }
}

/// Gibt es hier eine Oberfläche, auf der das eigene Menü laufen kann?
///
/// Geprüft wird, was wirklich gebraucht wird: eine Wayland-Verbindung **und**
/// `zwlr_layer_shell_v1`. Die Frage „ist das Wayland?" allein genügt nicht —
/// GNOME und KDE sind Wayland und haben kein layer-shell, und dort muss der
/// Tray beim Menü der Leiste bleiben.
///
/// Der Versuch selbst ist die Prüfung; eine Liste von Oberflächen, die es
/// können, wäre eine Liste, die altert.
pub fn moeglich() -> bool {
    let Ok(verbindung) = Connection::connect_to_env() else {
        return false;
    };

    let mut schlange = verbindung.new_event_queue();
    let qh = schlange.handle();
    let _registry = verbindung.display().get_registry(&qh, ());

    let mut suche = Suche { gefunden: false };

    schlange.roundtrip(&mut suche).is_ok() && suche.gefunden
}

/// Der Minimalzustand für [`moeglich`] — nur die eine Frage, und kein Objekt,
/// das danach noch Ereignisse schicken könnte.
struct Suche {
    gefunden: bool,
}

impl Dispatch<WlRegistry, ()> for Suche {
    fn event(
        suche: &mut Self,
        _registry: &WlRegistry,
        ereignis: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { interface, .. } = ereignis {
            if interface == "zwlr_layer_shell_v1" {
                suche.gefunden = true;
            }
        }
    }
}

/// Was ein Mausklick bedeutet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Klick {
    /// Den hervorgehobenen Punkt nehmen — und wenn keiner hervorgehoben ist
    /// (Rahmen, Polster, Trenner, ausgegrauter Punkt), nichts tun. Das Menü
    /// bleibt dann offen, wie in jedem anderen Menü auch.
    Waehlen,
    /// Das Menü geht zu — Fall 2 und Fall 3 aus Bastians Satz.
    Schliessen,
}

/// Wohin ein Klick gehört — als reine Funktion, damit sie ohne Compositor
/// prüfbar ist.
///
/// ## Warum das die heikelste Stelle im Modul ist
///
/// „Daneben" ist **zweierlei**, und beides muss schliessen:
///
/// * ein Klick auf einem **Auffänger** — so kommt er auf jedem Compositor,
///   der den Zeiger dorthin schickt, wo er steht;
/// * ein Klick, der als **Kastenereignis mit Koordinaten ausserhalb des
///   Kastens** ankommt — so kommt er auf Hyprland, das den Zeiger auf unsere
///   `exclusive`-Fläche umleitet (siehe die Tabelle am Kopf dieses Moduls).
///
/// Fiele der zweite Fall weg, bliebe das Menü bei jedem Klick daneben offen —
/// und das ist wörtlich der Fehler, den Bug #1124 beschreibt.
fn entscheiden(zeiger: Option<(Wer, f64, f64)>, kasten: Option<&Aufbau>) -> Klick {
    match (zeiger, kasten) {
        // Auf dem Kasten, und zwar wirklich darauf.
        (Some((Wer::Kasten, x, y)), Some(aufbau)) if aufbau.im_kasten(x, y) => Klick::Waehlen,
        // Alles andere ist daneben: ein Auffänger, eine fremde Fläche, kein
        // Zeiger — oder eben ein umgeleiteter Klick ausserhalb des Kastens.
        _ => Klick::Schliessen,
    }
}

impl Zustand {
    /// Je Ausgabe eine bildschirmgrosse, durchsichtige Fläche.
    fn faenger_anlegen(&mut self, qh: &QueueHandle<Self>) -> Result<(), String> {
        let compositor = self.compositor.clone().ok_or("Kein wl_compositor.")?;
        let schichten = self.schichten.clone().ok_or("Kein wlr-layer-shell.")?;

        // Ohne eine einzige Ausgabe bindet wlroots die Fläche an die, die der
        // Compositor für richtig hält — für einen Auffänger ist das in
        // Ordnung.
        let ausgaben: Vec<Option<WlOutput>> = if self.outputs.is_empty() {
            vec![None]
        } else {
            self.outputs.iter().cloned().map(Some).collect()
        };

        for (i, output) in ausgaben.into_iter().enumerate() {
            let surface = compositor.create_surface(qh, ());

            let layer = schichten.get_layer_surface(
                &surface,
                output.as_ref(),
                // `Overlay`, damit der Auffänger ÜBER der Leiste liegt —
                // sonst fängt er den Klick auf die Leiste nicht, und genau
                // der ist Fall 3 aus Bastians Satz.
                Layer::Overlay,
                NAMENSRAUM_FAENGER.to_string(),
                qh,
                Wer::Faenger(i),
            );

            layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
            // -1 heisst: die Sperrzonen anderer Flächen übergehen. Mit 0
            // würde die Leiste ihre 26 Pixel abziehen, und die
            // Mauskoordinaten wären gegenüber der Ausgabe verschoben — jede
            // Position daneben.
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            surface.commit();

            let name = output.as_ref().and_then(|o| {
                self.ausgabe_namen
                    .iter()
                    .find(|(id, _)| *id == o.id())
                    .map(|(_, n)| n.clone())
            });

            self.faenger.push(Faenger {
                output,
                name,
                surface,
                layer,
                viewport: None,
                breite: 0,
                hoehe: 0,
                skalierung_120: None,
                ganzzahlig: 1,
                liegt: false,
                _ablage: None,
            });
        }

        Ok(())
    }

    /// Den durchsichtigen Puffer auf einen Auffänger legen.
    ///
    /// Mit `wp_viewporter` reicht **ein Pixel**, auf die ganze Fläche
    /// gestreckt. Das ist nicht Feinschliff: Ein bildschirmgrosser Puffer
    /// wären bei 3840×2560 rund 39 MB, die der Compositor einliest — für eine
    /// Fläche, die vollständig durchsichtig ist und sich nie ändert.
    ///
    /// Ohne Viewporter ein Puffer in logischer Grösse mit `buffer_scale 1`.
    /// Dass er auf einem fein skalierten Schirm unscharf hochgerechnet würde,
    /// ist gleichgültig: Da ist nichts zu sehen.
    fn faenger_fuellen(&mut self, i: usize, qh: &QueueHandle<Self>) {
        let (Some(shm), Some(compositor)) = (self.shm.clone(), self.compositor.clone()) else {
            return;
        };

        let viewporter = self.viewporter.clone();

        let Some(f) = self.faenger.get(i) else {
            return;
        };

        if f.liegt || f.breite <= 0 || f.hoehe <= 0 {
            return;
        }

        let (voll_breite, voll_hoehe) = (f.breite, f.hoehe);

        let (breite, hoehe) = if viewporter.is_some() {
            (1, 1)
        } else {
            (voll_breite, voll_hoehe)
        };

        let groesse = breite as usize * hoehe as usize * 4;

        let datei = match leere_datei(groesse) {
            Ok(d) => d,
            Err(e) => {
                self.fehler = Some(format!("Kein Puffer anzulegen: {e}"));
                self.fertig = true;
                return;
            }
        };

        let pool = shm.create_pool(datei.as_fd(), groesse as i32, qh, ());
        let puffer = pool.create_buffer(0, breite, hoehe, breite * 4, Format::Argb8888, qh, ());

        let surface = self.faenger[i].surface.clone();

        let viewport = viewporter.as_ref().map(|v| {
            let vp = v.get_viewport(&surface, qh, ());
            vp.set_destination(voll_breite, voll_hoehe);
            vp
        });

        eingabeflaeche(&compositor, &surface, voll_breite, voll_hoehe, qh);

        surface.attach(Some(&puffer), 0, 0);
        surface.damage_buffer(0, 0, breite, hoehe);
        surface.commit();

        let f = &mut self.faenger[i];
        f.viewport = viewport;
        f._ablage = Some((datei, pool, puffer));
        f.liegt = true;
    }

    /// Die Skalierung dieser Ausgabe als Faktor.
    fn skalierung(&self, i: usize) -> f32 {
        let Some(f) = self.faenger.get(i) else {
            return 1.0;
        };

        // Ohne Viewporter wird in logischen Pixeln gemalt — der Faktor ist
        // dann 1, und zwar bewusst: Ein Puffer in Gerätegrösse bräuchte eine
        // ganzzahlige Skalierung, die genau aufgeht, und die gibt es bei 1.25
        // nicht. Lieber etwas weicher als an der falschen Stelle.
        if self.viewporter.is_none() {
            return 1.0;
        }

        match f.skalierung_120 {
            Some(v) if v > 0 => v as f32 / 120.0,
            _ => f.ganzzahlig.max(1) as f32,
        }
    }

    /// Den Menükasten an der Mausstelle anlegen.
    fn kasten_anlegen(
        &mut self,
        qh: &QueueHandle<Self>,
        i: usize,
        anker: Anker,
    ) -> Result<(), String> {
        if self.kasten.is_some() {
            return Ok(());
        }

        let compositor = self.compositor.clone().ok_or("Kein wl_compositor.")?;
        let schichten = self.schichten.clone().ok_or("Kein wlr-layer-shell.")?;
        let shm = self.shm.clone().ok_or("Kein wl_shm.")?;

        let skalierung = self.skalierung(i);
        let (flaeche_breite, flaeche_hoehe) = self
            .faenger
            .get(i)
            .map(|f| (f.breite.max(1), f.hoehe.max(1)))
            .unwrap_or((1920, 1080));

        // Die Schrift wird in GERÄTEpixeln geladen — einmal, und dann für das
        // Vermessen wie für das Malen dieselbe. Zwei Ladungen in zwei Grössen
        // wären zwei Breiten für denselben Text, und der Kasten passte zu
        // keiner davon.
        let mut schrift = Schrift::laden(self.stil.schriftgrad * skalierung)?;

        let aufbau = Aufbau::rechnen(
            &self.eintraege,
            &self.stil,
            &mut schrift,
            skalierung,
            anker,
            flaeche_breite,
            flaeche_hoehe,
        );

        let groesse = aufbau.pixel() * 4;
        let datei = leere_datei(groesse).map_err(|e| format!("Kein Puffer anzulegen: {e}"))?;

        let pool = shm.create_pool(datei.as_fd(), groesse as i32, qh, ());
        let puffer = pool.create_buffer(
            0,
            aufbau.kasten.breite,
            aufbau.kasten.hoehe,
            aufbau.kasten.breite * 4,
            Format::Argb8888,
            qh,
            (),
        );

        let surface = compositor.create_surface(qh, ());
        let output = self.faenger.get(i).and_then(|f| f.output.clone());

        let layer = schichten.get_layer_surface(
            &surface,
            output.as_ref(),
            Layer::Overlay,
            NAMENSRAUM_KASTEN.to_string(),
            qh,
            Wer::Kasten,
        );

        let (links, oben, breite, hoehe) = aufbau.logisch();

        layer.set_anchor(Anchor::Top | Anchor::Left);
        layer.set_size(breite as u32, hoehe as u32);
        // Oben und links sind die Ränder, an denen die Fläche hängt; die
        // anderen zwei bleiben 0.
        layer.set_margin(oben, 0, 0, links);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);

        let viewport = self.viewporter.as_ref().map(|v| {
            let vp = v.get_viewport(&surface, qh, ());
            vp.set_destination(breite, hoehe);
            vp
        });

        eingabeflaeche(&compositor, &surface, breite, hoehe, qh);
        surface.commit();

        self.schrift = Some(schrift);
        self.kasten = Some(Kasten {
            surface,
            layer,
            viewport,
            aufbau,
            datei,
            _pool: pool,
            puffer,
            bestaetigt: false,
        });

        Ok(())
    }

    /// Das Menü malen und zeigen.
    fn kasten_malen(&mut self) {
        let (Some(kasten), Some(schrift)) = (self.kasten.as_mut(), self.schrift.as_mut()) else {
            return;
        };

        if !kasten.bestaetigt {
            return;
        }

        let mut bild = vec![0u32; kasten.aufbau.pixel()];
        kasten
            .aufbau
            .malen(&mut bild, &self.eintraege, schrift, self.hervor);

        // Vom `Vec<u32>` in die Datei, die der Compositor liest. `u32` nach
        // Bytes in der Reihenfolge DIESES Rechners: `wl_shm` beschreibt
        // `ARGB8888` als 32-Bit-Wort in Wirtsreihenfolge, nicht als vier
        // Bytes in festgelegter Folge.
        let mut bytes = Vec::with_capacity(bild.len() * 4);

        for p in &bild {
            bytes.extend_from_slice(&p.to_ne_bytes());
        }

        if let Err(e) = kasten.datei.write_all_at(&bytes, 0) {
            self.fehler = Some(format!("Puffer nicht beschreibbar: {e}"));
            self.fertig = true;
            return;
        }

        kasten.surface.attach(Some(&kasten.puffer), 0, 0);
        kasten.surface.damage_buffer(
            0,
            0,
            kasten.aufbau.kasten.breite,
            kasten.aufbau.kasten.hoehe,
        );
        kasten.surface.commit();
    }

    /// Hervorhebung setzen und, wenn sie sich geändert hat, neu malen.
    fn hervorheben(&mut self, neu: Option<usize>) {
        if self.hervor == neu {
            return;
        }

        self.hervor = neu;
        self.kasten_malen();
    }

    /// Den hervorgehobenen Punkt wählen — oder nichts tun.
    fn waehlen(&mut self) {
        let Some(i) = self.hervor else {
            return;
        };

        if let Some(aktion) = self.eintraege.get(i).and_then(Eintrag::aktion) {
            self.gewaehlt = Some(aktion.clone());
            self.fertig = true;
        }
    }

    /// Der Zeiger hat sich bewegt — Menü anlegen oder Hervorhebung nachziehen.
    fn zeiger_gewandert(&mut self, qh: &QueueHandle<Self>) {
        let Some((wer, x, y)) = self.zeiger else {
            return;
        };

        match wer {
            Wer::Faenger(i) => {
                if self.kasten.is_none() {
                    // Ohne Leiste: Dieser Weg greift nur, wenn Hyprland die
                    // Auskunft nicht gab — dann gibt es auch keine
                    // Leistengeometrie, und es bleibt beim Zeiger.
                    let anker = Anker {
                        maus_x: x,
                        maus_y: y,
                        leiste: None,
                    };

                    if let Err(grund) = self.kasten_anlegen(qh, i, anker) {
                        self.fehler = Some(grund);
                        self.fertig = true;
                    }
                }
            }
            Wer::Kasten => {
                // Steht der Zeiger ausserhalb (Hyprlands Umleitung), gibt
                // `treffer` von sich aus `None` — die Hervorhebung fällt dann
                // weg, und das ist richtig: Es ist kein Punkt unter der Maus.
                let treffer = self
                    .kasten
                    .as_ref()
                    .and_then(|k| k.aufbau.treffer(&self.eintraege, x, y));

                self.hervorheben(treffer);
            }
        }
    }

    fn abraeumen(&mut self) {
        if let Some(k) = self.kasten.take() {
            if let Some(v) = &k.viewport {
                v.destroy();
            }
            k.layer.destroy();
            k.surface.destroy();
        }

        for f in self.faenger.drain(..) {
            if let Some(v) = &f.viewport {
                v.destroy();
            }
            f.layer.destroy();
            f.surface.destroy();
        }
    }

    /// Zu welcher unserer Flächen gehört dieses `wl_surface`?
    fn wer(&self, surface: &WlSurface) -> Option<Wer> {
        if let Some(k) = &self.kasten {
            if k.surface.id() == surface.id() {
                return Some(Wer::Kasten);
            }
        }

        self.faenger
            .iter()
            .position(|f| f.surface.id() == surface.id())
            .map(Wer::Faenger)
    }
}

/// Die Eingabefläche einer Fläche ausdrücklich auf `breite × hoehe` setzen
/// (logische Masse).
///
/// ## Warum das nicht weggelassen werden darf
///
/// Ohne Angabe gilt die Eingabefläche als unbegrenzt — in der Spezifikation.
/// In der Praxis schneidet der Compositor sie auf die Fläche zu, und „die
/// Fläche" ist die **Puffergrösse**. Beim Auffänger ist der Puffer ein
/// einziges Pixel, auf den Schirm gestreckt (`wp_viewport`): Gemessen am
/// 28.10.2026 auf Hyprland 0.56.2 kam deshalb **kein** `wl_pointer.enter`,
/// obwohl der Zeiger mitten über der Fläche stand. Von aussen sah das aus wie
/// „der Klick kommt nicht an".
///
/// Beim Kasten ist es derselbe Fehler mit anderem Vorzeichen: Sein Puffer ist
/// in GERÄTEpixeln und damit auf einem skalierten Schirm grösser als die
/// Fläche — die Eingabefläche ragte über den Kasten hinaus, und ein Klick
/// knapp daneben hätte als Treffer gezählt statt zu schliessen.
fn eingabeflaeche(
    compositor: &WlCompositor,
    surface: &WlSurface,
    breite: i32,
    hoehe: i32,
    qh: &QueueHandle<Zustand>,
) {
    let region = compositor.create_region(qh, ());
    region.add(0, 0, breite.max(1), hoehe.max(1));
    surface.set_input_region(Some(&region));
    region.destroy();
}

/// Eine leere, unverlinkte Datei der gewünschten Grösse.
///
/// Sofort nach dem Anlegen entfernt: Der offene Dateizeiger bleibt gültig,
/// und es liegt nichts herum, wenn der Prozess unsanft endet. `create_new`
/// statt `create`, damit ein fremder Name nicht überschrieben wird.
fn leere_datei(groesse: usize) -> IoResult<File> {
    use std::io::{Error, ErrorKind};

    let ordner = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);

    for versuch in 0..8u32 {
        let pfad = ordner.join(format!(
            "ai-brain-tray-menue-{}-{versuch}",
            std::process::id()
        ));

        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&pfad)
        {
            Ok(datei) => {
                let _ = std::fs::remove_file(&pfad);
                datei.set_len(groesse.max(4) as u64)?;
                return Ok(datei);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }

    Err(Error::new(
        ErrorKind::AlreadyExists,
        "kein freier Name für den Puffer",
    ))
}

// ---------------------------------------------------------------------------
// Die Wayland-Ereignisse
// ---------------------------------------------------------------------------

impl Dispatch<WlRegistry, ()> for Zustand {
    fn event(
        z: &mut Self,
        registry: &WlRegistry,
        ereignis: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = ereignis
        else {
            return;
        };

        match interface.as_str() {
            // 4 reicht; 6 brächte `preferred_buffer_scale`, und das bekommen
            // wir über die fraktionale Skalierung genauer.
            "wl_compositor" => z.compositor = Some(registry.bind(name, version.min(4), qh, ())),
            "wl_shm" => z.shm = Some(registry.bind(name, 1, qh, ())),
            "zwlr_layer_shell_v1" => {
                z.schichten = Some(registry.bind(name, version.min(4), qh, ()))
            }
            "wp_viewporter" => z.viewporter = Some(registry.bind(name, 1, qh, ())),
            "wp_fractional_scale_manager_v1" => z.bruchteile = Some(registry.bind(name, 1, qh, ())),
            // Fassung 4 wegen des `name`-Ereignisses: Daran hängt die
            // Zuordnung zu Hyprlands Monitorliste. Fassung 2 brächte nur
            // `scale`, und dann wäre die Mausstelle nicht zuzuordnen.
            "wl_output" => {
                let output: WlOutput = registry.bind(name, version.min(4), qh, ());
                z.outputs.push(output);
            }
            // Zeiger und Tastatur kommen über `Capabilities`; der Sitz
            // selbst wird danach nicht mehr gebraucht. Ein fallengelassener
            // Proxy zerstört in wayland-client nichts — dafür gibt es
            // `destroy()` —, der Griff darauf ist also entbehrlich.
            "wl_seat" => {
                let _: WlSeat = registry.bind(name, version.min(7), qh, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, ()> for Zustand {
    fn event(
        z: &mut Self,
        output: &WlOutput,
        ereignis: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ereignis {
            wl_output::Event::Scale { factor } => {
                for f in &mut z.faenger {
                    if f.output.as_ref().map(Proxy::id) == Some(output.id()) {
                        f.ganzzahlig = factor.max(1);
                    }
                }
            }

            // Der Name der Ausgabe, wie der Compositor sie nennt — bei
            // wlroots der Anschluss („HDMI-A-1"), und genau so steht er auch
            // in Hyprlands Monitorliste.
            wl_output::Event::Name { name } => {
                z.ausgabe_namen.push((output.id(), name.clone()));

                for f in &mut z.faenger {
                    if f.output.as_ref().map(Proxy::id) == Some(output.id()) {
                        f.name = Some(name.clone());
                    }
                }
            }

            _ => {}
        }
    }
}

impl Dispatch<WlSeat, ()> for Zustand {
    fn event(
        _z: &mut Self,
        sitz: &WlSeat,
        ereignis: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(was),
        } = ereignis
        else {
            return;
        };

        if was.contains(wl_seat::Capability::Pointer) {
            let _ = sitz.get_pointer(qh, ());
        }

        if was.contains(wl_seat::Capability::Keyboard) {
            let _ = sitz.get_keyboard(qh, ());
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, Wer> for Zustand {
    fn event(
        z: &mut Self,
        layer: &ZwlrLayerSurfaceV1,
        ereignis: zwlr_layer_surface_v1::Event,
        wer: &Wer,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match ereignis {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                sagen!("configure {wer:?}: {width}x{height}");
                layer.ack_configure(serial);

                match *wer {
                    Wer::Faenger(i) => {
                        let mut braucht_skalierung = false;

                        if let Some(f) = z.faenger.get_mut(i) {
                            f.breite = width as i32;
                            f.hoehe = height as i32;
                            braucht_skalierung = f.skalierung_120.is_none();
                        }

                        // Die fraktionale Skalierung erst jetzt anfragen:
                        // Vorher gibt es keine Fläche, auf die sie sich
                        // beziehen könnte.
                        if braucht_skalierung {
                            if let (Some(m), Some(surface)) = (
                                z.bruchteile.clone(),
                                z.faenger.get(i).map(|f| f.surface.clone()),
                            ) {
                                let _ = m.get_fractional_scale(&surface, qh, *wer);
                            }
                        }

                        z.faenger_fuellen(i, qh);
                    }

                    Wer::Kasten => {
                        if let Some(k) = z.kasten.as_mut() {
                            k.bestaetigt = true;
                        }

                        z.kasten_malen();
                    }
                }
            }

            zwlr_layer_surface_v1::Event::Closed => {
                sagen!("{wer:?} wurde geschlossen");

                // Der Compositor nimmt uns die Fläche weg (Ausgabe
                // abgemeldet, Sitzung endet). Dann ist das Menü zu — ohne
                // Wahl.
                z.fertig = true;
            }

            _ => {}
        }
    }
}

impl Dispatch<WpFractionalScaleV1, Wer> for Zustand {
    fn event(
        z: &mut Self,
        _proxy: &WpFractionalScaleV1,
        ereignis: wp_fractional_scale_v1::Event,
        wer: &Wer,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let wp_fractional_scale_v1::Event::PreferredScale { scale } = ereignis else {
            return;
        };

        if let Wer::Faenger(i) = *wer {
            if let Some(f) = z.faenger.get_mut(i) {
                f.skalierung_120 = Some(scale);
            }
        }
    }
}

impl Dispatch<WlPointer, ()> for Zustand {
    fn event(
        z: &mut Self,
        _zeiger: &WlPointer,
        ereignis: wl_pointer::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match ereignis {
            wl_pointer::Event::Enter {
                surface,
                surface_x,
                surface_y,
                ..
            } => {
                sagen!(
                    "enter {:?} bei {surface_x:.0},{surface_y:.0}",
                    z.wer(&surface)
                );

                let Some(wer) = z.wer(&surface) else {
                    return;
                };

                z.zeiger = Some((wer, surface_x, surface_y));
                z.zeiger_gewandert(qh);
            }

            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                if let Some((wer, _, _)) = z.zeiger {
                    z.zeiger = Some((wer, surface_x, surface_y));
                    z.zeiger_gewandert(qh);
                }
            }

            wl_pointer::Event::Leave { surface, .. } => {
                if let (Some(verlassen), Some((wo, _, _))) = (z.wer(&surface), z.zeiger) {
                    if verlassen == wo {
                        z.zeiger = None;

                        if wo == Wer::Kasten {
                            z.hervorheben(None);
                        }
                    }
                }
            }

            wl_pointer::Event::Button { state, button, .. } => {
                sagen!(
                    "taste {button} {state:?}, Zeiger auf {:?}",
                    z.zeiger.map(|(w, _, _)| w)
                );

                if state != WEnum::Value(wl_pointer::ButtonState::Pressed) {
                    return;
                }

                match entscheiden(z.zeiger, z.kasten.as_ref().map(|k| &k.aufbau)) {
                    Klick::Schliessen => z.fertig = true,
                    Klick::Waehlen => z.waehlen(),
                }
            }

            _ => {}
        }
    }
}

impl Dispatch<WlKeyboard, ()> for Zustand {
    fn event(
        z: &mut Self,
        _tastatur: &WlKeyboard,
        ereignis: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let wl_keyboard::Event::Key { key, state, .. } = ereignis else {
            sagen!("tastatur {ereignis:?}");
            return;
        };

        sagen!("taste {key} {state:?}");

        if state != WEnum::Value(wl_keyboard::KeyState::Pressed) {
            return;
        }

        match key {
            taste::ESC => z.fertig = true,
            taste::EINGABE | taste::EINGABE_ZIFFERNBLOCK => z.waehlen(),
            taste::RUNTER => {
                let neu = Aufbau::naechster_klickbarer(&z.eintraege, z.hervor, false);
                z.hervorheben(neu);
            }
            taste::HOCH => {
                let neu = Aufbau::naechster_klickbarer(&z.eintraege, z.hervor, true);
                z.hervorheben(neu);
            }
            _ => {}
        }
    }
}

// Diese Objekte schicken nichts, was uns angeht.
delegate_noop!(Zustand: ignore WlCompositor);
delegate_noop!(Zustand: ignore WlSurface);
delegate_noop!(Zustand: ignore WlRegion);
delegate_noop!(Zustand: ignore WlShm);
delegate_noop!(Zustand: ignore WlShmPool);
delegate_noop!(Zustand: ignore WlBuffer);
delegate_noop!(Zustand: ignore ZwlrLayerShellV1);
delegate_noop!(Zustand: ignore WpViewporter);
delegate_noop!(Zustand: ignore WpViewport);
delegate_noop!(Zustand: ignore WpFractionalScaleManagerV1);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menue::Aktion;

    use super::super::farben::Stil;
    use super::super::schrift::Schrift;

    fn eintraege() -> Vec<Eintrag> {
        vec![
            Eintrag::Zeile("Verbunden mit AI Brain".into()),
            Eintrag::Trenner,
            Eintrag::Haken {
                text: "Live-Leitung".into(),
                gesetzt: false,
                aktiv: true,
                aktion: Aktion::Umschalten,
            },
            Eintrag::Punkt {
                text: "Tray beenden".into(),
                aktiv: true,
                aktion: Aktion::Beenden,
            },
        ]
    }

    fn aufbau() -> Option<Aufbau> {
        let liste = eintraege();
        let stil = Stil::default();
        let mut s = Schrift::laden(stil.schriftgrad).ok()?;

        let anker = Anker {
            maus_x: 900.0,
            maus_y: 20.0,
            leiste: None,
        };

        Some(Aufbau::rechnen(
            &liste, &stil, &mut s, 1.0, anker, 1920, 1080,
        ))
    }

    /// Unsere Namensräume fangen mit dem Präfix an, nach dem
    /// `hyprland::flaeche_finden` eigene Flächen überspringt.
    ///
    /// Ohne diese Probe ist der Zusammenhang zwischen den beiden Konstanten
    /// hier und der einen dort eine Verabredung, die beim nächsten Umbenennen
    /// lautlos reisst — und dann richtet sich das Menü an seinem eigenen
    /// Auffänger aus.
    #[test]
    fn die_eigenen_namensraeume_tragen_das_praefix() {
        let praefix = hyprland::EIGENER_NAMENSRAUM;

        assert!(
            NAMENSRAUM_KASTEN.starts_with(praefix),
            "{NAMENSRAUM_KASTEN}"
        );
        assert!(
            NAMENSRAUM_FAENGER.starts_with(praefix),
            "{NAMENSRAUM_FAENGER}"
        );
    }

    /// Ein Klick auf dem Menü wählt, jeder andere schliesst.
    ///
    /// Das ist die Probe, die am echten Compositor nicht jederzeit zu fahren
    /// ist: Die Sitzung des Prüfrechners stellt Knopfereignisse nur
    /// zeitweise zu (Sperr-Failsafe). Die Entscheidung selbst hängt aber
    /// nicht am Compositor — nur die Frage, wo der Zeiger steht.
    #[test]
    fn ein_klick_daneben_schliesst_auf_beiden_wegen() {
        let Some(a) = aufbau() else {
            return;
        };

        let mitte = ((a.kasten.breite / 2) as f64, (a.kasten.hoehe / 2) as f64);

        // Auf dem Kasten: wählen.
        assert_eq!(
            entscheiden(Some((Wer::Kasten, mitte.0, mitte.1)), Some(&a)),
            Klick::Waehlen
        );

        // Auf einem Auffänger — der Weg ohne Zeigerumleitung.
        assert_eq!(
            entscheiden(Some((Wer::Faenger(0), 10.0, 10.0)), Some(&a)),
            Klick::Schliessen
        );
        assert_eq!(
            entscheiden(Some((Wer::Faenger(1), 900.0, 20.0)), Some(&a)),
            Klick::Schliessen
        );

        // Als Kastenereignis, aber ausserhalb — der Weg MIT Zeigerumleitung.
        // Die Zahlen sind die gemessenen: Hyprland schickte bei einem Klick
        // auf die Leiste `(-2818, -6)` an den Kasten.
        for (x, y) in [
            (-2818.0, -6.0),
            (-1.0, 10.0),
            (10.0, -1.0),
            (a.kasten.breite as f64, 10.0),
            (10.0, a.kasten.hoehe as f64),
            (99_999.0, 99_999.0),
        ] {
            assert_eq!(
                entscheiden(Some((Wer::Kasten, x, y)), Some(&a)),
                Klick::Schliessen,
                "({x},{y}) liegt neben dem Kasten"
            );
        }

        // Kein Zeiger, kein Kasten: auch zu. Ein Menü, das auf einen Klick
        // nicht reagiert, ist der Fehler, um den es hier geht.
        assert_eq!(entscheiden(None, Some(&a)), Klick::Schliessen);
        assert_eq!(
            entscheiden(Some((Wer::Kasten, 5.0, 5.0)), None),
            Klick::Schliessen
        );
        assert_eq!(entscheiden(None, None), Klick::Schliessen);
    }

    /// Die Ecken des Kastens gehören noch zum Kasten, einen Pixel weiter
    /// nicht mehr.
    #[test]
    fn die_grenze_laeuft_an_der_kante_des_kastens() {
        let Some(a) = aufbau() else {
            return;
        };

        let (b, h) = (a.kasten.breite as f64, a.kasten.hoehe as f64);

        for (x, y) in [
            (0.0, 0.0),
            (b - 1.0, 0.0),
            (0.0, h - 1.0),
            (b - 1.0, h - 1.0),
        ] {
            assert_eq!(
                entscheiden(Some((Wer::Kasten, x, y)), Some(&a)),
                Klick::Waehlen,
                "({x},{y}) liegt noch im Kasten"
            );
        }

        for (x, y) in [(-0.6, 0.0), (b, 0.0), (0.0, h), (b, h)] {
            assert_eq!(
                entscheiden(Some((Wer::Kasten, x, y)), Some(&a)),
                Klick::Schliessen,
                "({x},{y}) liegt schon daneben"
            );
        }
    }
}
