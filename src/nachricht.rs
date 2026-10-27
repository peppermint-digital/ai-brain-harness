//! Nachrichten zwischen den imkrs verschiedener Personen (#495).
//!
//! **Holprinzip wie bei den Aufträgen.** AI Brain stellt nicht zu — dieser
//! Rechner fragt, was für ihn anliegt, und meldet zurück, was er eingereiht
//! hat. Kein offener Port, keine Erreichbarkeitspflicht.
//!
//! **Mindestens einmal, nicht genau einmal.** `abholen` gibt eine Nachricht
//! heraus, ohne sie abzuschliessen; erst `bestaetigen` tut das. Stirbt imkr
//! zwischen beidem, kommt sie wieder — doppelt zustellen ist zu verkraften,
//! verlieren nicht. imkr überspringt Kennungen, die es schon hatte.
//!
//! **`abholen --json` ist ein Maschinenvertrag.** Die Antwort wird von imkr
//! geparst. Sie wird deshalb **unverändert durchgereicht**, nicht hier neu
//! zusammengesetzt: Ein zweiter Ort, der dieselbe Form beschreibt, ist ein
//! zweiter Ort, der von ihr abweichen kann.

use serde::Deserialize;

use crate::config;
use crate::mcp;

#[derive(Debug, Deserialize)]
struct Kontakt {
    person: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    rechner: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Ich {
    person: String,
    rechner: String,
}

#[derive(Debug, Deserialize)]
struct Kontakte {
    ich: Ich,
    #[serde(default)]
    kontakte: Vec<Kontakt>,
}

#[derive(Debug, Deserialize)]
struct Nachricht {
    id: u64,
    #[serde(default)]
    von_person: Option<String>,
    #[serde(default)]
    von: Option<String>,
    #[serde(default)]
    ziel: Option<String>,
    text: String,
    #[serde(default)]
    abholungen: u64,
}

#[derive(Debug, Deserialize)]
struct Postfach {
    #[serde(default)]
    nachrichten: Vec<Nachricht>,
}

#[derive(Debug, Deserialize)]
struct Gesendet {
    id: u64,
    #[serde(default)]
    an: Option<String>,
}

/// `nachricht <kontakte|senden|abholen|bestaetigen> …`
pub fn fuehren(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("kontakte") => kontakte(&args[1..]),
        Some("senden") => senden(&args[1..]),
        Some("abholen") => abholen(&args[1..]),
        Some("bestaetigen") => bestaetigen(&args[1..]),
        Some(anderes) => Err(format!(
            "`{anderes}` kenne ich nicht. Möglich: kontakte, senden, abholen, bestaetigen."
        )),
        None => Err("`ai-brain-harness nachricht <kontakte|senden|abholen|bestaetigen>`".to_string()),
    }
}

/// Flaggenwert herausziehen (`--ziel arbeit/2`).
///
/// Gibt den Wert und die beiden verbrauchten Stellen zurück, damit der
/// Aufrufer sie nicht als Text mitzählt — sonst landete „--ziel" in der
/// Nachricht.
///
/// Auch von [`crate::meldung`] benutzt: Eine zweite Umsetzung derselben Regel
/// ist eine zweite, die abweichen kann.
pub(crate) fn flagge(args: &[String], name: &str) -> Option<(String, usize)> {
    let pos = args.iter().position(|a| a == name)?;
    let wert = args.get(pos + 1)?;

    if wert.starts_with("--") {
        return None;
    }

    Some((wert.clone(), pos))
}

fn kontakte(args: &[String]) -> Result<(), String> {
    let connection = config::load()?;
    let token = mcp::access_token(&connection)?;
    let roh = mcp::call_tool(
        &connection,
        &token,
        "list-agent-contacts-tool",
        serde_json::json!({}),
    )?;

    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&roh).map_err(|e| format!("{e}"))?);

        return Ok(());
    }

    let daten: Kontakte = serde_json::from_value(roh)
        .map_err(|e| format!("Antwort auf list-agent-contacts-tool ist unlesbar: {e}"))?;

    println!("Dieser Rechner: {} ({})", daten.ich.rechner, daten.ich.person);
    println!();

    if daten.kontakte.is_empty() {
        println!("Niemand sonst hat einen Rechner angebunden — es gibt keinen, der abholt.");

        return Ok(());
    }

    println!("Erreichbar:");

    for k in &daten.kontakte {
        println!(
            "  {:<20} {:<34} {}",
            k.person,
            k.email.as_deref().unwrap_or("—"),
            // Die Rechner stehen hier, weil rechner-genaues Adressieren noch
            // nicht geht — man sieht wenigstens, wohin es gehen KÖNNTE.
            k.rechner.join(", ")
        );
    }

    println!();
    println!("Schicken:  ai-brain-harness nachricht senden <person> <text>");

    Ok(())
}

fn senden(args: &[String]) -> Result<(), String> {
    let von = flagge(args, "--von");
    let ziel = flagge(args, "--ziel");

    // Die Stellen der Flaggen und ihrer Werte sind kein Text.
    let mut belegt: Vec<usize> = Vec::new();

    for (_, pos) in [&von, &ziel].into_iter().flatten() {
        belegt.push(*pos);
        belegt.push(pos + 1);
    }

    let rest: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, _)| !belegt.contains(i))
        .map(|(_, a)| a)
        .collect();

    let Some(person) = rest.first() else {
        return Err(
            "An wen? `ai-brain-harness nachricht senden <person> <text>` (Liste: `nachricht kontakte`)"
                .to_string(),
        );
    };

    let text = rest[1..]
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    if text.trim().is_empty() {
        return Err("Die Nachricht ist leer. `nachricht senden <person> <text>`".to_string());
    }

    let connection = config::load()?;
    let token = mcp::access_token(&connection)?;

    let mut nutzlast = serde_json::json!({ "an": person, "text": text });

    if let Some((v, _)) = &von {
        nutzlast["von"] = serde_json::json!(v);
    }

    if let Some((z, _)) = &ziel {
        nutzlast["ziel"] = serde_json::json!(z);
    }

    let roh = mcp::call_tool(&connection, &token, "send-agent-message-tool", nutzlast)?;

    let gesendet: Gesendet = serde_json::from_value(roh)
        .map_err(|e| format!("Antwort auf send-agent-message-tool ist unlesbar: {e}"))?;

    println!(
        "Nachricht #{} liegt für {} bereit.",
        gesendet.id,
        gesendet.an.as_deref().unwrap_or(person.as_str())
    );

    Ok(())
}

fn abholen(args: &[String]) -> Result<(), String> {
    let connection = config::load()?;
    let token = mcp::access_token(&connection)?;
    let roh = mcp::call_tool(
        &connection,
        &token,
        "fetch-agent-messages-tool",
        serde_json::json!({}),
    )?;

    // Unverändert durchreichen — das ist die Form, auf die sich imkr verlässt.
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string(&roh).map_err(|e| format!("{e}"))?);

        return Ok(());
    }

    let postfach: Postfach = serde_json::from_value(roh)
        .map_err(|e| format!("Antwort auf fetch-agent-messages-tool ist unlesbar: {e}"))?;

    if postfach.nachrichten.is_empty() {
        println!("Keine Nachrichten.");

        return Ok(());
    }

    for n in &postfach.nachrichten {
        println!(
            "#{} von {}{}{}",
            n.id,
            n.von_person.as_deref().unwrap_or("?"),
            n.von.as_deref().map(|v| format!(" ({v})")).unwrap_or_default(),
            n.ziel
                .as_deref()
                .map(|z| format!(" → {z}"))
                .unwrap_or_default()
        );

        println!("  {}", n.text.replace('\n', "\n  "));

        // Grösser als eins heisst: schon einmal geholt, nie bestätigt.
        if n.abholungen > 1 {
            println!("  (zum {}. Mal geholt — noch nicht bestätigt)", n.abholungen);
        }

        println!();
    }

    println!("Eingereiht melden:  ai-brain-harness nachricht bestaetigen <nummer>");

    Ok(())
}

fn bestaetigen(args: &[String]) -> Result<(), String> {
    let Some(roh) = args.first() else {
        return Err("`ai-brain-harness nachricht bestaetigen <nummer>`".to_string());
    };

    let nummer: u64 = roh
        .trim_start_matches('#')
        .parse()
        .map_err(|_| format!("`{roh}` ist keine Nachrichtennummer."))?;

    let connection = config::load()?;
    let token = mcp::access_token(&connection)?;

    let antwort = mcp::call_tool_text(
        &connection,
        &token,
        "confirm-agent-message-tool",
        serde_json::json!({ "message_id": nummer }),
    )?;

    println!("{}", antwort.trim());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::flagge;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flagge_findet_wert_und_stelle() {
        let a = args(&["Martin", "hallo", "--ziel", "arbeit/2"]);

        assert_eq!(flagge(&a, "--ziel"), Some(("arbeit/2".to_string(), 2)));
        assert_eq!(flagge(&a, "--von"), None);
    }

    #[test]
    fn flagge_ohne_wert_gilt_nicht() {
        // `--ziel --von x` darf nicht „--von" als Ziel nehmen, und ein
        // abschliessendes `--ziel` ohne Wert darf nicht abstürzen.
        assert_eq!(flagge(&args(&["--ziel", "--von", "x"]), "--ziel"), None);
        assert_eq!(flagge(&args(&["hallo", "--ziel"]), "--ziel"), None);
    }
}
