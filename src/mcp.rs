//! MCP-Server über stdio.
//!
//! Claude Code startet diesen Prozess und spricht JSON-RPC über stdin/stdout.
//! Wir reichen jede Nachricht an den MCP-Endpunkt der Instanz weiter und geben
//! die Antwort zurück — mit einem gültigen Token davor.
//!
//! Das ist der Teil, der den bisherigen Sonderweg ersetzt: Bisher hing dieser
//! Rechner über `php artisan mcp:start brain` am Repo-Verzeichnis, ohne
//! OAuth, ohne Gerät, nicht widerrufbar und nur hier lauffähig. Jetzt geht
//! jeder Rechner denselben Weg über die Domain.
//!
//! Zwei Dinge sind hier heikel:
//!
//! * **stdout gehört dem Protokoll.** Jede Ausgabe, die kein JSON-RPC ist,
//!   bricht die Verbindung. Alle Meldungen gehen deshalb nach stderr.
//! * **Benachrichtigungen haben keine Antwort.** JSON-RPC-Nachrichten ohne
//!   `id` sind Notifications; auf sie darf nichts zurückgeschrieben werden,
//!   sonst wartet die Gegenseite auf eine Antwort, die es nicht gibt.

use crate::config::{self, Connection};
use serde::Deserialize;
use std::io::{BufRead, Read, Write};
use std::time::{Duration, Instant};

/// Frist fuer einen Werkzeugaufruf an die Instanz.
///
/// **Ohne Frist legt eine festgefahrene Verbindung die ganze Sitzung lahm.**
/// Dieser Server arbeitet stdin streng der Reihe nach ab: haengt eine Anfrage,
/// steht alles dahinter an. Am 16.10.2026 genau so erlebt — der MCP-Server
/// einer 25 Stunden alten Sitzung wartete mit vier offenen Sockets, und jeder
/// weitere Werkzeugaufruf lief ins Leere, waehrend derselbe Aufruf durch einen
/// frischen Prozess in 0,4 s antwortete. Von aussen sah es aus, als sei die
/// Instanz krank; sie war es nicht.
///
/// Grosszuegig gewaehlt: `get-context-tool` liefert sechsstellige Zeichenmengen,
/// und ein Abbruch mitten in einer echten Antwort waere schlimmer als Warten.
/// Entscheidend ist nicht die Hoehe, sondern dass es ueberhaupt eine gibt.
const FRIST_WERKZEUG: Duration = Duration::from_secs(180);

/// Frist fuer die Token-Beschaffung. Kurz: das ist ein kleiner Austausch, und
/// ohne Token geht ohnehin nichts weiter.
const FRIST_TOKEN: Duration = Duration::from_secs(20);

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default = "default_expiry")]
    expires_in: u64,
}

fn default_expiry() -> u64 {
    3600
}

struct TokenCache {
    value: String,
    valid_until: Instant,
    /// Laufzeit laut Instanz, damit die Ablage auf der Platte dieselbe Frist
    /// benutzt wie der Speicher — sonst laufen zwei Regeln nebeneinander.
    laufzeit: u64,
}

pub fn serve() -> Result<(), String> {
    let connection = config::load()?;

    eprintln!(
        "ai-brain-harness: verbunden mit {} als Gerät „{}“",
        connection.base_url, connection.device_name
    );

    let mut cache: Option<TokenCache> = None;

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("stdin nicht lesbar: {e}"))?;
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        let message: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(e) => {
                eprintln!("ai-brain-harness: ungültiges JSON von Claude Code übersprungen ({e})");
                continue;
            }
        };

        let is_notification = message.get("id").is_none();

        // **Eigene Werkzeuge VOR dem Token.** Der Tunnel hat mit AI Brain
        // nichts zu tun: er oeffnet den Browser des Menschen auf dessen
        // Rechner. Wer ihn erst nach einem Token-Abruf bedient, macht ihn von
        // etwas abhaengig, das er nicht braucht — und unbrauchbar, wenn die
        // Instanz gerade nicht erreichbar ist.
        if let Some(antwort) = eigenes_werkzeug(&message) {
            if !is_notification {
                writeln!(stdout, "{antwort}")
                    .map_err(|e| format!("stdout nicht beschreibbar: {e}"))?;
                stdout.flush().map_err(|e| format!("stdout nicht leerbar: {e}"))?;
            }

            continue;
        }

        let token = match token(&connection, &mut cache) {
            Ok(token) => token,
            Err(err) => {
                eprintln!("ai-brain-harness: {err}");

                if !is_notification {
                    respond_with_error(&mut stdout, &message, err.meldung())?;
                }

                continue;
            }
        };

        match forward(&connection, &token, trimmed) {
            Ok(Some(body)) => {
                if !is_notification {
                    let body = mit_eigenen_werkzeugen(&message, body);

                    writeln!(stdout, "{body}").map_err(|e| format!("stdout nicht beschreibbar: {e}"))?;
                    stdout.flush().map_err(|e| format!("stdout nicht leerbar: {e}"))?;
                }
            }
            Ok(None) => {
                // Leere Antwort — bei Notifications der Normalfall.
            }
            Err(err) => {
                eprintln!("ai-brain-harness: {err}");

                if !is_notification {
                    respond_with_error(&mut stdout, &message, &err)?;
                }
            }
        }
    }

    Ok(())
}

/// Einmalig einen Token holen — für Aufrufe ausserhalb der Dauerschleife
/// (etwa die Synchronisation), die keinen Zwischenspeicher brauchen.
/// Abstand zum Ablauf, in Sekunden.
///
/// Ein Token, das in einer Sekunde abläuft, ist für einen Aufruf, der gerade
/// beginnt, schon zu alt — und die Uhren von Rechner und Server stimmen nie
/// genau.
const TOKEN_ABSTAND: i64 = 120;

#[derive(serde::Serialize, serde::Deserialize)]
struct TokenAblage {
    access_token: String,
    gueltig_bis: i64,
}

fn token_pfad() -> Option<std::path::PathBuf> {
    config::config_dir().ok().map(|d| d.join("token.json"))
}

/// Bis wann ein Token als gültig gelten soll.
///
/// Eigene Funktion, damit die Regel prüfbar ist, ohne eine Uhr zu stellen.
fn ablauf(jetzt: i64, laufzeit: i64) -> i64 {
    jetzt + (laufzeit - TOKEN_ABSTAND).max(0)
}

fn jetzt() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn token_aus_ablage() -> Option<String> {
    let roh = std::fs::read_to_string(token_pfad()?).ok()?;
    let ablage: TokenAblage = serde_json::from_str(&roh).ok()?;

    (ablage.gueltig_bis > jetzt()).then_some(ablage.access_token)
}

fn token_ablegen(token: &str, laufzeit: u64) -> Option<()> {
    let pfad = token_pfad()?;

    let inhalt = serde_json::to_string(&TokenAblage {
        access_token: token.to_string(),
        gueltig_bis: ablauf(jetzt(), laufzeit as i64),
    })
    .ok()?;

    std::fs::write(&pfad, inhalt).ok()?;

    // Die Datei trägt ein Bearer-Token mit einer Stunde Laufzeit. Das Geheimnis
    // daneben ist stärker, aber das ist kein Grund, dieses offen zu lassen.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&pfad, std::fs::Permissions::from_mode(0o600));
    }

    Some(())
}

/// Ein abgelehntes Token aus der Ablage werfen.
///
/// Ohne das blieb ein serverseitig ungültig gewordenes Token bis zu einer
/// Stunde liegen und JEDER Aufruf scheiterte — mit einer Meldung, die nach
/// einem Fehler der Instanz aussieht.
pub fn token_verwerfen() {
    if let Some(pfad) = token_pfad() {
        let _ = std::fs::remove_file(pfad);
    }
}

/// Warum ein Zugangstoken nicht zu haben war.
///
/// **Die beiden Fälle sind nicht derselbe Befund, und ein Mensch liest sie
/// verschieden.** „Der Zugang ist weg" heisst: neu koppeln. „Die Instanz
/// antwortet nicht" heisst: warten oder ans Netz denken. Solange beides eine
/// Zeichenkette war, musste jeder Aufrufer sie nach Wörtern durchsuchen, um
/// das eine vom anderen zu unterscheiden — und eine umformulierte Meldung
/// hätte die Unterscheidung still gekippt (#9059).
pub enum TokenFehler {
    /// Die Instanz weist den Gerätezugang ab (401 am Token-Endpunkt). Entzogen
    /// unter „Meine Geräte", oder das Geheimnis stimmt nicht mehr.
    Abgewiesen(String),
    /// Alles andere: kein Netz, Zeitüberschreitung, unlesbare Antwort.
    Sonst(String),
}

impl TokenFehler {
    pub fn meldung(&self) -> &str {
        match self {
            TokenFehler::Abgewiesen(m) | TokenFehler::Sonst(m) => m,
        }
    }
}

impl std::fmt::Display for TokenFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.meldung())
    }
}

/// Ein Zugangstoken für einen einzelnen Befehl.
///
/// **Über Prozessgrenzen zwischengespeichert, nicht nur im Arbeitsspeicher.**
/// Der Speicher in {@link serve} hilft nur dem langlebigen Proxy; ein kurzer
/// Aufruf startet, prägt und endet. Ein Abholer, der alle 30 Sekunden ruft,
/// erzeugte damit 120 Token pro Stunde — am 14.10.2026 gemessen: 600 bis 760
/// pro Stunde auf diesem Rechner, weil mehrere Abholer liefen. Jede Prägung
/// kostet die Instanz knapp eine halbe Sekunde (#7050).
pub fn access_token(connection: &Connection) -> Result<String, String> {
    access_token_typisiert(connection).map_err(|e| e.meldung().to_string())
}

/// Dasselbe, mit dem Grund als Typ statt als Satz.
///
/// Für Aufrufer, die „abgemeldet" von „nicht erreichbar" unterscheiden müssen —
/// `status` tut das (#9059).
pub fn access_token_typisiert(connection: &Connection) -> Result<String, TokenFehler> {
    if let Some(token) = token_aus_ablage() {
        return Ok(token);
    }

    let mut cache = None;
    let wert = token(connection, &mut cache)?;

    // Scheitert das Ablegen, ist das kein Grund, den Aufruf abzubrechen — es
    // kostet nur den nächsten Aufruf eine Prägung.
    token_ablegen(&wert, cache.as_ref().map_or(3600, |c| c.laufzeit));

    Ok(wert)
}

/// Holt einen Token und behält ihn, bis er fast abgelaufen ist.
///
/// Der Puffer von 60 Sekunden ist kein Zierrat: ohne ihn läuft ein Token
/// zwischen Prüfung und Ankunft beim Server ab, und der Aufruf scheitert mit
/// einem 401, für den es keine erkennbare Ursache gibt.
fn token(
    connection: &Connection,
    cache: &mut Option<TokenCache>,
) -> Result<String, TokenFehler> {
    if let Some(cached) = cache {
        if Instant::now() < cached.valid_until {
            return Ok(cached.value.clone());
        }
    }

    let response = ureq::post(&connection.token_endpoint)
        .timeout(FRIST_TOKEN)
        .set("Accept", "application/json")
        .send_form(&[
            ("grant_type", "client_credentials"),
            ("client_id", &connection.client_id),
            ("client_secret", &connection.client_secret),
            ("scope", &connection.scope),
        ])
        .map_err(|e| match e {
            ureq::Error::Status(401, _) => TokenFehler::Abgewiesen(format!(
                "Der Gerätezugang wurde abgelehnt. Wurde er unter „Meine Geräte“ entzogen? \
                 Dann neu koppeln: ai-brain-harness connect {}",
                connection.base_url
            )),
            other => TokenFehler::Sonst(format!("Token nicht erhältlich: {other}")),
        })?;

    let token: TokenResponse = response
        .into_json()
        .map_err(|e| TokenFehler::Sonst(format!("Token-Antwort ist unlesbar: {e}")))?;

    let lifetime = Duration::from_secs(token.expires_in.saturating_sub(60).max(30));

    *cache = Some(TokenCache {
        value: token.access_token.clone(),
        valid_until: Instant::now() + lifetime,
        laufzeit: token.expires_in,
    });

    Ok(token.access_token)
}

/// Ruft ein Werkzeug der Instanz auf und gibt dessen Nutzlast als JSON zurück.
///
/// Der `mcp`-Unterbefehl reicht fremde Anfragen nur durch; hier formt der
/// ai-brain-harness selbst eine. Beides läuft über denselben Kanal und dieselbe
/// Anmeldung — es gibt genau einen Weg zur Instanz, und der ist MCP.
///
/// Ein `initialize` ist nicht nötig: Der HTTP-Weg ist zustandslos, jede
/// Anfrage trägt ihre Anmeldung selbst.
pub fn call_tool(
    connection: &Connection,
    token: &str,
    name: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    })
    .to_string();

    let body = forward(connection, token, &payload)?
        .ok_or_else(|| format!("Die Instanz hat auf {name} nichts geantwortet."))?;

    let antwort: serde_json::Value = serde_json::from_str(&nutzlast(&body))
        .map_err(|e| format!("Antwort auf {name} ist unlesbar: {e}"))?;

    if let Some(fehler) = antwort.get("error") {
        let grund = fehler
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("kein Grund genannt");

        return Err(format!("Die Instanz lehnt {name} ab: {grund}"));
    }

    let text = antwort
        .pointer("/result/content/0/text")
        .and_then(|t| t.as_str())
        .ok_or_else(|| format!("Antwort auf {name} hat keinen Inhalt."))?;

    // Ein abgelehnter Aufruf kommt als gewöhnliche Antwort mit `isError` —
    // der Text ist dann eine Meldung, kein JSON. Ohne diese Abzweigung
    // scheiterte er als "unlesbares JSON" und die eigentliche Begründung
    // (etwa: das Gerät gehört jemand anderem) ginge verloren.
    if antwort.pointer("/result/isError").and_then(|f| f.as_bool()) == Some(true) {
        return Err(text.to_string());
    }

    serde_json::from_str(text).map_err(|e| format!("Inhalt von {name} ist kein JSON: {e} — {text}"))
}

/// Wie `call_tool`, gibt aber den Text zurück statt ihn als JSON zu lesen.
///
/// Für Werkzeuge, die bewusst Prosa liefern (`get-task-tool` und Verwandte).
/// Das ist **kein** Bruch der Regel „ein Programm liest keine Prosa": Der Text
/// wird hier nicht zerlegt, sondern an ein Sprachmodell weitergegeben — und
/// dafür ist er gemacht. Wer aus so einer Antwort Felder herausziehen will,
/// braucht ein Bundle-Werkzeug, nicht diese Funktion.
pub fn call_tool_text(
    connection: &Connection,
    token: &str,
    name: &str,
    arguments: serde_json::Value,
) -> Result<String, String> {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    })
    .to_string();

    let body = forward(connection, token, &payload)?
        .ok_or_else(|| format!("Die Instanz hat auf {name} nichts geantwortet."))?;

    let antwort: serde_json::Value = serde_json::from_str(&nutzlast(&body))
        .map_err(|e| format!("Antwort auf {name} ist unlesbar: {e}"))?;

    if let Some(fehler) = antwort.get("error") {
        let grund = fehler
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("kein Grund genannt");

        return Err(format!("Die Instanz lehnt {name} ab: {grund}"));
    }

    let text = antwort
        .pointer("/result/content/0/text")
        .and_then(|t| t.as_str())
        .ok_or_else(|| format!("Antwort auf {name} hat keinen Inhalt."))?;

    if antwort.pointer("/result/isError").and_then(|f| f.as_bool()) == Some(true) {
        return Err(text.to_string());
    }

    Ok(text.to_string())
}

/// Zieht aus der Antwort das JSON heraus.
///
/// Die Instanz darf laut Protokoll auch als Ereignisstrom antworten
/// (`text/event-stream`). Dann steht die Nutzlast hinter `data:`, und der Rest
/// sind Rahmenzeilen, an denen ein JSON-Parser scheitert.
fn nutzlast(body: &str) -> String {
    if !body.lines().any(|z| z.starts_with("data:")) {
        return body.trim().to_string();
    }

    body.lines()
        .filter_map(|z| z.strip_prefix("data:"))
        .map(str::trim)
        .rfind(|z| !z.is_empty())
        .unwrap_or("")
        .to_string()
}

fn forward(connection: &Connection, token: &str, payload: &str) -> Result<Option<String>, String> {
    let response = ureq::post(&connection.mcp_url)
        .timeout(FRIST_WERKZEUG)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .set("Accept", "application/json, text/event-stream")
        .send_string(payload)
        .map_err(|e| match e {
            ureq::Error::Status(code, resp) => {
                // Ein abgelehnter Token gehört nicht in die Ablage. Sonst
                // scheitert jeder weitere Aufruf bis zum Ablauf an demselben
                // Token, und die Meldung zeigt auf die Instanz.
                if code == 401 {
                    token_verwerfen();
                }

                let body = resp.into_string().unwrap_or_default();
                format!("Instanz antwortet mit HTTP {code}: {}", body.trim())
            }
            other => format!("Instanz nicht erreichbar: {other}"),
        })?;

    // NICHT `into_string()`: Das deckelt bei 10 MB und bricht mit „response too
    // big" ab. Ein Eintrag mit Binäranhang liegt darüber — `kami` bringt eine
    // 9 MB grosse ZIP mit, die als Base64 auf rund 12 MB wächst. Der Deckel
    // träfe damit ausgerechnet den Fall, für den die Base64-Kodierung überhaupt
    // eingeführt wurde.
    //
    // Die Obergrenze bleibt, aber grosszügig: ohne jede Grenze könnte eine
    // fehlgeleitete Antwort den Arbeitsspeicher füllen.
    const OBERGRENZE: u64 = 256 * 1024 * 1024;

    let mut body = String::new();
    response
        .into_reader()
        .take(OBERGRENZE)
        .read_to_string(&mut body)
        .map_err(|e| format!("Antwort der Instanz ist unlesbar: {e}"))?;

    let trimmed = body.trim();

    if trimmed.is_empty() {
        return Ok(None);
    }

    Ok(Some(trimmed.to_string()))
}

/// Gehoert dieser Aufruf einem Werkzeug, das der Harness SELBST bedient?
///
/// Nur `tools/call`, und nur fuer die Namen, die `tunnel::fuer_mcp` kennt.
/// Alles andere geht unveraendert an die Instanz — dieser Server ist ein Proxy
/// und soll es bleiben.
fn eigenes_werkzeug(message: &serde_json::Value) -> Option<String> {
    if message.get("method").and_then(|m| m.as_str()) != Some("tools/call") {
        return None;
    }

    let name = message.pointer("/params/name")?.as_str()?;

    let argumente = message
        .pointer("/params/arguments")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    let ergebnis = crate::tunnel::fuer_mcp(name, &argumente)?;
    let id = message.get("id").cloned().unwrap_or(serde_json::Value::Null);

    let (text, fehler) = match ergebnis {
        Ok(text) => (text, false),
        Err(grund) => (grund, true),
    };

    Some(
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{ "type": "text", "text": text }],
                "isError": fehler,
            },
        })
        .to_string(),
    )
}

/// Die eigenen Werkzeuge in eine `tools/list`-Antwort einfuegen.
///
/// Schlaegt das Anreichern fehl — unerwartete Antwortform, kaputtes JSON —,
/// geht die Antwort der Instanz **unveraendert** durch. Eine Werkzeugliste
/// halb zu ersetzen waere schlimmer als eine ohne unsere drei.
fn mit_eigenen_werkzeugen(message: &serde_json::Value, body: String) -> String {
    if message.get("method").and_then(|m| m.as_str()) != Some("tools/list") {
        return body;
    }

    // Der Ereignisstrom (`data:`-Zeilen) wird nicht angefasst: dort muesste die
    // Rahmung mitgeschrieben werden, und das ist dieser Stelle nicht wert.
    if body.lines().any(|z| z.starts_with("data:")) {
        return body;
    }

    let Ok(mut antwort) = serde_json::from_str::<serde_json::Value>(&body) else {
        return body;
    };

    let Some(liste) = antwort
        .pointer_mut("/result/tools")
        .and_then(|t| t.as_array_mut())
    else {
        return body;
    };

    liste.extend(crate::tunnel::werkzeuge());

    antwort.to_string()
}

fn respond_with_error(
    stdout: &mut std::io::Stdout,
    request: &serde_json::Value,
    message: &str,
) -> Result<(), String> {
    let error = serde_json::json!({
        "jsonrpc": "2.0",
        "id": request.get("id").cloned().unwrap_or(serde_json::Value::Null),
        "error": {
            "code": -32000,
            "message": message,
        }
    });

    writeln!(stdout, "{error}").map_err(|e| format!("stdout nicht beschreibbar: {e}"))?;
    stdout.flush().map_err(|e| format!("stdout nicht leerbar: {e}"))?;

    Ok(())
}

/// Fragt die Instanz, welche Werkzeuge sie anbietet.
///
/// **Wofuer.** Der eigene Loop muss dem Modell die Werkzeuge beschreiben, und
/// die Beschreibungen gehoeren der Instanz — nicht einer Liste hier im Harness.
/// Eine solche Liste waere ab dem naechsten neuen Werkzeug in AI Brain falsch,
/// und zwar stillschweigend: das Modell wuesste nichts davon.
pub fn tools_list(
    connection: &Connection,
    token: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
    })
    .to_string();

    let body = forward(connection, token, &payload)?
        .ok_or_else(|| "Die Instanz hat auf tools/list nichts geantwortet.".to_string())?;

    let antwort: serde_json::Value = serde_json::from_str(&nutzlast(&body))
        .map_err(|e| format!("Antwort auf tools/list ist unlesbar: {e}"))?;

    if let Some(fehler) = antwort.get("error") {
        let grund = fehler
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("kein Grund genannt");

        return Err(format!("Die Instanz lehnt tools/list ab: {grund}"));
    }

    Ok(antwort
        .pointer("/result/tools")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::{ablauf, TOKEN_ABSTAND};

    #[test]
    fn ablauf_zieht_den_abstand_ab_und_wird_nie_negativ() {
        // Eine Stunde Laufzeit, zwei Minuten Abstand: 58 Minuten gueltig.
        assert_eq!(ablauf(1_000, 3600), 1_000 + 3600 - TOKEN_ABSTAND);

        // Eine Laufzeit KUERZER als der Abstand darf nicht in die
        // Vergangenheit zeigen — sonst gilt das Token nie und jeder Aufruf
        // praegt neu, also genau der Zustand, den die Ablage behebt.
        assert_eq!(ablauf(1_000, 60), 1_000);
        assert_eq!(ablauf(1_000, 0), 1_000);

        // Und die Grenze selbst: genau der Abstand ergibt sofort ablaufend,
        // nicht negativ.
        assert_eq!(ablauf(1_000, TOKEN_ABSTAND), 1_000);
    }

    use super::nutzlast;

    /// `nutzlast` war bis zum 14.10.2026 ungetestet — aufgefallen beim
    /// Aufräumen eines clippy-Befundes (`filter(..).last()` → `rfind`). Ein
    /// Umbau an einer Parse-Funktion ohne Test ist eine Wette; hier ist der
    /// Test, der sie unnötig macht.
    #[test]
    fn nutzlast_nimmt_das_letzte_datenfeld() {
        // Der Normalfall: eine gewöhnliche JSON-Antwort ohne SSE-Rahmen.
        assert_eq!(nutzlast("  {\"ok\":true}  "), "{\"ok\":true}");

        // SSE: Es zählt das LETZTE nicht-leere `data:` — bei MCP über HTTP
        // kommt die Antwort als Ereignisstrom, und davor können Kommentar-
        // und Leerzeilen stehen.
        let strom = ": ping\n\
                     event: message\n\
                     data: {\"erste\":1}\n\
                     \n\
                     data:\n\
                     data: {\"letzte\":2}\n\
                     \n";
        assert_eq!(nutzlast(strom), "{\"letzte\":2}");

        // Nur leere Datenfelder: leere Zeichenkette, kein Absturz.
        assert_eq!(nutzlast("data:\ndata:   \n"), "");

        // Ein einzelnes Feld ohne Leerzeichen hinter dem Doppelpunkt.
        assert_eq!(nutzlast("data:{\"a\":1}"), "{\"a\":1}");
    }
}

#[cfg(test)]
mod proxy_tests {
    use super::*;

    #[test]
    fn nur_tools_call_wird_abgefangen() {
        // Alles andere ist Sache der Instanz. Dieser Server ist ein Proxy.
        for methode in ["tools/list", "initialize", "ping"] {
            let m = serde_json::json!({"jsonrpc":"2.0","id":1,"method":methode});

            assert!(eigenes_werkzeug(&m).is_none(), "{methode}");
        }
    }

    #[test]
    fn ein_fremdes_werkzeug_geht_an_die_instanz() {
        let m = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "list-tasks-tool", "arguments": {} },
        });

        assert!(eigenes_werkzeug(&m).is_none());
    }

    #[test]
    fn die_werkzeugliste_wird_angereichert() {
        let m = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
        let koerper = serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "tools": [{ "name": "list-tasks-tool" }] },
        })
        .to_string();

        let neu = mit_eigenen_werkzeugen(&m, koerper);
        let wert: serde_json::Value = serde_json::from_str(&neu).expect("lesbar");
        let namen: Vec<&str> = wert["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();

        assert!(namen.contains(&"list-tasks-tool"), "die Instanz bleibt drin");
        assert!(namen.contains(&"tunnel-start"));
        assert_eq!(namen.len(), 4);
    }

    #[test]
    fn eine_unerwartete_antwort_geht_unveraendert_durch() {
        // **Halb ersetzen waere schlimmer als nicht anreichern.** Wer hier bei
        // einer unbekannten Form etwas zurueckgibt, kann eine ganze
        // Werkzeugliste verschlucken.
        let m = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});

        for koerper in [
            "kein json".to_string(),
            serde_json::json!({"result": {}}).to_string(),
            serde_json::json!({"error": {"message":"nein"}}).to_string(),
            "data: {\"result\":{\"tools\":[]}}\n".to_string(),
        ] {
            assert_eq!(
                mit_eigenen_werkzeugen(&m, koerper.clone()),
                koerper,
                "unveraendert durchlassen"
            );
        }
    }

    #[test]
    fn eine_andere_methode_wird_nicht_angereichert() {
        let m = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call"});
        let koerper = serde_json::json!({"result": {"tools": []}}).to_string();

        assert_eq!(mit_eigenen_werkzeugen(&m, koerper.clone()), koerper);
    }
}
