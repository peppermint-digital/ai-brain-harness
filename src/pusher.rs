//! Ein kleiner Pusher-Client für Reverb (#529/#530).
//!
//! **Warum von Hand und nicht als Crate.** Die Pusher-Crates sind entweder
//! nur die Server-API (Ereignisse *senden* per HTTP) oder bringen tokio mit.
//! Der Harness ist durchgehend synchron (`ureq`), und was die Leitung vom
//! Protokoll braucht, ist klein: verbinden, einen privaten Kanal abonnieren,
//! Ping beantworten, Ereignisse lesen. Senden muss sie über den Socket nichts
//! — die Ausgabe geht als POST durch AI Brain, weil nur dort Grenze und
//! Protokoll greifen.
//!
//! ## Das Protokoll, so weit es hier gebraucht wird
//!
//! ```text
//! ws(s)://host:port/app/KEY?protocol=7
//!   <- pusher:connection_established  {socket_id, activity_timeout}
//!   -> pusher:subscribe               {channel, auth}
//!   <- pusher_internal:subscription_succeeded
//!   <- geraet.eingabe                 {channel, data: "<JSON als Zeichenkette>"}
//!   <-> pusher:ping / pusher:pong
//! ```
//!
//! **`data` ist eine Zeichenkette mit JSON darin**, nicht ein Objekt — so
//! schickt Laravel jedes Ereignis. [`rahmen_lesen`] nimmt trotzdem beides,
//! damit ein Fake oder ein künftiger Server nicht an dieser Feinheit scheitert.

use std::net::TcpStream;
use std::time::{Duration, Instant};

use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// Wie lange Verbindungsaufbau und Abonnement höchstens dauern dürfen.
const FRIST_AUFBAU: Duration = Duration::from_secs(15);

/// Wie lange auf ein `pusher:pong` gewartet wird, bevor die Leitung als tot gilt.
const FRIST_PONG: Duration = Duration::from_secs(30);

/// Ein Rahmen, so weit er hier interessiert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rahmen {
    pub ereignis: String,
    pub kanal: Option<String>,
    /// Die Nutzlast als JSON-Text, gleich ob sie als Zeichenkette oder als
    /// Objekt kam.
    pub daten: String,
}

/// Liest einen Pusher-Rahmen. `None` für alles, was kein Rahmen ist.
pub fn rahmen_lesen(text: &str) -> Option<Rahmen> {
    let wert: serde_json::Value = serde_json::from_str(text).ok()?;
    let ereignis = wert.get("event")?.as_str()?.to_string();
    let kanal = wert
        .get("channel")
        .and_then(|k| k.as_str())
        .map(str::to_string);

    let daten = match wert.get("data") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Null) | None => "{}".to_string(),
        Some(anders) => anders.to_string(),
    };

    Some(Rahmen {
        ereignis,
        kanal,
        daten,
    })
}

/// Die Adresse des Sockets.
///
/// `client` und `version` sind für Reverb nur Auskunft; `protocol=7` ist die
/// Fassung, die pusher-js heute spricht.
pub fn url(scheme: &str, host: &str, port: u16, app_key: &str) -> String {
    let ws = if scheme == "https" { "wss" } else { "ws" };

    format!(
        "{ws}://{host}:{port}/app/{app_key}?protocol=7&client=ai-brain-harness&version={}",
        env!("CARGO_PKG_VERSION")
    )
}

/// Eine offene Verbindung.
pub struct Verbindung {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    pub socket_id: String,
    /// Nach so viel Stille fragt der Client selbst nach (`activity_timeout`).
    stille: Duration,
    zuletzt: Instant,
    ping_seit: Option<Instant>,
}

impl Verbindung {
    /// Verbindet und wartet auf `pusher:connection_established`.
    pub fn oeffnen(adresse: &str) -> Result<Self, String> {
        let (ws, _) = tungstenite::connect(adresse).map_err(|e| {
            format!(
                "Reverb unter {} nicht erreichbar: {e}",
                ohne_abfrage(adresse)
            )
        })?;

        let mut verbindung = Verbindung {
            ws,
            socket_id: String::new(),
            stille: Duration::from_secs(120),
            zuletzt: Instant::now(),
            ping_seit: None,
        };

        // Kurz, weil die Leitung zwischen zwei Lesungen auch ihre eigene Uhr
        // braucht: Lücken in der Eingabe werden nach 300 ms übersprungen.
        verbindung.lesefrist(Duration::from_millis(100))?;

        let rahmen = verbindung.warten_auf(|r| {
            r.ereignis == "pusher:connection_established" || r.ereignis == "pusher:error"
        })?;

        if rahmen.ereignis == "pusher:error" {
            return Err(format!("Reverb lehnt die Verbindung ab: {}", rahmen.daten));
        }

        let daten: serde_json::Value = serde_json::from_str(&rahmen.daten)
            .map_err(|e| format!("connection_established ist unlesbar: {e}"))?;

        verbindung.socket_id = daten
            .get("socket_id")
            .and_then(|s| s.as_str())
            .ok_or("connection_established ohne socket_id")?
            .to_string();

        // Reverb nennt 30 s; ein Wert, der fehlt oder unsinnig klein ist, darf
        // keinen Ping-Sturm auslösen.
        if let Some(sek) = daten.get("activity_timeout").and_then(|s| s.as_u64()) {
            verbindung.stille = Duration::from_secs(sek.clamp(10, 600));
        }

        Ok(verbindung)
    }

    /// Abonniert einen privaten Kanal mit einer fertigen Signatur.
    pub fn abonnieren(&mut self, kanal: &str, auth: &str) -> Result<(), String> {
        self.senden(serde_json::json!({
            "event": "pusher:subscribe",
            "data": { "channel": kanal, "auth": auth },
        }))?;

        let rahmen = self.warten_auf(|r| {
            r.ereignis == "pusher_internal:subscription_succeeded"
                || r.ereignis == "pusher:subscription_error"
                || r.ereignis == "pusher:error"
        })?;

        if rahmen.ereignis != "pusher_internal:subscription_succeeded" {
            return Err(format!(
                "Abonnement von {kanal} abgelehnt: {}",
                rahmen.daten
            ));
        }

        Ok(())
    }

    /// Der nächste Rahmen, der kein Protokoll-Rauschen ist — oder `None`, wenn
    /// innerhalb der Lesefrist nichts kam.
    ///
    /// Ping und Pong werden hier erledigt. Ein `Err` heisst: diese Verbindung
    /// ist tot, eine neue aufbauen.
    pub fn naechster(&mut self) -> Result<Option<Rahmen>, String> {
        if let Some(seit) = self.ping_seit {
            if seit.elapsed() > FRIST_PONG {
                return Err("Reverb antwortet nicht mehr auf Ping.".to_string());
            }
        } else if self.zuletzt.elapsed() > self.stille {
            self.senden(serde_json::json!({ "event": "pusher:ping", "data": {} }))?;
            self.ping_seit = Some(Instant::now());
        }

        let Some(rahmen) = self.lesen()? else {
            return Ok(None);
        };

        match rahmen.ereignis.as_str() {
            "pusher:ping" => {
                self.senden(serde_json::json!({ "event": "pusher:pong", "data": {} }))?;
                Ok(None)
            }
            "pusher:pong" => Ok(None),
            "pusher:error" => {
                // 4000–4099: nicht wiederholen; alles andere: neu verbinden.
                // Beides ist für die Leitung dasselbe — die Verbindung ist weg.
                Err(format!("Reverb meldet einen Fehler: {}", rahmen.daten))
            }
            _ => Ok(Some(rahmen)),
        }
    }

    /// Ordentlich auflegen. Fehler dabei sind egal — es ist ohnehin das Ende.
    pub fn schliessen(mut self) {
        let _ = self.ws.close(None);
        let _ = self.ws.flush();
    }

    fn lesefrist(&mut self, frist: Duration) -> Result<(), String> {
        let tcp = match self.ws.get_mut() {
            MaybeTlsStream::Plain(s) => s,
            MaybeTlsStream::Rustls(s) => s.get_mut(),
            _ => return Err("Unbekannte Socket-Art.".to_string()),
        };

        tcp.set_read_timeout(Some(frist))
            .map_err(|e| format!("Lesefrist nicht setzbar: {e}"))
    }

    fn senden(&mut self, wert: serde_json::Value) -> Result<(), String> {
        self.ws
            .send(Message::text(wert.to_string()))
            .map_err(|e| format!("Senden an Reverb scheiterte: {e}"))
    }

    /// Ein Rahmen oder `None` nach Ablauf der Lesefrist.
    fn lesen(&mut self) -> Result<Option<Rahmen>, String> {
        match self.ws.read() {
            Ok(Message::Text(text)) => {
                self.zuletzt = Instant::now();
                self.ping_seit = None;
                Ok(rahmen_lesen(text.as_str()))
            }
            Ok(Message::Close(_)) => Err("Reverb hat die Verbindung geschlossen.".to_string()),
            Ok(_) => {
                // WebSocket-Ping/Pong beantwortet tungstenite selbst; auch das
                // ist ein Lebenszeichen.
                self.zuletzt = Instant::now();
                Ok(None)
            }
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(format!("Lesen von Reverb scheiterte: {e}")),
        }
    }

    fn warten_auf(&mut self, passt: impl Fn(&Rahmen) -> bool) -> Result<Rahmen, String> {
        let beginn = Instant::now();

        while beginn.elapsed() < FRIST_AUFBAU {
            if let Some(rahmen) = self.lesen()? {
                if passt(&rahmen) {
                    return Ok(rahmen);
                }
            }
        }

        Err(format!(
            "Reverb hat binnen {} s nicht geantwortet.",
            FRIST_AUFBAU.as_secs()
        ))
    }
}

/// Die Adresse ohne Abfrageteil — für Meldungen.
fn ohne_abfrage(adresse: &str) -> &str {
    adresse.split('?').next().unwrap_or(adresse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daten_als_zeichenkette_wie_laravel() {
        let r = rahmen_lesen(
            r#"{"event":"geraet.eingabe","channel":"private-geraet.7.eingabe","data":"{\"art\":\"erstbild\"}"}"#,
        )
        .unwrap();

        assert_eq!(r.ereignis, "geraet.eingabe");
        assert_eq!(r.kanal.as_deref(), Some("private-geraet.7.eingabe"));
        assert_eq!(r.daten, r#"{"art":"erstbild"}"#);
    }

    #[test]
    fn daten_als_objekt_gehen_auch() {
        let r = rahmen_lesen(r#"{"event":"pusher:ping","data":{}}"#).unwrap();
        assert_eq!(r.daten, "{}");
        assert_eq!(r.kanal, None);
    }

    #[test]
    fn kein_rahmen() {
        assert_eq!(rahmen_lesen("kaputt"), None);
        assert_eq!(rahmen_lesen(r#"{"data":"x"}"#), None);
    }

    #[test]
    fn adresse_folgt_dem_schema() {
        assert!(url("https", "brain.example", 443, "k")
            .starts_with("wss://brain.example:443/app/k?protocol=7"));
        assert!(url("http", "127.0.0.1", 8080, "k").starts_with("ws://127.0.0.1:8080/app/k?"));
    }
}
