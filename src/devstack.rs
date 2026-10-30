//! DevStacks ohne Browser steuern (#9135).
//!
//! Bis hierher liess sich ein lokaler Dev-Stack nur ueber den Knopf unter
//! `/command-center/development` starten — also nur mit angemeldetem Browser.
//! Mehrere Agent-Sessions teilen sich aber EIN Fenster auf dem Rechner des
//! Nutzers, dort springt der aktive Tab, und eine Session, die ihren
//! UI-Nachweis erbringen wollte, kam deshalb nicht an ihren Stack.
//!
//! **Die Antwort wird durchgereicht, nicht neu gebaut.** Adresse, Port,
//! Dienste und die `docker ps`-Gegenprobe formuliert `devstack-tool` in der
//! Instanz; hier wird sie nur ausgegeben. Ein zweiter Ort, der dieselbe
//! Auskunft beschreibt, ist ein zweiter Ort, der von ihr abweichen kann —
//! dasselbe Muster wie bei `aufgaben`.
//!
//! Der Stack laeuft immer auf dem Rechner der INSTANZ, nicht auf diesem. Das
//! ist kein Mangel, sondern der Punkt: Docker, Adresse und Reverse Proxy
//! liegen dort, und genau deshalb geht die Steuerung durch die Instanz.

use crate::config;
use crate::mcp;

const HILFE: &str = "\
ai-brain-harness devstack <befehl> [projekt]

  status <projekt>   Laeuft der Stack? Mit Adresse, Port und docker-ps-Gegenprobe
  start <projekt>    Stack hochfahren (und nachsehen, dass DIESER hochkam)
  stop <projekt>     Stack herunterfahren; die Datenbank des Stacks bleibt
  liste              Was startbar ist und was gerade laeuft

Das Projekt ist der Codebase-Slug, z. B. peppermint-crm.

Vor dem Stoppen in die Laufzeit sehen: Auf dem Rechner der Instanz arbeiten
mehrere Sessions gleichzeitig. Ein Stack, der seit einer halben Stunde laeuft,
gehoert sehr wahrscheinlich jemand anderem.

Anmelden im Stack danach ohne Passwort (auf dem Rechner der Instanz):
  docker exec -e HOME=/tmp devstack-<projekt>-php-1 php artisan dev:login-link
";

/// `devstack status|start|stop|liste [projekt]`
pub fn verwalten(args: &[String]) -> Result<(), String> {
    let befehl = match args.first().map(String::as_str) {
        Some("--help") | Some("-h") | Some("help") | None => {
            print!("{HILFE}");

            return Ok(());
        }
        Some(w) => w,
    };

    let aktion = match befehl {
        "status" | "start" | "stop" => befehl,
        "liste" | "list" => "liste",
        anders => return Err(format!("Unbekannt: devstack {anders}\n\n{HILFE}")),
    };

    // Das Projekt steht als naechstes Wort da. Eine Option ist es nie, deshalb
    // werden `--`-Woerter hier nicht als Projekt angenommen — sonst stuende
    // bei einem Tippfehler „Keine Codebase --jso gefunden".
    let projekt = args
        .get(1)
        .filter(|w| !w.starts_with("--"))
        .map(String::as_str);

    if aktion != "liste" && projekt.is_none() {
        return Err(format!(
            "`devstack {aktion}` braucht ein Projekt, z. B. `devstack {aktion} peppermint-crm`.\n\n{HILFE}"
        ));
    }

    let mut nutzlast = serde_json::json!({ "aktion": aktion });

    if let Some(p) = projekt {
        nutzlast["projekt"] = serde_json::json!(p);
    }

    let connection = config::load()?;
    let token = mcp::access_token(&connection)?;

    let text = mcp::call_tool_text(&connection, &token, "devstack-tool", nutzlast)?;

    println!("{}", text.trim_end());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::verwalten;

    fn w(teile: &[&str]) -> Vec<String> {
        teile.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn ohne_befehl_kommt_die_hilfe_und_kein_fehler() {
        // Ein nackter Aufruf darf nicht als Fehlschlag enden — er ist die
        // Frage „was kann das hier?".
        assert!(verwalten(&[]).is_ok());
    }

    #[test]
    fn unbekannter_befehl_nennt_die_hilfe() {
        let fehler = verwalten(&w(&["neustarten"])).unwrap_err();

        assert!(
            fehler.contains("Unbekannt: devstack neustarten"),
            "{fehler}"
        );
        assert!(fehler.contains("liste"), "die Hilfe gehoert dazu: {fehler}");
    }

    #[test]
    fn start_ohne_projekt_fragt_danach_statt_zu_starten() {
        // Wichtiger als die Meldung ist, dass hier KEIN Aufruf nach draussen
        // geht: `start` ohne Projekt darf nicht raten.
        let fehler = verwalten(&w(&["start"])).unwrap_err();

        assert!(fehler.contains("braucht ein Projekt"), "{fehler}");
        assert!(fehler.contains("peppermint-crm"), "{fehler}");
    }

    #[test]
    fn eine_option_wird_nicht_als_projekt_genommen() {
        // Sonst stuende dort „Keine Codebase --jso gefunden" — eine Antwort
        // auf die falsche Frage.
        let fehler = verwalten(&w(&["status", "--json"])).unwrap_err();

        assert!(fehler.contains("braucht ein Projekt"), "{fehler}");
    }
}
