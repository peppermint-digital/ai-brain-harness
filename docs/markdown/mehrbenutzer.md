# Mehrere Menschen

*Ja, aber die Frage ist zu grob. Drei Bedeutungen, zwei davon koennen wir.*

Die kurze Antwort: **ja**, aber die Frage ist zu grob gestellt. Mehrbenutzerfaehig
kann dreierlei heissen, und wir sind es zweimal.

> **Schaubild:** Gemessen am 15.10.2026 an der echten Instanz: zwei Zugaenge auf diesem Rechner, einer auf dem Laptop.
> (in der HTML-Fassung dieser Seite)

## Die drei Bedeutungen

| Was gemeint sein kann | Koennen wir? | Wie |
|---|---|---|
| Mehrere **Rechner** an einer Instanz | ja | Ein Geraetezugang je Rechner. Jeder sieht nur seine Auftraege. |
| Mehrere **Konten** auf einem Rechner | ja | Die Ablage liegt im Heimverzeichnis. Jedes Konto koppelt sich selbst. |
| Zwei **Menschen unter einem Konto** | nein | Dann handeln beide als dieselbe Person. Das ist eine echte Grenze. |

## Woran die Identitaet haengt

An genau einer Sache: dem **Geraetezugang**. Das ist ein eigener OAuth-Client,
entstanden beim Koppeln ueber den Device-Flow, mit einer Person als Besitzer. Unter
„Meine Geraete“ ist er einzeln entziehbar.

Daraus folgt alles Weitere, ohne dass es irgendwo zweimal steht:

- welche Auftraege dieser Rechner abholen kann,
- welche Nachrichten er bekommt (und welche er nie bekommt — seine eigenen),
- welche Repository-Eintraege auf ihm landen,
- wem ein geschriebener Gedanke, eine Zeitbuchung, ein Pruefspur-Eintrag gehoert.

## Geteilt oder getrennt?

| Getrennt je Geraet | Geteilt ueber alle |
|---|---|
| Zugangsdaten und Token | Projektwissen und Wiki |
| Repository-Auswahl | Aufgaben und Bugs |
| Laufregister und Protokolle | Zeiterfassung (je Person zugeordnet) |
| Verzeichnis-Freigaben | Pruefspur |

> **Eine Falle beim Koppeln.** Der Geraetename entsteht als `$USER@hostname`. Omarchy nennt per Vorgabe jeden Rechner `omarchy` — zwei Rechner mit demselben Benutzernamen erzeugen also denselben Namen. Beim Koppeln einen eigenen Namen mitgeben, sonst stehen zwei gleichnamige Zugaenge in der Liste und niemand weiss mehr, welcher welcher ist.

## Was ein zweiter Mensch tun muss

```bash
# in seinem eigenen Konto, auf demselben oder einem anderen Rechner
curl -fsSL https://brain.proxy.peppermint-digital.com/install.sh | sh
ai-brain-cli connect
# bestaetigt im Browser mit SEINEM AI-Brain-Konto
ai-brain-cli sync --apply
```

Ab da schreibt er in dasselbe Projektwissen wie alle anderen, und jeder Eintrag traegt
seinen Namen. Er sieht die Aufgaben, die er sehen darf, und seine Zeit laeuft auf sein Konto.

## Wo es noch klemmt

- **Ein Konto, zwei Menschen** bleibt nicht trennbar. Wer das braucht, braucht
getrennte Systemkonten.
- **Rechner-genaues Adressieren bei Nachrichten** gibt es noch nicht — eine
Nachricht geht an eine Person, und der erste ihrer Rechner, der abholt, bekommt sie.
- **Zwei Abholer je Rechner** teilen sich einen Zugang und sind fuer AI Brain
nicht unterscheidbar. Eine Haltefrist von fuenf Minuten verhindert die doppelte Zustellung,
aber die Unterscheidung selbst kann die Instanz nicht treffen.
