# Alle Befehle

*Achtzehn Befehle, nach Zweck sortiert, mit den Schaltern, die man wirklich braucht.*

Achtzehn Befehle. Die Liste steht auch im Programm selbst — `ai-brain-cli` ohne
Argument zeigt sie.

## Einrichten

| Befehl | Was |
|---|---|
| `connect [domain]` | Rechner mit einer Instanz verbinden. Ohne Angabe die Instanz, von der das Programm kam. |
| `status` | Zeigt die aktuelle Verbindung. |
| `configure` | Traegt den MCP-Server in Claude Code und Codex ein. |
| `disconnect` | Entfernt die lokale Verbindung. |
| `freigabe [add <pfad>]` | Verzeichnisse, in denen Laeufe mehr duerfen als fragen. |

## Arbeiten

| Befehl | Was |
|---|---|
| `agent [...]` | Einen Lauf hier starten. |
| `agents` | Welche Laeufe hier laufen. |
| `logs <kennung>` | Was ein Lauf geschrieben hat. |
| `stop <kennung>` | Einen Lauf beenden. |

Schalter fuer `agent`:

| Schalter | Wirkung |
|---|---|
| `--aufgabe "…"` | Auftrag in Worten. |
| `--task <nr>` | Aufgabe aus AI Brain holen. |
| `--projekt <slug>` | Projektbezug. |
| `--modell claude\|codex` | Vorgabe ist `claude`. |
| `--schreibend`, `--vollzugriff` | Mehr Rechte — nur in freigegebenen Verzeichnissen. |
| `--verzeichnis <pfad>` | Wo der Lauf arbeitet. |
| `--imkr` | In einem eigenen imkr-Pane starten. |
| `--weiter`, `--sitzung <id>` | Eine Sitzung fortsetzen. |
| `--zeitgrenze <s>` | Vorgabe 3600 Sekunden. |
| `--trocken` | Zeigt, was gestartet wuerde, ohne zu starten. |

## Aufgaben und Auftraege

> **Zwei Woerter, die sich aehneln und Verschiedenes meinen.** Eine **Aufgabe** ist ein Eintrag aus der Projektliste, den sich jemand aussucht. Ein **Auftrag** ist etwas, das AI Brain genau diesem Rechner hingelegt hat.

| Befehl | Was |
|---|---|
| `aufgaben [--projekt <slug>] [--json]` | Offene Aufgaben zum Auswaehlen. Ohne Projekt projektuebergreifend. |
| `auftraege` | Was AI Brain fuer diesen Rechner bereitlegt. |
| `auftrag <nr>` | Annehmen und abarbeiten. Nimmt die Schalter von `agent` mit. |
| `auftrag-melden <nr> <status>` | Ergebnis nachtragen, falls das automatische Melden scheiterte. |

## Nachrichten

| Befehl | Was |
|---|---|
| `nachricht kontakte` | Wen dieser Rechner erreichen kann. |
| `nachricht senden <person> <text>` | Mit `--von` und `--ziel` fuer imkr-Adressen. Hoechstens 8 KB. |
| `nachricht abholen` | Was fuer diesen Rechner anliegt. |
| `nachricht bestaetigen <nr>` | Eingereiht — kommt nicht wieder. |

## Wissen abgleichen

| Befehl | Was |
|---|---|
| `sync [--apply]` | Skills und Agents aus dem Repository holen, einzeln gewaehlte MCP-Server in Claude Code und Codex eintragen. Ohne `--apply` wird nur gezeigt. |
| `publish <name>` | Einen lokalen Eintrag zurueckgeben. |
| `select` | Waehlen, welche Eintraege dieser Rechner bekommt. MCP-Server nur einzeln, nie ueber „alles“. |
| `select --dazu a,b / --weg a,b` | Ohne Terminal ergaenzen bzw. entfernen, der Modus bleibt. Namen als `typ/slug` (`mcp/playwright`) oder eindeutiger Slug; mehrdeutig oder unbekannt bricht ab, ohne zu speichern. |
| `select --setzen a,b` | Stellt auf Modus „auswahl“ und ersetzt die ganze Liste. Ein Rechner auf „alles“ verliert damit alle uebrigen Eintraege — zum Ergaenzen `--dazu`. |

Weitere Schalter fuer `sync`: `--only`, `--force`,
`--aufraeumen`, `--ja`.

## Live-Leitung

| Befehl | Was |
|---|---|
| `leitung an [--dauer 30m]` | Von Hand einschalten, hoechstens 8 h. Hebt ein vorheriges `aus` auf. |
| `leitung aus` | Auflegen — und zwar bis zum naechsten Einschalten, nicht bis zum naechsten Herzschlag. |
| `leitung status [--json]` | Schalter, Grund, Prozess, Verbindung. |
| `leitung dienst` | Haelt die Leitung und schaltet sie selbst, nach der Freigabe in AI Brain. |
| `leitung autostart` | Den Dienst beim Anmelden starten. `kein-autostart` bestellt es ab. |
| `leitung laufen` | Die Leitung im Vordergrund halten, ohne Dienst. |

Die Leitung zeigt ein imkr-Pane dieses Rechners in AI Brain und nimmt Tastendruecke von dort
an. Der Rechner verbindet sich **ausgehend**; AI Brain ruft hier nie etwas auf.

**Drei Zustaende, und die Reihenfolge ist die Regel.** Ein *Veto* (aus
`leitung aus` oder dem Linksklick im Tray) schlaegt alles und hat keine Frist —
sonst waere „aus“ eine Bitte und keine Anweisung. Darunter gilt der Hand-Schalter mit
seiner Restzeit, darunter die automatische Leitung. Sie hat keine Frist, sondern eine Bedingung:
Mindestens ein Channel muss diesen Rechner in AI Brain freigegeben haben. Faellt die Freigabe,
legt sie binnen einer Minute selbst auf. Ohne Freigabe bleibt sie aus.

**Das verschiebt keine Sperre.** Ob ein Agent etwas darf, prueft AI Brain bei jedem
Aufruf gegen die Freigabe je Channel und Geraet. Eine offene Leitung ohne Freigabe ist nutzlos,
nicht erlaubt.

Ueber dieselbe Leitung kommen die Auftraege der imkr-Werkzeuge eines Channels. Jeder ist eine
eigene Art, und der Rechner baut seine Befehlszeile selbst: `sitzungen` fuehrt
`imkr ls --json` aus (gefiltert, ohne Bildschirminhalte), `lesen` fuehrt
`imkr read` aus, und nur `befehl` fuehrt einen mitgeschickten Text aus —
und haengt an einer eigenen Freigabe.

## Als MCP-Server

```bash
ai-brain-cli mcp
```

Laeuft als MCP-Server und gibt Claude Code und Codex alle AI-Brain-Werkzeuge. Wird von
`configure` eingetragen; man ruft es selten von Hand auf.
