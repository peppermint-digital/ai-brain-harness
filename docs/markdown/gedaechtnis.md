# Wo das Wissen liegt

*Hermes merkte sich Dinge auf der Platte. Wir merken sie uns in AI Brain — mit der Person daran, die sie geschrieben hat.*

Das war die Frage, die am laengsten offen war. Hermes war ein Einzelplatz-Programm und hat sein
Gedaechtnis auf der Platte gefuehrt — unter dem Konto, mit dem es lief. Fuer einen Menschen an
einem Rechner ist das richtig. Fuer mehrere ist es eine Sackgasse.

> **Schaubild:** Links der alte Zuschnitt, rechts unserer. Der Unterschied ist nicht die Technik, sondern wem das Wissen gehoert.
> (in der HTML-Fassung dieser Seite)

## Was auf dem Rechner bleibt

Unter `~/.config/ai-brain-cli/` — und das ist *alles*:

| Datei | Was | Verlust, wenn weg? |
|---|---|---|
| `connection.json` | Zugangsdaten des Geraets (0600) | Neu koppeln. |
| `token.json` | Zugangstoken, eine Stunde gueltig | Nichts, wird neu geholt. |
| `sync-state.json` | Welcher Stand zuletzt abgeglichen wurde | Der naechste Abgleich vergleicht Dateien statt Pruefsummen. |
| `freigaben.json` | Welche Verzeichnisse mehr duerfen | Laeufe fragen wieder vor jeder Tat. |
| `gesicherte-fassungen/` | Was `--force` ueberschrieben hat | Die Sicherungskopien. |
| `…/state/ai-brain-cli/laeufe/` | Laufregister und Protokolle | Die Liste vergangener Laeufe. |

**Kein Gedanke, keine Notiz, kein Gelerntes.** Nichts davon ist Wissen — es ist
Handwerkszeug. Wer das Verzeichnis loescht, verliert keinen Inhalt.

## Wohin das Wissen geht

Alles, was ein Lauf lernt, schreibt er ueber MCP nach AI Brain. Dort liegt es zweifach:
als Markdown-Datei in `~/knowledge-base/` und als Zeile in der Datenbank fuer die Suche.
Die Datei ist die Quelle, die Datenbank der Index.

| Art | Werkzeug | Liegt unter | Reichweite |
|---|---|---|---|
| Projektwissen | `add-memory-tool` | `projekte/<slug>/memories/` | ein Projekt |
| Uebergreifendes | `add-wiki-tool` | `wiki/` | alle Projekte |
| Fehler | `create-bug-tool` | `projekte/<slug>/bugs/` | ein Projekt |
| Aufgaben, Zeit, Pruefspur | eigene Werkzeuge | Datenbank | alle |

Gemessen am 15.10.2026: **4239 Eintraege** ueber **47 Projekte**.
An 4227 davon haengt die Person, die sie geschrieben hat.

## Ein drittes Gedaechtnis, das nicht unseres ist

> In der Datenbank steht ausserdem `agent_memories` mit rund 8600 Zeilen. Das gehoert **NanoClaw**, nicht der CLI: Es ist das Gedaechtnis der Container, abgegrenzt je Channel-Gruppe — `avatar-3`, `email_henneberg` und so weiter. Wer dort sucht, sucht am falschen Ort.

## Der Unterschied in einem Satz

Bei Hermes gehoerte das Gedaechtnis dem *Rechner*. Bei uns gehoert es dem
*Projekt* — und jeder Eintrag weiss, welche Person ihn beigesteuert hat.

## Was das praktisch heisst

- Ein Lauf auf dem Laptop und einer auf dem Desktop schreiben in dasselbe Projektwissen.
- Ein zweiter Mensch am selben Rechner schreibt ebenfalls dorthin — unter seinem Namen.
- Ein neu aufgesetzter Rechner ist nach `connect` und `sync` sofort auf Stand.
- Was ein Agent gestern gelernt hat, steht heute im Kontext — unabhaengig davon, wo er lief.
