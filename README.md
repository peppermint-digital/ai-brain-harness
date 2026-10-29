# AI Brain Harness

Verbindet einen Rechner mit einer AI-Brain-Instanz, gibt ihm deren Werkzeuge
*und* die Hoheit über diesen Rechner — und hält das Gespräch in einer eigenen
Maske.

Der Befehl heisst `ai-brain-harness`, der Einstieg ist `brain`.

> **Umbenennung, 15.10.2026.** Bis dahin hiess der Befehl `ai-brain-cli` (und
> davor `ai-brain-companion`). Der Installer legt `ai-brain-cli` weiter als
> Verweis an, bis alle Rechner umgestellt sind — ein Umbenennen, das unterwegs
> MCP-Einträge, `brain`, Skripte und das imkr-Plugin bricht, ist teurer als ein
> Verweis, der eine Weile mitläuft.
>
> Zwei Dinge behalten absichtlich den alten Namen:
>
> - **Die Ablage** (`~/.config/ai-brain-cli`). Sie ist unsichtbar, und ein
>   zweiter Datenumzug auf drei Rechnern wäre Risiko ohne Gegenwert. `install.sh`
>   schreibt dort hinein, *bevor* das Programm das erste Mal läuft.
> - **Der Gedächtnis-Schlüssel** in AI Brain. Unter ihm liegen die Einträge des
>   Harness — am 15.10.2026 waren es 139. Ihn umzubenennen würde sie verwaisen
>   lassen, und das sähe aus wie „der Harness hat kein Gedächtnis".
>
> `ai-brain-harness configure` schreibt den MCP-Eintrag auf den neuen Pfad um
> **und räumt Einträge früherer Fassungen weg**. Das ist kein Beiwerk: Auf
> Laptop und Desktop zeigte der Eintrag am 15.10.2026 noch auf
> `ai-brain-companion` vom August, und auf diesem Rechner auf eine veraltete
> Kopie unter `/usr/local/bin`. Der Eintrag sah jeweils richtig aus; nur der
> Pfad hinter ihm war alt.

Du gibst eine Domain an, bestätigst einmal einen kurzen Code im Browser, fertig.
Zugangsdaten tippst du nirgends ab; sie wandern nie durch die Zwischenablage.

## Was ein Harness ist — und was dieser anders macht

Ein *Harness* ist die Schicht zwischen einem Sprachmodell und der Welt: sie hält
das Gespräch, reicht Werkzeuge hinein, verwaltet Zugänge und Rechte und
entscheidet, was ausgeführt werden darf. Eine Quellcode-Studie über elf solcher
Systeme zerlegt sie in sechs Teile — Werkzeugschicht, Zugangsverwaltung,
Rechtesystem, Loop-Steuerung, Plattformabstraktion, Laufzeitumgebung. Daran
lassen sich die Systeme vergleichen, die dasselbe Problem verschieden lösen:

|  | Agentenschleife | Werkzeuge | Zugänge | Rechte | Rechnerzugriff |
|---|---|---|---|---|---|
| **Pi** | eigen, bewusst minimal (Systemprompt und Werkzeugdefinitionen unter 1000 Token) | 8 eingebaute, 4 standardmässig an: `read`, `write`, `edit`, `bash`; dazu `grep`, `find`, `ls` | selbst besorgen | als Erweiterung nachrüstbar (Rechtegitter, Pfadschutz, Sandbox) | ja |
| **OpenClaw** | Pi als Kern, darüber ein Plugin-Ökosystem | je Integration ein Plugin mit Manifest — Channels, Modellanbieter, Speicher, Skills | je Plugin | je Plugin | ja |
| **Hermes** | eigen | eigene, mitgebracht (Terminal, Datei, Gedächtnis, Cronjob, TTS, Vision …) | eigene Ablage unter `~/.hermes` | keine | ja, aber `computer_use` nur unter macOS |
| **AI Brain Harness** | **eigen *und* gemietet** — eigener Loop für OpenAI-kompatible und Anthropic, `claude`/`codex` weiter nutzbar | **AI Brains MCP-Werkzeuge *und* die dieses Rechners** | **AI Brains Register** | **AI Brains Nutzerrechte** | ja, voller Rechner |

### Der Unterschied in einem Satz

Die anderen bringen ihre Welt mit. Dieser erbt sie.

Was in AI Brain eingerichtet ist, kennt der Harness ohne eine Zeile
Integrationscode: Server samt Host, Nutzer, Port und letztem Zustand;
Zugangsdaten projektübergreifend; Modellkonten; Mailkonten; Projekte, Aufgaben,
Fehler, Gedächtnis, Wiki, Vault. Eine neue Fähigkeit ist deshalb fast immer ein
neues Werkzeug **in AI Brain**, nicht Code hier — dort nützt es gleichzeitig
allen Rechnern, allen Channels und allen angebundenen Produkten.

### Die Rechte sind der unterschätzte Teil

Der Harness meldet sich über OAuth an und **ist** dabei eine Person. Läuft er
als Bastian, hat er genau Bastians Rechte — nicht mehr. Damit ist die
Rechtefrage gelöst, bevor sie gestellt wird: keine zweite Rechteverwaltung, und
ein Harness auf dem Rechner einer Mitarbeiterin kommt nur an das, was sie sehen
darf. Bei Pi und Hermes gibt es diese Ebene nicht; bei OpenClaw hängt sie am
jeweiligen Plugin.

### Zwei Werkzeugserver in derselben Hand

Ein Zug der Maske hat beides zugleich:

```
            ┌─────────────────────────────┐
  brain ───▶│  Maske  (maske.rs)          │
            │  Eingabe · Darstellung      │
            └──────────┬──────────────────┘
                       │  pro Zug
            ┌──────────▼──────────────────┐
            │  Motor (gemietet)           │
            └─────┬───────────────┬───────┘
                  │               │
        ┌─────────▼──────┐  ┌─────▼──────────────┐
        │  ai-brain      │  │  rechner           │
        │  (mcp)         │  │  (rechner-mcp)     │
        │                │  │                    │
        │  Server        │  │  Dienste           │
        │  Zugangsdaten  │  │  Datenträger       │
        │  Aufgaben      │  │  Last · VRAM       │
        │  Gedächtnis    │  │  Befehle           │
        │  Mail · Vault  │  │  Bildschirm        │
        │  Rechtefenster │  │  Fenster · Tasten  │
        └────────────────┘  └────────────────────┘
```

Das war die Trennung, an der es vorher scheiterte: Hermes konnte die Maschine,
aber nicht unsere Daten. Die Werkzeuge von AI Brain konnten unsere Daten, aber
nicht die Maschine. Und ein Channel in seinem Docker-Container kommt
grundsätzlich nur an sein eigenes Dateisystem — der Harness läuft **auf** dem
Rechner.

### Was noch fehlt

Der **eigene Loop**. Heute wird der Motor pro Zug gerufen (`claude -p`), und das
Gespräch hält die Sitzungskennung zusammen. Oberfläche, Werkzeuge, Verlauf und
Buchführung gehören uns; das Rechnen ist eingekauft. Ein eigener Loop braucht
einen eigenen Modellzugang — eine Kostenentscheidung, die nicht beim Bauen
fallen soll. Die Trennung sitzt deshalb an einer einzigen Stelle
(`maske.rs::motor_args`), damit sie sich tauschen lässt.

### Belege

Hermes ist auf dem Hauptrechner gemessen (Werkzeugliste, Ablage,
`unavailable_toolsets`). Pi und OpenClaw sind aus deren öffentlicher
Dokumentation belegt:

- [Pi: The Minimal Agent Within OpenClaw](https://lucumr.pocoo.org/2026/1/31/pi/)
- [Pi Agent Extensions: Change the Harness, Not Just the Prompt](https://www.aibuilderclub.com/blog/pi-agent-extensions-guide)
- [Agent runtime architecture · OpenClaw](https://docs.openclaw.ai/agent-runtime-architecture)
- [OpenClaw Architecture — Tools, Plugins, and Capability Boundaries](https://theagentstack.substack.com/p/openclaw-architecture-part-5-tools)
- [Harness Engineering: Anatomy, Architecture, and Evolution of Coding Agents — A Source-Code Study of Eleven Systems](https://arxiv.org/pdf/2609.00006)

## Was dabei passiert

```
ai-brain-harness                         AI Brain
   |                                 |
   |-- Steckbrief lesen ------------>|   /.well-known/ai-brain
   |<-- Endpunkte + Client-ID -------|
   |                                 |
   |-- Kopplung starten ------------>|   /oauth/device/code
   |<-- Code "WXYZ-1234" ------------|
   |                                 |
   |   Du bestätigst im Browser      |   /oauth/device
   |                                 |
   |-- wartet ---------------------->|
   |<-- Zugangstoken ----------------|
   |                                 |
   |-- Gerät einrichten ------------>|   /api/v1/companion/provision
   |<== eigener Gerätezugang ========|
```

Am Ende hat **dieser Rechner** einen eigenen Zugang, der **dir** gehört. Er
taucht unter „Meine Geräte" auf und lässt sich dort jederzeit entziehen.

Warum ein eigener Zugang pro Gerät und nicht ein geteilter: So bleibt
nachvollziehbar, was auf welchem Rechner passiert ist. In der Zeiterfassung
steht der Gerätename, zugerechnet wird die Arbeit dir.

## Installieren

### Von deiner AI-Brain-Instanz (empfohlen)

```bash
curl -fsSL https://deine-brain-domain.de/install.sh | sh
```

Jede Instanz liefert die CLI selbst aus. Der Vorteil: **Die Domain steckt
schon im Befehl** — das Skript merkt sie sich, und danach genügt
`ai-brain-harness connect` ohne Argument.

Ohne AI Brain ist die CLI ohnehin nutzlos; die Instanz ist also ihr
natürlicher Ort. Wer AI Brain selbst betreibt, muss seine Leute damit nicht auf
ein fremdes Repository schicken.

Was eine Instanz bereithält, steht unter `https://deine-domain.de/install.json`.

> **Für Betreiber:** Die Wege kommen mit AI Brain mit, die Programme nicht — 8 MB
> Binärdateien gehören nicht ins Repository. Einmal
> `php artisan companion:fetch` auf der Instanz, danach versorgt sie ihre eigenen
> Rechner. Ohne Internetzugang: `companion:fetch --von=/pfad/zum/ordner`. Liegt
> für eine Plattform nichts bereit, sagt das Skript das und verweist auf den
> Quelltext, statt eine kaputte Datei zu installieren.
>
> **Bauen kannst du selbst — GitHub Actions braucht es dafür nicht.** Linux
> direkt mit `cargo build --release`, Windows in einem Container:
>
> ```bash
> docker run --rm -v "$PWD":/src:ro -v /tmp/aus:/out -w /work rust:1-bookworm sh -c '
>   apt-get update -qq && apt-get install -y -qq gcc-mingw-w64-x86-64
>   rustup target add x86_64-pc-windows-gnu
>   cp -r /src/. /work/ && rm -rf /work/target
>   cargo build --release --target x86_64-pc-windows-gnu
>   cp target/x86_64-pc-windows-gnu/release/ai-brain-harness.exe /out/'
> ```
>
> Dauert etwa 20 Sekunden. `mingw-w64` und Rust sind frei — hier steht keine
> Lizenz im Weg. **Für macOS geht das nicht:** Dafür braucht es Apples SDK, und
> dessen Lizenz erlaubt die Nutzung nur auf Apple-Hardware. Das lässt sich nicht
> wegcontainern, sondern nur auf einem Mac erledigen (siehe unten) — dessen
> Ergebnis du dann mit `companion:fetch --von=…` auf die Instanz legst, damit
> jeder weitere Mac es fertig bekommt.

### Von GitHub

```bash
curl -fsSL https://raw.githubusercontent.com/peppermint-digital/ai-brain-harness/main/install.sh | sh
```

> Seit dem 27.10.2026 gibt es Releases (`v*`-Tag → GitHub Actions). Gibt es für
> deine Plattform keins, baut das Skript aus dem Quelltext (siehe unten).

Holt das gebaute Programm aus dem letzten Release, legt es nach `~/.local/bin`
und trägt das Verzeichnis in `~/.zshrc` und `~/.bashrc` ein, falls es dort noch
fehlt. Mehrfaches Ausführen ändert nichts doppelt; einen vorhandenen eigenen
Eintrag lässt es in Ruhe. `~/.bash_profile` und `~/.profile` werden nur
angefasst, wenn sie schon existieren.

Danach eine neue Shell öffnen (oder `. ~/.zshrc`).

Gibt es kein passendes Release, **baut das Skript aus dem Quelltext** — dann
braucht es Rust (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
und etwa eine halbe Minute.

### macOS

Solange auf der Instanz kein fertiges Programm für macOS liegt, baut der Mac es
sich selbst — zwei Befehle, ein bis zwei Minuten Rechenzeit:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install --git https://github.com/peppermint-digital/ai-brain-harness
```

Danach liegt es in `~/.cargo/bin` und du verbindest wie überall:

```bash
ai-brain-harness connect brain.example.com
```

Das ist kein Notbehelf, sondern für einen einzelnen Mac der kürzere Weg — er
kommt ohne Release, ohne Token und ohne fremde Baumaschine aus.

Wer mehrere Macs anzubinden hat, baut einmal auf einem davon und legt das
Ergebnis auf die Instanz:

```bash
# auf dem Mac
cargo build --release
# Datei als ai-brain-harness-aarch64-apple-darwin auf die Instanz bringen,
# dort:  php artisan companion:fetch --von=/pfad/zum/ordner
```

Ab dann genügt auf jedem weiteren Mac der Einzeiler von der Instanz.

### Windows

```powershell
irm https://deine-brain-domain.de/install.ps1 | iex
```

Wie unter Linux liefert die Instanz das Skript selbst aus, mit ihrer Adresse
darin. Es holt die `.exe`, **prüft die Prüfsumme aus `/install.json`**, nimmt die
Herkunftsmarkierung weg (sonst fragt der SmartScreen beim ersten Start nach),
merkt sich die Adresse unter `%APPDATA%\ai-brain-cli\instanz` und ergänzt
den PATH — den des **Benutzers**, nicht den des Systems, der bräuchte
Administratorrechte.

Danach ein neues Terminal öffnen, dann genügt `ai-brain-harness connect` ohne
Argument.

Wenn du die Adresse nicht zur Hand hast, geht auch der Weg über dieses
Repository — das Skript fragt dann danach und holt den Rest von dort:

```powershell
irm https://raw.githubusercontent.com/peppermint-digital/ai-brain-harness/main/install.ps1 | iex
```

Alternativ funktioniert unter WSL die Linux-Anleitung unverändert.

### Von Hand

```bash
git clone https://github.com/peppermint-digital/ai-brain-harness.git
cd ai-brain-harness
cargo build --release
install -m 755 target/release/ai-brain-harness ~/.local/bin/
```

### Plattformen

| Plattform | Stand |
|---|---|
| Linux x86-64 | Ende zu Ende erprobt, fertiges Programm auf der Instanz |
| Windows x86-64 | gebaut (GNU-Ziel, im Container); Installer gegen die echte Instanz durchgespielt, aber **auf echtem Windows noch nicht gelaufen** |
| macOS | wird auf dem Mac selbst gebaut, **noch nicht im Feld erprobt** |

Die Pfadauflösung unterscheidet sich zwischen den Systemen: `HOME` gibt es unter
Windows nicht, dort heißt es `USERPROFILE`, und die Verbindungsdaten gehören
nach `%APPDATA%` statt in einen versteckten Ordner. Das ist berücksichtigt und
durch einen Test abgedeckt — aber ein Test ist kein echtes Windows.

**Signaturen brauchst du dafür nicht.** Ein Programm, das per `curl` im Terminal
ankommt, bekommt unter macOS kein Quarantäne-Merkmal — Gatekeeper greift bei
`.dmg`, `.pkg` und allem, was im Finder doppelgeklickt wird. Unter Windows
nimmt das Installationsskript die Markierung mit `Unblock-File` weg.

## Verbinden

```bash
ai-brain-harness connect brain.example.com
```

Die CLI zeigt einen Code und eine Adresse. Öffne sie, melde dich an,
bestätige. Der Browser meldet „Gerät verbunden", das Terminal zeigt:

```
Fertig.
  Gerät          : bastian@mein-laptop (linux x86_64)
  Zugerechnet an : Bastian
  Hinterlegt in  : ~/.config/ai-brain-cli/connection.json
  Claude Code    : Eintrag „ai-brain" in ~/.claude.json
```

Claude Code neu starten, dann stehen die Werkzeuge bereit.

**Codex wird mitbedient**, falls es installiert ist. Die CLI nutzt dafür
`codex mcp add` — Codex kennt sein eigenes TOML-Format am besten, und die
`~/.codex/config.toml` enthält meist schon andere Abschnitte, die kein
selbstgebauter Schreiber anfassen sollte.

### Optionen

| Option | Wirkung |
|---|---|
| `--name <Name>` | Abweichender Gerätename |
| `--no-claude-config` | Claude-Code-Konfiguration nicht anfassen |

## Gerätenamen

Der Standardname ist `konto@rechner` plus Plattform, also etwa
`bastian@omarchy (linux x86_64)`.

Das Konto steht bewusst mit drin: Zwei Benutzerkonten auf derselben Maschine
sind aus Sicht von AI Brain zwei Geräte, weil die Verbindung pro Konto in
`~/.config` liegt. Ohne den Kontonamen entstünden zwei Einträge mit identischem
Namen — und weil dieser Name in der Zeiterfassung als Gerät erscheint, wäre die
Auswertung pro Rechner damit wertlos. Genau das ist beim ersten Feldtest
passiert.

Nachträglich umbenennen kannst du jedes Gerät unter „Meine Geräte" über das
Stift-Symbol. Neu koppeln ist dafür nicht nötig.

## Skills und Agents abgleichen

```bash
ai-brain-harness sync           # zeigt nur, was sich ändern würde
ai-brain-harness sync --apply   # schreibt
```

Skills landen unter `~/.claude/skills/`, Agents unter `~/.claude/agents/`.

### Was dabei nicht passiert

**Was nur bei dir liegt, bleibt liegen.** Ein Skill, den du selbst gebaut hast,
wird nicht gelöscht, nicht überschrieben und nicht hochgeladen. Die geteilte
Ablage geht er nichts an, solange du ihn nicht dorthin gibst.

**Was du geändert hast, wird nicht stillschweigend ersetzt.** Hat sich ein
Eintrag auf beiden Seiten bewegt, meldet `sync` einen Konflikt und lässt ihn in
Ruhe. Mit `--force` überschreibst du trotzdem — deine Fassung wandert dann als
`<name>.lokal` daneben, statt verloren zu gehen.

**Hooks werden nie eingerichtet.** Ein Hook ist keine Datei, sondern ein Befehl
in der `settings.json`, der von selbst ausgeführt wird. Er taucht in der Liste
auf, damit du weißt, dass es ihn gibt — eingerichtet wird er von Hand.

### Optionen

| Option | Wirkung |
|---|---|
| `--apply` | Änderungen wirklich schreiben |
| `--only <name>` | Nur diesen einen Eintrag |
| `--force` | Auch überschreiben, wo lokal geändert wurde |
| `--aufraeumen` | Nicht mehr Ausgewähltes entfernen (siehe unten) |

`--force` und `--aufraeumen` tun allein nichts — sie brauchen zusätzlich
`--apply`.

### Auswahl pro Rechner

Unter „Meine Geräte" legst du fest, was auf diesem Rechner liegen soll. Die
CLI meldet sich mit ihrem Gerätezugang an, die Instanz weiß also, wer
fragt, und schickt genau diese Auswahl.

Pro Gerät gibt es zwei Betriebsarten:

| Modus | Bedeutung |
|---|---|
| **Alles** | Der Rechner bekommt das gesamte Repository |
| **Nur Auswahl** | Genau das Angehakte — auch nichts, wenn nichts angehakt ist |

**Neu verbundene Rechner starten auf „Nur Auswahl"** und damit leer. Du
entscheidest, was drauf soll, statt 194 Einträge vorgesetzt zu bekommen.

### Abgewähltes entfernen

```bash
ai-brain-harness sync --apply --aufraeumen
```

Entfernt Einträge, die nicht mehr ausgewählt sind. Aber nur die, die **der
ai-brain-harness selbst geschrieben hat** und die seither **unverändert** sind.

Alles andere bleibt liegen, auch mit `--aufraeumen`: deine Eigenbauten, alles
was du verändert hast, und alles was sie nie angelegt hat. Eine Synchronisation,
die fremde Arbeit löschen kann, ist keine Synchronisation mehr.

Ohne `--aufraeumen` wird nichts entfernt — es wird nur gemeldet.

## Etwas zurückgeben

```bash
ai-brain-harness publish mein-skill
ai-brain-harness publish mein-skill --grund "Abschnitt zu X ergänzt"
```

Nimmt einen Eintrag aus `~/.claude/skills/` oder `~/.claude/agents/` und legt
ihn im Repository ab. Existiert er dort schon, wird er aktualisiert statt
verdoppelt — die vorherige Fassung bleibt als Version erhalten und ist
zurückholbar.

Das ist bewusst ein eigener Befehl und passiert **nie** als Nebeneffekt von
`sync`. Ein Skill, den du für dich gebaut hast, mit Notizen oder Pfaden darin,
soll nicht ungefragt in einer Ablage landen, die alle lesen können.

Binärdateien überspringt der Befehl und sagt welche — lieber melden als etwas
Kaputtes hochladen.

### Wie Änderungen erkannt werden

Die CLI merkt sich beim Schreiben zwei Dinge: die Prüfsumme, die die
Instanz damals nannte, und einen eigenen Hash über die Dateien, wie sie sie
hingelegt hat. Beim nächsten Lauf vergleicht sie den ersten mit der Instanz
(hat sich dort etwas getan?) und den zweiten mit der Platte (hat hier jemand
Hand angelegt?).

Beide Vergleiche laufen gegen den eigenen Wert. Die Server-Prüfsumme
nachzurechnen wäre naheliegend, aber zerbrechlich: Zwei Sprachen müssten bis
aufs letzte Escaping identisch serialisieren, und ein einziger Unterschied
ließe alles als „geändert" erscheinen.

## Agentenläufe

```bash
ai-brain-harness agent --task 7666
ai-brain-harness agent --projekt ai-brain "Changelog für heute schreiben"
ai-brain-harness agent --modell codex --dir ~/Development/crm "Tests reparieren"
ai-brain-harness agent --task 7666 --trocken     # nur zeigen, was passieren würde
```

Startet einen Agenten auf diesem Rechner, meldet ihn im Register an und
protokolliert, was er tut.

**Die Schleife ist gemietet, nicht gebaut.** Die CLI ruft `claude -p` bzw.
`codex exec` auf. Es gibt hier keinen eigenen Modellaufruf, keinen eigenen
Werkzeug-Dispatch und keine eigene Kontextkürzung — der Claude Agent SDK *ist*
Claude Code als Bibliothek, und für Codex, Ollama und OpenRouter löst eine
eigene Schleife ohnehin nichts. Was die CLI beiträgt, ist das, was dem
gemieteten Agenten fehlt: Kontext aus AI Brain, eine geprüfte Umgebung, ein
Registereintrag und ein Protokoll.

**Dein eigener Login zählt.** Der Lauf nutzt die Anmeldung, die auf diesem
Rechner ohnehin gilt — Abo oder `ANTHROPIC_API_KEY`. Es wird kein Token
weitergereicht und kein Proxy gebraucht.

**Der Vorlauf bleibt kurz.** Der Agent bekommt gesagt, welche Aufgabe gemeint
ist und dass `/brain` gilt — den Kontext holt er sich selbst über MCP. Alles
hineinzukopieren hiesse, eine zweite Kopie zu pflegen, die veraltet.

**Sitzungsvariablen werden entfernt.** `CLAUDECODE`, `CLAUDE_CODE_*`,
`CLAUDE_PID`, `CLAUDE_EFFORT`. Ohne das hält sich ein aus einer laufenden
Claude-Code-Sitzung gestarteter Agent für ein Kind davon und schreibt kein
eigenes Transkript.

### Eine Unterhaltung weiterführen

```bash
ai-brain-harness agent --projekt x "Merke dir die Zahl 4711."
ai-brain-harness logs <kennung>            # zeigt die Sitzungskennung
ai-brain-harness agent --sitzung <uuid> "Welche Zahl war es?"   # → 4711
ai-brain-harness agent --fortsetzen "..."  # die letzte hier
```

Ohne diese Schalter beginnt jeder Lauf von vorn. Das war die grösste Lücke
gegenüber Hermes, das Unterhaltungen über Monate hält — die Schalter gibt es in
beiden Werkzeugen, sie wurden nur nicht durchgereicht.

**Die Sitzungskennung kommt von uns, nicht aus der Ausgabe.** `claude` nimmt
eine vorgegebene (`--session-id <uuid>`); die CLI leitet sie aus der
Lauf-Kennung ab und schreibt sie ins Register. Der Umweg über
`--output-format json` hätte das Protokoll von lesbarem Text in JSON verwandelt,
nur um eine Zeichenkette zu erfahren.

| | claude | codex |
|---|---|---|
| frisch | `--session-id <uuid>` (wir vergeben sie) | eigene Kennung, von codex |
| `--fortsetzen` | `-c` | `exec resume --last` |
| `--sitzung <id>` | `-r <id>` | `exec resume <id>` |

**Eine Kombination geht nicht, und sie wird abgelehnt statt stillschweigend
geändert:** `codex exec resume` kennt kein `-s/--sandbox`, also ist
`--schreibend` beim Fortsetzen mit codex nicht ausdrückbar. Den Schalter
einfach fallen zu lassen wäre das Schlimmste — der Lauf liefe mit anderen
Rechten als verlangt, und niemand sähe es.

Im Pane (`--imkr`) geht Fortsetzen nur mit `--modell claude`; für codex heisst
der interaktive Weg `codex resume` und ist hier nicht verdrahtet.

### Was in AI Brain davon ankommt

Jeder Lauf meldet Anfang und Ende als Audit-Eintrag: Projekt, Aufgabe, Modell,
Verzeichnis, Rechte-Stufe, Weg, Exit-Code, Dauer.

**Person und Gerät muss die CLI dafür nicht mitgeben.** AI Brain nimmt die
Person aus dem Token und den Gerätenamen aus dem OAuth-Client — bei einem
Gerätezugang ist der Client-Name der Gerätename. Dieselbe Person an zwei
Rechnern bleibt damit auseinanderzuhalten.

**Ein fehlgeschlagenes Protokoll hält den Lauf nicht auf.** Die Arbeit passiert
auf diesem Rechner; ist AI Brain gerade nicht erreichbar, gibt es eine Zeile auf
stderr und der Lauf läuft. Ohne Verbindung geht `agent` überhaupt — nur `--task`
verlangt sie, denn ein Lauf „an Aufgabe #N", der die Aufgabe nicht kennt,
arbeitet an etwas Erfundenem.

**Zeiteinträge werden nicht angelegt.** Ein Agentenlauf ist Maschinenzeit, keine
Arbeitszeit eines Menschen; automatisch erfasst würde er die Zeiterfassung
aufblähen und sich mit dem eigenen Timer überlappen. Wer die Zeit haben will,
trägt sie bewusst ein.

### In einem imkr-Pane statt als Kindprozess

```bash
ai-brain-harness agent --imkr --task 7667
```

Ein beaufsichtigter Lauf ist ein Kindprozess: Die CLI hält ihn, schreibt sein
Protokoll, stempelt seinen Herzschlag. Richtig für unbeaufsichtigte Arbeit —
aber man kann ihm nicht zusehen und nichts hineinrufen.

Mit `--imkr` startet der Lauf **interaktiv in einem neuen imkr-Fenster**. Dann
gilt:

| | beaufsichtigt | `--imkr` |
|---|---|---|
| Protokoll | `protokoll.txt` | der Schirm (`logs` zeigt ihn) |
| Zustand | Herzschlag der CLI | von imkr (`imkr status`-Hooks) |
| Zusehen, mitreden | nein | `imkr read`, `imkr msg` |
| Zeitgrenze | ja | nein (das Pane gehört imkr) |

Die CLI baut davon **nichts** nach. Was imkr kann — Panes, Agentenzustand,
Nachrichten zwischen Agenten, Übersicht über mehrere Rechner — bleibt bei imkr.

**imkr ist optional.** Ohne imkr fehlt nur diese Betriebsart. `imkr new`
startet allerdings keine Sitzung: `--imkr` geht nur aus einer laufenden
imkr-Sitzung heraus, und wer es von aussen versucht, bekommt das gesagt.

**Ein Pane in einem unbekannten Verzeichnis fragt erst nach Vertrauen.** Claude
Code tut das beim ersten Mal; im Druckmodus (`-p`) passiert es nicht, in einem
Pane schon. Die CLI sagt das vorher — und beantwortet die Frage nicht für dich.
Ein Flag in deine Konfiguration zu schreiben, um eine Sicherheitsabfrage
wegzuklicken, ist keine Bequemlichkeit.

Solange dort etwas fragt, meldet `agents` den Lauf als `wartet` mit Grund — und
**nicht** als `fertig`. Das ist nicht selbstverständlich: imkr meldet ein Pane,
das an einem Dialog hängt, als `done`. Ein Lauf gilt erst dann als fertig, wenn
er vorher gearbeitet hat.

### Rechte und Grenzen

Drei Stufen, aufsteigend:

| | claude | codex | braucht Freigabe |
|---|---|---|---|
| (ohne Schalter) | Vorgaben | Vorgaben | nein |
| `--schreibend` | `--permission-mode acceptEdits` | `-s workspace-write` | **ja** |
| `--vollzugriff` | `--permission-mode bypassPermissions` | `--dangerously-bypass-approvals-and-sandbox` | **ja** |

**Gesichert wird nicht „ein Agent läuft", sondern „ein Agent fragt nicht
mehr".** Ein Lauf ohne Schalter fragt, bevor er etwas tut — damit ist er nicht
gefährlicher als ein Mensch, der dort `claude` tippt, und er braucht keine
Freigabe. Das ist Absicht: Ein Riegel, der am Alltag hängenbleibt, wird
abgeschaltet.

```bash
ai-brain-harness freigabe                        # was freigegeben ist
ai-brain-harness freigabe add ~/Development      # mit Unterverzeichnissen
ai-brain-harness freigabe rm ~/Development
```

**Ohne Freigabedatei gibt es keinen Vollzugriff — nirgends.** Pfade werden
aufgelöst, nicht verglichen: `~/frei/../geheim` ist als Zeichenkette etwas
anderes als `/home/du/geheim`, als Verzeichnis aber dasselbe. Und ein Nachbar
mit gleichem Namensanfang (`Development-geheim` neben `Development`) gilt nicht
mit.

Das Heimatverzeichnis oder `/` freizugeben verlangt zusätzlich `--trotzdem` —
das wären auch `~/.ssh` und die Zugangsdaten dieser CLI.

**Zeitgrenze:** Vorgabe eine Stunde, `--zeitgrenze <sekunden>`, `0` hebt sie
auf. Ein unbeaufsichtigter Lauf, der hängt, hielte sonst ewig seinen Herzschlag
und sähe für jeden Leser gesund aus. Beim Erreichen wird der Lauf beendet und
mit Grund vermerkt.

Eine **Zug-Grenze gibt es nicht**: `claude` kennt kein `--max-turns`. Einen
Schalter anzubieten, den die Gegenseite nicht hat, wäre ein Versprechen ohne
Deckung.

**`codex exec` braucht ein Git-Verzeichnis.** Ausserhalb bricht es mit „Not
inside a trusted directory" ab, ohne etwas zu tun. Die CLI prüft das vorher und
sagt es — umgangen wird es nicht (`--skip-git-repo-check` wäre ein Riegel, den
wir für dich wegklicken).

Was `--modell ollama` angeht: noch nicht verdrahtet. Der Weg führt über die
codex-CLI mit eigener Basis-Adresse; bis dahin sagt die CLI das, statt etwas
anderes zu starten.

### Laufende Läufe ansehen

Die CLI hält mehrere Agentenläufe gleichzeitig und gibt Auskunft über sie.

```bash
ai-brain-harness agents                  # was hier läuft
ai-brain-harness agents --json           # dasselbe für Programme
ai-brain-harness logs <kennung>          # was ein Lauf geschrieben hat
ai-brain-harness logs <kennung> --zeilen 500
ai-brain-harness stop <kennung>          # einen Lauf beenden
```

```
● laeuft    1760440000-999     claude     #7665    1h06
✓ fertig    1760436210-8821    codex      ai-brain 4m
✗ fehler    1760430004-7719    claude     #7612    12m
              Aufseher verschwunden — kein Lebenszeichen seit 412 s
```

Vier Zustände: `laeuft`, `wartet` (der Agent hat eine Rückfrage), `fertig`,
`fehler`.

**Wie ein verschwundener Lauf erkannt wird.** Der Aufseher stempelt im Takt ein
Lebenszeichen. Bleibt der Stempel länger als drei Minuten stehen, gilt der Lauf
als verschwunden und erscheint als `fehler` mit Grund. Das ist bewusst nicht
„läuft der Prozess noch?": Diese Frage braucht entweder eine weitere
Abhängigkeit oder `/proc`, das es unter macOS nicht gibt — und sie übersieht den
Fall, dass ein Prozess lebt, aber nichts mehr tut.

**Der Befund wird geschrieben, nicht nur angezeigt.** `agents` korrigiert den
Zustand in der Datei. Sonst stünde dort für immer „laeuft", und jeder weitere
Leser — das imkr-Plugin, AI Brain — müsste dieselbe Herleitung nachbauen.

**Was wann verschwindet:** abgeschlossene Läufe nach 14 Tagen samt Protokoll.
Laufende nie, egal wie alt.

### Wo das liegt

| Pfad | Inhalt |
|---|---|
| `${XDG_STATE_HOME:-~/.local/state}/ai-brain-cli/laeufe/<kennung>/lauf.json` | Zustand des Laufs |
| `…/laeufe/<kennung>/protokoll.txt` | seine Ausgabe |

Unter Windows `%LOCALAPPDATA%\ai-brain-cli\laeufe`.

Bewusst **nicht** in `~/.config`: Dort liegen die Zugangsdaten, das wird
gesichert und zieht bei einer Umbenennung mit um. Protokolle laufender Agenten
haben da nichts zu suchen.

Die Kennung ist `<sekunde>-<prozess>`, bei Kollision mit Zusatz. Der Ordner wird
mit `create_dir` beansprucht — das scheitert, wenn es ihn schon gibt, und ist
damit auch zwischen zwei Prozessen eindeutig.

## Die Maske

`brain` öffnet die Eingabemaske. Sie gehört uns — Eingabe, Darstellung und
Buchführung; der Motor wird pro Zug gerufen und sieht das Terminal nie.

```
▰ AI Brain Harness · AI Brain
──────────────────────────────────────────────────────────────────
Wer        Bastian auf omarchy  ·  Konto claude:henneberg
Motor      claude · Vorgabe des Werkzeugs
Hier       ai-brain · ~/Development/ai-brain
Greift auf AI Brain · diesen Rechner  ·  lesend — nachsehen ja, anfassen nein
──────────────────────────────────────────────────────────────────
Lauf 1792069936-409863  ·  /hilfe für Befehle ohne Modell  ·  Ctrl-D beendet

▸ Führe aus: sleep 8 && echo FERTIG
  ⚒ Bash  sleep 8 && echo FERTIG
  ⠹ Bash arbeitet · 8s

FERTIG
  0.5144 $
```

Die letzte Zeile vor der Antwort überschreibt sich selbst und zählt mit. Das
ist kein Zierrat: während ein Werkzeug arbeitet, kommt vom Motor **nichts** —
ein Befehl mit 60 Sekunden Frist sah vorher aus wie ein Hänger, und man drückt
Ctrl-C auf etwas, das gerade richtig läuft.

Der Werkzeugname kommt aus dem Blockanfang, nicht aus der fertigen Nachricht.
Das ist der Unterschied zwischen „ich sehe, was er tut" und „ich sehe
hinterher, was er getan hat". Die Argumente stehen daneben: wer einem Programm
die Hoheit über seinen Rechner gibt, soll sehen, womit es sie benutzt.

### Befehle ohne Modell

Eine Nachfrage ist keine Denkaufgabe. Diese Befehle laufen ohne Motor und
kosten nichts:

| Befehl | Was er zeigt |
|---|---|
| `/aufgaben` | offene Aufgaben des Projekts |
| `/auftraege` | Aufträge, die für diesen Rechner anliegen |
| `/daueraufgaben` | eingerichtete Daueraufträge dieses Rechners |
| `/erinnerungen` | Gedächtnis mit AI Brain abgleichen |
| `/dienste [namen]` | Zustand von Diensten (ohne Namen: alle fehlgeschlagenen) |
| `/rechner` | Last, Speicher, Platten, Grafikkarte |
| `/konto [name]` | welches Konto arbeitet — ohne Name: beide Quellen |
| `/rechte [stufe]` | `lesend` oder `steuernd` |
| `/neu` | Verlauf fallen lassen |
| `/hilfe` | diese Liste (auch `/help`, `/?`) |
| `/ende` | Schluss (auch `/quit`, `/exit`, oder Ctrl-D) |

### Konten

`/konto` fragt **zwei** Quellen und kennzeichnet beide:

```
  Auf diesem Rechner (imkr)
    claude
      stelljes · henneberg (standard) · department
    codex
      henneberg (standard)

  In AI Brain
    Stelljes                     claude  …9gAA
```

Wer imkr benutzt, hat seine Konten dort; wer nicht, hat sie in AI Brain. Fällt
eine Quelle aus, verschluckt das die andere nicht.

Ein Wechsel beginnt ein **neues** Gespräch: der Verlauf gehört dem Konto, mit
dem er begonnen hat. Gewechselt wird derzeit nur zu imkr-Konten — ob ein Gerät
das Token der Instanz abholen darf, ist eine offene Entscheidung.

## Dieser Rechner als Werkzeug

Neben den Werkzeugen von AI Brain hängt an jedem Zug ein zweiter
Werkzeugserver: dieser Rechner.

| Werkzeug | Was es tut | Braucht steuernde Rechte |
|---|---|---|
| `rechner-dienste` | Zustand von systemd-Units; ohne Argument alle fehlgeschlagenen | — |
| `rechner-datentraeger` | Belegung der Dateisysteme und Netzlaufwerke | — |
| `rechner-last` | Laufzeit, Last, Speicher, Grafikkarte | — |
| `rechner-fenster` | offene Fenster der grafischen Sitzung | — |
| `rechner-bildschirmfoto` | Bildschirmaufnahme (grim) | — |
| `rechner-befehl` | Shell-Befehl als angemeldeter Nutzer | ja |
| `rechner-dienst-steuern` | Unit starten, stoppen, neu starten | ja |
| `rechner-tippen` | Text in das Fenster mit dem Fokus (wtype) | ja |
| `rechner-taste` | Taste oder Kombination drücken | ja |

Unter `lesend` **fehlen** die ändernden Werkzeuge in der Liste, statt im Aufruf
abgelehnt zu werden. Ein Werkzeug, das dasteht und dann „darfst du nicht" sagt,
lädt zum Probieren ein und kostet jedes Mal einen Zug.

Die Rechte stehen in der Umgebung (`AI_BRAIN_RECHNER_RECHTE`), nicht im Aufruf:
ein Modell kann sie im laufenden Prozess nicht erweitern.

Bildschirm, Tastatur und Fenster brauchen eine laufende grafische Sitzung. Aus
einer reinen Textsitzung heraus wird das gesagt, statt mit „failed to retrieve
wayland globals" zu scheitern — eine Meldung, die nach einem fehlenden
Programm klingt, obwohl nur die Sitzung fehlt.

## Daueraufträge

Ein Dauerauftrag ist eine wiederkehrende Aufgabe, die in AI Brain als **Daten**
liegt. Man legt sie im Gespräch an — „überwache das Restic-Backup" — und der
Harness tut es selbst über `create-device-job-tool`. Kein Code, kein Deploy.

```bash
ai-brain-harness daueraufgaben              # was eingerichtet ist
ai-brain-harness daueraufgaben --fahren     # Fälliges abarbeiten und melden
ai-brain-harness taktgeber --einrichten     # den Abholer als systemd-Nutzer-Timer
ai-brain-harness taktgeber                  # läuft er? wann zuletzt?
ai-brain-harness taktgeber --entfernen
```

`art: befehl` läuft **ohne Modell**. Für Überwachung ist das der Normalfall:
„ist der Dienst aktiv", „ist der Snapshot jünger als 26 Stunden" sind
Schwellenfragen. Ein Modell dafür zu starten kostet bei jedem Durchlauf Geld
für eine Antwort, die feststeht. `art: auftrag` ist für das, was wirklich
Urteil braucht — und wird vom Taktgeber bewusst übersprungen: ein Zeitplan, der
unbeaufsichtigt Modelle anwirft, ist eine Kostenquelle ohne Aufsicht.

### Die Stillfrist ist der Kern

Jeder Dauerauftrag hat eine Stillfrist. Bleibt die Meldung länger aus, entsteht
in AI Brain eine Aufgabe.

Das ist nicht Bequemlichkeit, sondern der Grund für das Ganze. Ein Wächter
meldet sich, **wenn** etwas nicht stimmt — ein abgeschalteter meldet sich
ebenfalls nicht und sieht damit genauso aus wie ein zufriedener. Von diesen
beiden Zuständen ist der falsche der beruhigende.

Auf diesem Rechner stand ein Backup-Wächter dreieinhalb Monate in einer Liste
und ist **nie gelaufen**. Niemand hat es gemerkt, weil Pausiert wie Ruhe
aussieht. Deshalb gilt ein nie gelaufener Dauerauftrag ab seinem Anlegen als
verstummt, nicht erst ab der ersten Meldung.

Und deshalb überwacht der Taktgeber sich **nicht** selbst — das wäre derselbe
Zirkel. Bleibt er stehen, kommen keine Meldungen, und die Gegenstelle schlägt
Alarm.

## Live-Leitung

Die Leitung zeigt ein imkr-Pane dieses Rechners live in AI Brain und nimmt
Tastendrücke von dort an. Der Rechner verbindet sich dafür **ausgehend**
(Reverb, Pusher-Protokoll); AI Brain ruft hier nie etwas auf.

```bash
ai-brain-harness leitung an --dauer 30m     # von Hand einschalten, höchstens 8 h
ai-brain-harness leitung status [--json]    # Schalter, Grund, Prozess, Verbindung
ai-brain-harness leitung aus                # auflegen, bis du wieder einschaltest
ai-brain-harness leitung dienst             # halten und selbst an-/ausschalten
ai-brain-harness leitung autostart          # den Dienst beim Anmelden starten
```

Gezeigt werden nur Panes dieses Rechners, und Tastendrücke stehen nie im
Protokoll.

### Der Schalter: drei Zustände, und die Reihenfolge ist die Regel

Bis zum 27.10.2026 war der Schalter einfach: aus, bis jemand `leitung an`
tippt. Seit #8932 schaltet der Rechner auch selbst — sobald ein Channel ihn in
AI Brain freigegeben hat. Damit gibt es drei Einträge in `leitung.json`, und
**sie werden in dieser Reihenfolge gelesen**:

| `veto` | `bis` gültig | `auto` | Ergebnis | |
|---|---|---|---|---|
| **ja** | — | — | **aus** | Ein Mensch hat abgeschaltet |
| nein | **ja** | — | **an** | Von Hand, mit Restzeit |
| nein | nein | **ja** | **an** | Automatisch, mit Bedingung |
| nein | nein | nein | **aus** | Niemand hat eingeschaltet |

**Das Veto steht ganz oben, und es hat keine Frist.** `leitung aus`, das
Häkchen „Live-Leitung" im Tray-Menü und der **Linksklick** auf das Symbol
schreiben es; `leitung an` und dieselben zwei Wege im Tray löschen es — und
nichts sonst. (Am Linksklick hing das schon bis v0.5.0; von v0.5.3 bis v0.6.0
öffnete er stattdessen ein Menü, seit v0.6.1 schaltet er wieder.) Wäre „aus"
bloss ein gelöschtes `auto`, würde die nächste
Auskunft von AI Brain es binnen einer Minute wieder setzen: ein Schalter, der
sich selbst zurückstellt, ist kein Schalter. Und ein Veto mit Ablaufzeit ginge
wieder an, wenn niemand hinsieht.

**Von Hand behält die Höchstdauer von 8 h, automatisch hat keine.** Der Deckel
existiert, weil niemand ans Ausschalten denkt — eine Bedingung, die jede Minute
neu gelesen wird, tut das strikt besser, und sie hängt am Besitzer in AI Brains
Maske statt an dem, der zuletzt am Rechner sass. Läuft die Hand-Frist ab,
übernimmt die automatische Leitung; es gibt dazwischen kein Loch.

Ein `bis` jenseits der Höchstdauer gilt als nicht gesetzt — den hat niemand
über `leitung an` geschrieben, und eine von Hand verlängerte Datei ist genau
der Fall, gegen den der Deckel schützt. Eine unlesbare Datei ist **aus**.

### Automatisch an — woher der Rechner das weiss

```
ai-brain-harness leitung autostart     # einmal, oder beim Installieren
```

Danach läuft `leitung dienst` ab jedem Anmelden. Er fragt
`GET /api/v1/geraete/ich`; dort steht seit #8932

```json
"imkr": { "freigegeben": true, "sehen": 1, "befehlen": 1, "channels": ["avatar-1"] }
```

Steht dort `freigegeben: true`, setzt er `auto` — und die Leitung geht an, ohne
dass jemand etwas tippt. Steht dort `false`, bleibt sie aus. Solange sie steht,
bringt **jeder Herzschlag** die Angabe mit (`200 {"freigabe": …, "channels": […]}`):
Fällt der Haken in AI Brain, verschwindet `auto`, und die Leitung legt binnen
einer Sekunde selbst auf — derselbe Weg wie beim Ablauf des Hand-Schalters.

Die **Channel-Namen** gehen mit, nicht nur das Ja, und sie kommen auf jedem
Schlag mit statt nur beim Aufbau: Der Tray zeigt sie (`an — automatisch,
Freigabe avatar-1`), und ein zweiter Haken, der dazukommt, während die Leitung
steht, würde die Anzeige sonst still unvollständig machen. „Automatisch an"
beantwortet nur die halbe Frage; die ganze lautet *wer sieht da mit?*.

**Keine Auskunft ist nicht „nein".** Kein Netz, eine Instanz ohne das Feld, eine
kaputte Antwort: Dann öffnet die automatische Leitung nicht — und sie legt auch
keine auf, die zu Recht stand. Von Hand einschalten geht unabhängig davon
weiter.

**Das verschiebt die Sperre nicht.** Ob ein Agent etwas darf, prüft AI Brain bei
jedem Aufruf gegen die Freigabe je Channel und Gerät. Eine offene Leitung ohne
Freigabe ist nutzlos, nicht erlaubt — sie trotzdem zu schliessen ist Hygiene.

**Kein systemd-Dienst**, sondern derselbe Autostart-Mechanismus wie beim Tray
(`ai-brain-autostart`: XDG-`.desktop`, LaunchAgent, Windows-Run-Key). Die
Leitung gehört in die Nutzersitzung — sie spricht mit dem imkr-Server dieses
Nutzers und liest seinen Konfigurationsordner. Ein Systemdienst wäre der
falsche Geltungsbereich, und `systemd --user` gäbe es nur unter Linux. Ohne
`KeepAlive`: Was sich erholen kann, erholt sich in der eigenen Schleife, und
ein 403 ändert sich durch Neustarten nicht.

Der Dienst hält die PID-Datei für seine ganze Lebenszeit, auch während er
wartet. Sonst startete `leitung an` einen zweiten Prozess daneben, und zwei
Leitungen auf einem Gerät schicken sich gegenseitig Endemarker. Stattdessen
schreibt `an` nur den Schalter, und der Dienst wacht binnen einer Sekunde auf.

Solange die Leitung steht, schickt sie alle 60 s einen Herzschlag
(`POST /api/v1/geraete/herzschlag` mit Harness-Version, imkr-Version und
gezeigtem Pane), unabhängig davon, ob jemand zusieht. Daran erkennt AI Brain,
ob das Gerät für Agent-Befehle erreichbar ist. Kennt eine Instanz den
Endpunkt noch nicht (404), schlägt die Leitung still weiter.

Das Protokoll (`leitung.log` neben der Verbindung, `leitung status` nennt den
Pfad) wird über Neustarts fortgeschrieben, damit die letzten Zeilen vor einem
Abriss erhalten bleiben. Ab 1 MB liegt der alte Teil als `leitung.log.1`.

### Was ein Agent über die Leitung darf

Über dieselbe Leitung kommen nicht nur Tastendrücke eines Menschen, sondern
auch Aufträge der MCP-Werkzeuge eines Channels (`imkr-sitzungen`, `imkr-lesen`,
`imkr-befehl`). Jeder Auftrag ist eine **eigene Art**, und jede Art hat ihre
eigene Befehlszeile, die der Harness selbst baut:

| Art | Ausgeführt wird | Nutzlast | Freigabe in AI Brain |
|---|---|---|---|
| `sitzungen` | `imkr ls --json` | nur `auftrag`, `frist` | „darf Panes sehen" |
| `lesen` | `imkr read <pane> --lines <n>` | `pane`, `zeilen` | „darf Panes sehen" |
| `befehl` | der mitgeschickte Befehl im Pane | `pane`, `befehl` | „darf Befehle schicken" |

**Warum das drei Arten sind und nicht eine mit Text darin.** Wäre „auflisten"
ein `befehl` mit `imkr ls --json` als Text, hinge die Grenze zwischen sehen
und befehlen an einer Zeichenkette, die der Aufrufer formuliert: Wer
`imkr ls --json; rm -rf ~` schreiben kann, hat aus einem Leserecht ein
Ausführungsrecht gemacht, ohne irgendeine Prüfung zu umgehen. Deshalb tragen
`sitzungen` und `lesen` **kein Textfeld** — bei `sitzungen` nicht einmal eine
Zahl. Es gibt nichts, in das ein Befehl passen würde.

Kein `--all`: Das würde über ssh die Nachbarrechner befragen. Der Harness
bleibt strikt lokal, und eine Freigabe gilt für **diesen** Rechner.

**`sitzungen` schickt keine Bildschirminhalte.** `imkr ls --json` legt zu einem
wartenden Agenten ein `tail` bei — den unteren Rand seines Bildschirms. Der
Harness projiziert die Antwort deshalb auf eine **Positivliste**: Pro Sitzung
Name, Fenster- und Pane-Zahl, die Zustandszählung, `ruht` und die flache
`pane_liste`; pro Pane darin `adresse` (`sitzung/nummer`, genau die Form, die
`lesen` und `befehl` erwarten), `pane`, `fenster`, `fenster_name`, `programm`,
`zustand`, `seit`, `meta` und `titel`. Alles andere nicht.

Positivliste und nicht ein Verbot von `tail`: Beides wirkt heute gleich, der
Unterschied kommt mit der nächsten imkr-Fassung. Legt die ein Feld dazu — eine
Vorschau, eine Zwischenablage —, fährt es bei einer Sperrliste still mit. Hier
muss jemand es erst hinschreiben. Gefiltert wird auf dem **Rechner**, nicht bei
AI Brain: Was einmal draußen ist, steht in einem Auftragsbuch und einem
Protokoll, und das holt ein nachträglicher Filter nicht mehr ein.

Lässt sich die Antwort nicht zerlegen, wird der Auftrag **abgelehnt** statt
ungefiltert gemeldet — was wir nicht lesen können, können wir auch nicht
säubern.

**Die Liste wird vorher gedeckelt, nicht hinterher gekürzt.** Ein Ergebnis über
16 KB geht sonst durch dieselbe Kürzung wie eine Befehlsausgabe — und die
behält das *Ende* mit einem Vermerk davor. Bei einer Befehlsausgabe ist das
richtig, bei JSON ergibt es ein ungültiges Dokument, und der Agent erfährt
nicht warum. Gemessen am 27.10.2026: 16 Sitzungen mit 10 Panes wogen kompakt
3,3 kB; ein Rechner mit 16 *laufenden* Sitzungen à 5 Panes läge darüber.

Passt es nicht, fallen die `pane_liste`n der letzten Sitzungen weg — von
hinten, weil jede andere Wahl hieße, hier zu entscheiden, welche Sitzung
wichtiger ist. **Jede Sitzung bleibt in der Liste** (sonst erfährt ein Agent,
sie gäbe es nicht), die betroffenen tragen `pane_liste_fehlt: true`, und oben
steht `gekuerzt` mit der Zahl. Ausgegeben wird kompaktes JSON: Einrückung
kostete 70 % mehr Bytes, und die gehen von den Panes ab, die noch hineinpassen.

### Der Tray

`ai-brain-tray` ist ein eigenes Programm für Arbeitsrechner mit Desktop. Es
zeigt **zwei** Zustände und schaltet einen davon. Die Regeln bleiben im Harness: der Tray ruft nur `leitung status
--json`, `an` und `aus`.

```bash
cargo build --release -p ai-brain-tray
ai-brain-tray --autostart                   # beim Anmelden starten
ai-brain-tray --pruefen                     # gibt es hier eine Leiste?
```

#### Zwei Zustände, nicht einer

| Zeile | Was sie sagt |
|---|---|
| `Mit AI Brain verbunden (Gerät …)` | Es liegt ein Gerätezugang vor. |
| `Live-Leitung (AI Brain sieht deine Panes): an, noch 2 h` | Von Hand, mit Restzeit. |
| `… : an — automatisch, Freigabe avatar-1` | Der Dienst hält sie, und wer mitsieht. |
| `… : aus — abgeschaltet, bis du wieder einschaltest` | Jemand hat das Veto gesetzt. |
| `… : aus` | Niemand hat eingeschaltet, und es gibt keine Freigabe. |
| `Verbunden, zeigt bau/2` | Wie es der laufenden Leitung geht. |

Der Zusatz in Klammern ist länger als schön. `Live-Leitung: aus` versteht aber
nur, wer weiß, was die Leitung tut — und ein Schalter, dessen Wirkung man nicht
kennt, wird entweder nie oder versehentlich benutzt.

**Die Zeile hängt seit #8932 an `an`, nicht mehr an `bis`.** Eine automatische
Leitung hat keine Restzeit, also ist `bis` dort `null` — und bis dahin hiess
das „aus". In der Leiste hätte „aus" gestanden, während AI Brain mitliest:
dieselbe Verwechslung wie #8914, nur gefährlicher. Das Wort daneben kommt aus
dem neuen Feld `grund` (`von Hand`, `automatisch`, `abgeschaltet`, `aus`); ein
älterer Harness schickt es nicht, und dann gilt wieder `bis` allein.

**`aus` und `abgeschaltet` stehen getrennt da.** „aus" liest sich wie „kommt
gleich wieder"; bei einem Veto kommt sie nicht wieder, und das ist der Punkt.
Ein wartender Dienst auf einem Rechner ohne Freigabe ist dabei **kein Fehler** —
er bleibt blau, nicht rot: Sonst sähe jeder Rechner ohne Haken gestört aus.

Der **Gerätename** kommt aus AI Brain, nicht aus der `connection.json`: Dort
steht er als Abschrift vom Kopplungstag, und wer den Zugang später unter „Meine
Geräte" umbenennt, ändert sie nicht mit. Gemeldet wurde das am Tray — dort stand
`Gerät omarchy (linux x86_64)`, und weil Desktop und Laptop beide `omarchy`
heißen, war die Anzeige richtig und trotzdem nutzlos.

`GET /api/v1/geraete/ich` nennt den Namen; der Harness legt ihn als
`geraet.json` neben die Verbindung und fragt höchstens alle sechs Stunden neu.
Mit vier Sekunden Frist statt fünfzehn, und der Zeitstempel wird **vor** der
Anfrage geschrieben: Der Tray liest den Status im Zwei-Sekunden-Takt, und eine
Instanz, die nicht antwortet, darf ihn nicht bei jedem Takt hängen lassen. Bis
eine Antwort kommt, steht der Name vom Koppeln da — nie ein leerer Platz.

Bis zum 27.10.2026 stand dort nur `Live: aus` — und das hat wie **nicht
verbunden** gewirkt, obwohl der Rechner an seiner Instanz hing. Zwei Zustände
in einer Zeile zusammenzufassen spart eine Zeile und kostet die Auskunft.

Die Verbindung ist der *Zugang*, nicht die Erreichbarkeit: Dass die Instanz in
dieser Sekunde antwortet, steht dort nicht. Der Tray liest den Status alle zwei
Sekunden, und eine HTTP-Anfrage in diesem Takt wäre Dauerlast für eine
Auskunft, die niemand so oft braucht. Läuft die Leitung, sagt ihre `lage`
ohnehin, ob sie steht.

#### Bedienung und Symbol

- **Linksklick schaltet die Live-Leitung** — an für 30 Minuten, oder aus.
  Genau das, was das Häkchen im Menü tut; es gibt die Regel nur einmal
  (`kern::schritt`). Ohne Antwort vom Harness schaltet er **nicht**: Ein Klick,
  der im Zweifel das Gegenteil tut, wäre der nächste Ärger.
- **Rechtsklick öffnet das Menü** — und es ist das einzige. Gezeigt wird es von
  der Leiste, aus unserem DBusMenu.
- **„AI Brain öffnen"** steht als Punkt im Menü: die Adresse der Instanz, mit
  der dieser Rechner verbunden ist. Ohne bekannte Adresse ist der Punkt
  ausgegraut — eine erfundene URL wäre schlimmer als ein Klick, der nichts tut.

**Was unter Omarchy fehlt, und zwar benannt statt behauptet:** Ein zweiter
Rechtsklick schließt die Karte der Leiste nicht, und Esc schließt sie auch
nicht. Ein Klick daneben schließt sie. Warum das nicht an uns liegt, steht
zwei Abschnitte weiter unten; gemeldet gehört es nach oben (Task #8958). Auf
waybar, KDE, GNOME, macOS und Windows geht beides.

**Sechs Fassungen hatte dieser Klick, und jede hat den nächsten Punkt
freigelegt (Stand 29.10.2026).**

1. **Gar nichts** (bis v0.4.1): `Activate` war nicht behandelt. In den meisten
   Leisten öffnet der *Rechtsklick* das Menü und der Linksklick löst
   `Activate` aus — ohne Behandlung tat er also nichts, und von außen sah das
   aus wie ein Tray, der sich nicht schalten und nicht beenden lässt. Ein
   Klick, der nichts tut, ist schlimmer als kein Klick: Er lässt am Programm
   zweifeln statt an der Bedienung.
2. **Die Leitung schalten** (v0.4.2): löste den toten Klick. Ein Symbolklick,
   der etwas schaltet, ist aber in keiner anderen Leiste so — die Umschaltung
   sitzt seit v0.5.0 deshalb als Häkchen im Menü.
3. **Das Menü der Leiste öffnen** (v0.5.0, `ItemIsMenu` / `MENU_ON_ACTIVATE`):
   in Omarchys Leiste ließ sich das Menü per Linksklick nicht mehr schließen
   (Bug #1124).
4. **AI Brain öffnen** (v0.5.1): Rechtsklick fürs Menü, Linksklick für die
   Anwendung — „wie die anderen". Grundlage war Bastians Antwort auf eine
   Rückfrage, und die haben wir anders gelesen, als sie gemeint war.
5. **Zurück zum Menü der Leiste** (v0.5.3), auf Ansage: *„Der Harness macht
   jetzt auf Klick AI-Brain auf, aber der sollte doch einfach nur das
   Tray-Menü aufmachen. Mit Linksklick."* Damit war der Klick richtig — und
   die Kehrseite aus Punkt 3 wieder da: Er schloss das Menü nicht.
6. **Das eigene Menüfenster** (v0.5.4 bis v0.6.0), damit der zweite Klick es
   schließt. Es schloss — und legte frei, dass es damit **zwei** Menüs über
   dieselbe Sache gab: unser Fenster am Linksklick, das der Leiste am
   Rechtsklick.

**Jetzt (v0.6.1) schaltet der Linksklick, und es gibt wieder ein Menü.**
Bastian am 29.10.2026, wörtlich: *„Es gibt jetzt ein Rechtsklick und ein
Linksklick-Tray-Menü. Das macht ja keinen Sinn. … Sonst müssen wir linksklick
Schalter machen, also dass man es damit anmacht und ausmacht, so wie es vorher
war und rechtsklick öffnet das Menü, aber dann muss rechtsklick das Menü auch
wieder schließen."*

Die erste Hälfte steht so im Programm. Die zweite ist auf Omarchy nicht zu
bauen, und das ist kein Vorsatz, sondern gemessen — siehe den nächsten
Abschnitt. Weil der Linksklick dort der **einzige** Klick ist, der bei uns
ankommt, schließen sich „links schaltet" und „unser Fenster bleibt
erreichbar" aus: Das Menü muss erreichbar bleiben, also zeigt es die Leiste.

> Dieselbe Falle wie in Fassung 3 und 6, nur eine Ebene höher: Jede Fassung
> hat getan, was zuletzt gewünscht war, und dabei die nächste Kehrseite
> freigelegt. Was sie *nicht* getan hat, ist beide Hälften der Bitte vorher
> gegeneinander zu prüfen. Diese Fassung sagt deshalb von vornherein, was sie
> **nicht** kann.

#### Warum der Rechtsklick nicht bei uns ankommt (gemessen 29.10.2026)

Omarchy 4.0.4-1, quickshell 0.3.1 — nachgelesen im Quelltext der Leiste und im
Programm selbst:

| Frage | Befund |
|---|---|
| Wie meldet quickshell den Rechtsklick? | `plugins/bar/widgets/Tray.qml`, `MouseArea.onPressed` → `displayMenu()` → `openTrayMenu()`. Bieten wir ein DBusMenu an, endet das **immer** mit `trayMenuOpen = true` — die Karte der Leiste, ohne Umschalten. `close()` ist vorhanden, wird vom Klickpfad nie gerufen |
| Und wenn wir keins anbieten? | `openTrayMenu` nimmt den Zweig `!item.menu` und ruft `item.display(…)`. Das endet in quickshell mit `qCritical "No menu present for"`: es passiert **nichts** |
| Ruft quickshell `ContextMenu`? | **Nie.** Im Programm gibt es genau eine Fundstelle der Zeichenkette, und die liegt im `qt_static_metacall` des erzeugten DBus-Stellvertreters. Die QML-Schnittstelle (`quickshell-service-statusnotifier.qmltypes`) kennt nur `activate`, `secondaryActivate`, `scroll` und `display` — kein `contextMenu` |
| Könnte ein spec-treuer Host uns erreichen? | Heute nicht: `ksni` 0.3.7 beantwortet `ContextMenu` selbst mit `UnknownMethod` und gibt die Eigenschaft `Menu` fest als `/MenuBar` heraus. „Kein DBusMenu anbieten" geht damit gar nicht |
| Schließt ein Klick daneben? | Ja. `Ui/PopupCard.qml` hat einen `HyprlandFocusGrab` über `[Karte, Leistenfenster]` — alles außerhalb löst ihn und schließt. Ein Klick **auf** der Leiste nicht, und eine Tastenbehandlung gibt es dort nicht, also auch kein Esc |

**Zwei Folgen, und sie hängen zusammen.** Dass die Karte beim zweiten
Rechtsklick nicht zugeht, können wir nicht beheben: StatusNotifierItem kennt
keinen Weg, dem Host „schließ mein Menü" zu sagen — das Menü ist sein Fenster,
wir liefern über DBusMenu nur den Inhalt. Und dass unser eigenes Fenster nicht
an den Rechtsklick kann, liegt daran, dass dieser Klick die Leiste nie verlässt.

Geprüft und verworfen wurden außerdem:

| Weg | Warum nicht |
|---|---|
| Eine Option in Omarchy/quickshell | Es gibt keine. `~/.config/omarchy/shell.json` kennt zum Tray nur Layout, `pinned` und `hidden` |
| Die drei Zeilen in `Tray.qml` ändern | Eine Paketdatei, beim nächsten `omarchy`-Update weg. Gehört nach oben gemeldet, nicht hierher kopiert |
| Item abmelden und neu anmelden | Symbol flackert, Leiste sieht einen Verbindungsabbruch. Tauscht den Fehler gegen einen schlimmeren |
| Eine eigene Fläche dauerhaft über dem Symbol | Wo das Symbol liegt, wissen wir nicht verlässlich — der Bereich fährt ein und aus, Symbole kommen und gehen. Und sie würde Hover und Tooltip der Leiste schlucken |
| Ein leeres DBusMenu anbieten | Die Karte geht trotzdem auf: `fittedContentHeight` setzt den Innenabstand als Untergrenze, es erscheint ein leerer Kasten von etwa 232×20 px |
| Bei `AboutToShow` einen Auffänger aufziehen | Er läge **über** der Karte und würde deren Zeilen schlucken. Nur die Leistenzeile abzudecken wäre denkbar — vorgemerkt, nicht gebaut, weil es daran hängt, wie Hyprland den Fokus-Griff löst |

#### Das eigene Menüfenster (Linux/Wayland) — seit v0.6.1 der Rückweg

Von v0.5.4 bis v0.6.0 war das die Vorgabe; jetzt steht es unter
`AI_BRAIN_TRAY_MENUE=eigen`. Gelöscht ist es nicht: Es ist der Weg, der an
diesem Klick alles kann, was gewünscht war — nur eben am **linken**. Wer es
setzt, bekommt zum Preis des Schalters zwei Menüs zurück (unser Fenster links,
das der Leiste rechts).

`ItemIsMenu` steht seit v0.5.4 auf `false`, der Linksklick kommt also als
`Activate` an — und in dieser Betriebsart spannt der Tray dann **zwei** Arten
von Flächen auf (`zwlr_layer_shell_v1`, Ebene `Overlay`):

| Fläche | Größe | Wozu |
|---|---|---|
| **Auffänger**, je Ausgabe | ganzer Schirm, durchsichtig | fängt jeden Klick daneben |
| **Kasten**, einmal | so groß wie das Menü | das Menü selbst |

Der Auffänger liegt **über** der Leiste. Damit lösen sich alle drei Fälle von
selbst, und zwar ohne einen Zustand „Menü offen", den zwei Programme gleich
sehen müssten — die Fläche *ist* der Zustand:

1. Linksklick aufs Symbol → `Activate` → Menü auf.
2. Zweiter Linksklick → trifft den Auffänger, nicht mehr das Symbol → zu.
   Beim dritten Klick gibt es keine Fläche mehr, `Activate` kommt wieder an.
3. Klick irgendwo daneben → derselbe Weg, auf jedem Schirm.

Gezeichnet wird in reinem Rust: `wayland-client` ohne `libwayland`,
`fontdue` für die Buchstaben, kein GTK, kein `libxkbcommon`.

#### Es sieht aus wie Omarchys Menü, weil es dieselben Zahlen liest (v0.5.6)

Dieser Abschnitt gilt für das **eigene** Fenster, also für
`AI_BRAIN_TRAY_MENUE=eigen`. In der Vorgabe seit v0.6.1 zeigt die Leiste ihre
eigene Karte — die sieht aus wie Omarchys Menü, weil sie es *ist*.

Bis v0.5.5 nahm das Fenster *ungefähr* die Farben des Themes und sonst eigene
Masse. Seit v0.5.6 liest es die Zahlen, die Omarchys Tray-Menü selbst
benutzt — abgelesen an `Ui/PopupCard.qml` und `plugins/bar/widgets/Tray.qml`:

| Stelle | Woher |
|---|---|
| Polster der Karte | `Style.space(8)` |
| Rahmen | **Vordergrund bei 0.45**, Breite `max(1, space(2))` |
| Radius | Hyprlands `decoration:rounding` — auf einer Vorgabe-Installation **0** |
| Zeilenhöhe | `Style.space(30)` |
| Hover | `[controls] hover-cursor-color` bei `hover-cursor-fill-alpha` (0.08), über die **ganze** Zeile, Radius `max(2, radius)` |
| Trenner | 1 px, eingerückt `space(10)`, `popups.border` bei 0.45 |
| Häkchen | Spalte `space(22)`, Zeichen U+F00C, Text ab `space(28)` |
| Schrift | `Style.font.bodySmall` = `round(base-size × 0.917)`, Familie aus `fc-match monospace` |
| Ausgegraut | die ganze Zeile auf 0.45 |
| Abstand zur Leiste | `general:gaps_out / 2` (Omarchys `Style.gapsOut`) |

Gelesen wird **erst** `<theme>/shell.toml`, **dann**
`~/.config/omarchy/shell.toml` darüber — dort schreibt
`omarchy display text size` seine `[font] base-size`, und daran hängt die
ganze Grössenrechnung (`space(px) = round(px × scale × base-size/12)`).

Drei Dinge, die beim Nachbauen überraschen und in `tray/src/popup/farben.rs`
begründet stehen:

- Der Rahmen ist **nicht** `popups.border` (im Theme oft ein deckendes Blau).
  Das Tray-Menü überschreibt ihn ausdrücklich mit dem Vordergrund bei 0.45.
- `[spacing] popup-row-height` und `popup-padding` wirken **nicht**: Omarchys
  Tray-Menü setzt beides als Literal und liest die Token nicht.
- Eine überfahrene Zeile wechselt **nicht** die Textfarbe. Was den Zustand
  zeigt, ist allein die Fläche darunter.

**Die Lage ist die von Omarchys Popup**, nicht die der Maus: mittig unter dem
Symbol, direkt unter der Leiste, in die Leiste hinein geklemmt. Welche Fläche
die Leiste ist, sagt `hyprctl layers` — genommen wird die oberste, die den
Klick enthält und *dünn* ist (höchstens die halbe Ausgabe in einer Richtung).
Ohne die Dünn-Regel gewinnt der bildschirmgrosse Hintergrund, und der Kasten
landet unten am Rand; genau so ist es am 28.10.2026 gemessen worden.

**Ein Themewechsel kommt von selbst mit.** Das Menü ist ein eigener Prozess,
der bei jedem Klick neu startet und dabei das Theme liest — es gibt keinen
Zustand, der veralten könnte, und deshalb auch keinen Wächter.

Die eine bewusste Abweichung: Omarchys Karte ist **fest** `space(232)` breit
und kürzt alles Längere mit „…". Hier ist 232 die *Mindest*breite — ein kurzes
Menü sieht damit genauso aus, „Leitung aufräumen (Prozess läuft ohne
Schalter)" bleibt aber lesbar.

Ohne Theme (jeder Rechner, der nicht Omarchy ist) gilt eine dunkle Vorgabe mit
denselben Massen.

```bash
# Die Karte als Bild, ohne Compositor und ohne Leiste — dieselbe Rechnung,
# dieselben Themefarben, dieselbe Schrift:
ai-brain-tray --menue-bild /tmp/menue.ppm < menue.json
magick /tmp/menue.ppm /tmp/menue.png
```

Das gibt es, weil „sieht es aus wie Omarchys Menü?" eine Frage an ein Bild
ist — und auf einem Rechner ohne Screencopy (codingmachine) oder ganz ohne
Sitzung (CI) sonst niemand sie beantworten kann.
`AI_BRAIN_TRAY_MENUE_SKALIERUNG` und `AI_BRAIN_TRAY_MENUE_HERVOR` stellen
Skalierung und hervorgehobene Zeile ein.

**Seit v0.6.1 wird beim Start nicht mehr nach layer-shell gesehen** — die
Vorgabe braucht keins, und eine Messung, an der nichts hängt, ist eine Zeile,
die veralten kann. Nur `eigen` braucht es; fehlt es dort, schaltet der
Linksklick und das Programm schreibt den Grund ins Protokoll statt einen
Klick, der nichts tut.

`AI_BRAIN_TRAY_MENUE` legt die Bedienung von Hand fest:

| Wert | Linksklick | Rechtsklick |
|---|---|---|
| `schalter`, oder nichts | Live-Leitung an (30 min) / aus | **das** Menü, von der Leiste gezeigt |
| `eigen` | unser Menüfenster auf und zu | Menü der Leiste — also zwei |
| `leiste` | Menü der Leiste | Menü der Leiste |

`AI_BRAIN_TRAY_MENUE_LAUT=1` schreibt jedes Wayland-Ereignis nach stderr.

#### Omarchy: das Symbol anpinnen, sonst fährt der Bereich ein

Omarchys Leiste hält den Tray-Bereich **nur offen, solange der Zeiger darauf
steht** (`Tray.qml`: `onHoveredChanged: root.expanded = hovered`, ohne
Ausnahme). Wer auf ein Symbol klickt und die Maus ins Menü bewegt, sieht den
Bereich hinter sich einfahren — bei jedem Tray-Programm gleich, auch bei
Omarchys eigenem Menü.

**Von unserer Seite ist daran nichts zu machen**, und diesmal ist der Satz
belegt statt behauptet. Für die Karte der Leiste (Vorgabe seit v0.6.1) liegt
es ohnehin ganz bei ihr. Und für unser eigenes Fenster
(`AI_BRAIN_TRAY_MENUE=eigen`): Solange es offen ist, leitet Hyprland alle
Zeigerereignisse auf unsere Fläche um — die Leiste sieht also keinen Hover
mehr, ob wir sie überdecken oder nicht. Sie auszusparen würde ausserdem den
Klick auf die Leiste als Weg zum Schliessen kosten.

Die Einstellung dafür hat Omarchy selbst: **Rechtsklick auf den kleinen Pfeil
links neben den Tray-Symbolen** öffnet eine Liste aller Tray-Programme, jedes
mit *Pin* und *Hide*. Ein angepinntes Symbol steht dauerhaft ausserhalb des
einfahrenden Bereichs.

In der Datei sieht das danach so aus (`~/.config/omarchy/shell.json`, unter
`bar.layout.right`):

```json
{ "id": "omarchy.tray", "pinned": ["ai-brain-tray"], "hidden": [] }
```

`ai-brain-tray` ist die SNI-`Id` dieses Trays — dieselbe, die
`busctl --user get-property … org.kde.StatusNotifierItem Id` ausgibt.

#### Was am laufenden Compositor gemessen wurde

Hyprland 0.56.2, zwei Ausgaben, Skalierung 1.25 (28.10.2026):

| `keyboard_interactivity` | Zeigerereignisse gehen an | Esc |
|---|---|---|
| `none` überall | die Fläche **unter** dem Zeiger, flächenlokal | kommt nicht an |
| `exclusive` auf dem Kasten | **immer an den Kasten**, kastenlokal (auch außerhalb) | kommt an |
| `exclusive` auf den Auffängern | immer an den **ersten**, Koordinaten der ganzen Anordnung | kommt an |

**Hyprland leitet den Zeiger auf jede Layer-Fläche um, deren
`keyboard_interactivity` nicht `none` ist.** Daran sind drei Anläufe
gescheitert: Mit `exclusive` auf den Auffängern erschien das Menü auf dem
falschen Bildschirm. Deshalb: Auffänger `none`, Kasten `exclusive`. Die
Umleitung ist dann kein Verlust, sondern ein zweiter Weg zum Schließen — ein
Klick daneben kommt als Kastenereignis mit Koordinaten außerhalb an.

Zwei weitere Punkte aus derselben Messung:

- **Die Eingabefläche muss ausdrücklich gesetzt werden.** Mit `wp_viewport`
  ist der Puffer des Auffängers 1×1; der Compositor schneidet die
  Eingabefläche darauf zu, und es kam kein einziges Zeigerereignis an.
- **Die Mausstelle kommt nicht aus `wl_pointer.enter`.** Hyprland schickt es
  erst, wenn der Zeiger sich bewegt — und beim Klick auf ein Tray-Symbol steht
  er still. Gefragt wird deshalb Hyprlands Sockel (`cursorpos`, `j/monitors`);
  wo es den nicht gibt, wird auf das `enter` gewartet und zuletzt die obere
  rechte Ecke genommen.

#### Die Konvention je Plattform

Seit v0.6.1 ist sie auf allen drei Leisten **dieselbe**: links schalten,
rechts das Menü. Das war vorher nicht so, und es war jedes Mal begründet —
genau das ist der Punkt.

| | Linksklick | Rechtsklick | Warum |
|---|---|---|---|
| Linux | **Live-Leitung an/aus** | das Menü, von der Leiste gezeigt | Der einzige Klick, der bei uns ankommt, ist der linke; also schaltet er, und das Menü bleibt dort, wo es ohnehin aufgeht |
| macOS | **Live-Leitung an/aus** | das Menü | **bewusste Abweichung.** Ein `NSStatusItem` mit Menü zeigt es üblicherweise bei jedem Klick; für Menüleisten-Extras gibt es keine zweite Gestik. Ein Tray, der sich je Leiste anders bedienen lässt, sind aber drei Trays — und das Menü bleibt über den Rechtsklick vollständig erreichbar, samt Zugehen |
| Windows | **Live-Leitung an/aus** | das Menü | **jetzt die Norm.** Der Infobereich trennt Linksklick (Standardaktion) von Rechtsklick (Kontextmenü), und die Standardaktion dieses Trays ist das Schalten. Von v0.5.3 bis v0.6.0 wichen wir hier ab |

> Dass eine Begründung technisch richtig ist, heißt nicht, dass das Ergebnis
> stimmt — und dass eine Rückfrage beantwortet wurde, heißt nicht, dass die
> Antwort so gemeint war, wie wir sie gelesen haben. Beides hat diesen Klick je
> eine Fassung gekostet. Und „auf unserer Seite nicht umgehbar" galt für den
> Weg, den wir gegangen waren, nicht für die Aufgabe: Das Menü selbst zu
> zeichnen stand die ganze Zeit offen.
>
> Die sechste Fassung hat eine eigene Lehre, und sie zeigt in die andere
> Richtung: **Eine Bitte kann zwei Hälften haben, die sich ausschließen.**
> „Links schalten" und „rechts unser Menü" waren einzeln beide machbar und
> zusammen nicht, weil auf Omarchy nur ein Klick bei uns ankommt. Wer erst
> baut und dann merkt, liefert die halbe Bitte und nennt es fertig. Deshalb
> steht hier, was diese Fassung **nicht** kann, bevor jemand es sucht.

Das Symbol wird **selbst gezeichnet** und nennt keinen Theme-Symbolnamen. Mit
`network-*` aus dem Theme kam es auf einer dunklen Leiste als dunkel auf dunkel
heraus. Ein Theme liefert eine Form, aber keine Zusage über den Kontrast.
Gezeichnet wird deshalb mit beiden Kontrastkanten — weißer Saum nach innen,
dunkle Kante nach außen —, und damit braucht es kein Theme-Wissen:

| | |
|---|---|
| grau | nicht verbunden |
| blau | verbunden, Leitung aus |
| grün | Leitung läuft und überträgt |
| gelb | unterwegs |
| rot | gestört (aufgelegt, getrennt, Harness stumm) |

Gefüllt heißt Leitung an, hohl heißt aus.

Den Harness sucht der Tray **neben sich selbst**, dann in `~/.local/bin`, dann
im `PATH` (oder unter `AI_BRAIN_HARNESS`). Autostart startet ihn ohne die Shell
des Nutzers, und dort fehlt `~/.local/bin` im `PATH` regelmäßig — er hing dann
in der Leiste und fand sein eigenes Programm nicht.

#### Ein Programm, drei Leisten

| | Leiste | Kiste | Autostart |
|---|---|---|---|
| Linux | StatusNotifierItem (D-Bus) | `ksni` | `~/.config/autostart/*.desktop` |
| macOS | NSStatusItem | `tray-icon` + `tao` | `~/Library/LaunchAgents` |
| Windows | Shell_NotifyIcon | `tray-icon` + `tao` | Run-Key unter `HKCU` |

Bis zum 27.10.2026 gab es ihn nur für Linux, mit der Begründung: ksni spricht
D-Bus, das gibt es dort nicht, ein Tray wäre ein **eigenes Programm**. Richtig
war die Technik, falsch die Folgerung — ein eigenes Programm wäre eine zweite
Bedienung, die auseinanderläuft. Jetzt ist nur das Anbinden an die Leiste je
Plattform eigen; was angezeigt wird (`ansicht`), wie das Symbol aussieht
(`symbol`), was ein Klick tut (`kern`) und wie der Harness gefunden wird
(`harness`) ist geteilt und ohne Plattformbezug — also überall testbar.

Unter Windows **kein** Autostart-Ordner: Dort müsste eine `.lnk` liegen, und die
schreibt man nicht ohne COM; eine `.cmd` an derselben Stelle würde bei jeder
Anmeldung ein Konsolenfenster aufblitzen lassen. Der Run-Key unter `HKCU`
braucht keine Administratorrechte und zeigt kein Fenster.

#### Er kommt mit der Installation — aber nicht überall

`install.sh` (Linux, macOS) und `install.ps1` (Windows) bringen ihn mit, und
zwar nur dort, wo er etwas anzeigen kann. Die Frage „gibt es hier eine Leiste"
ist je Plattform eine andere:

- **Linux.** `DBUS_SESSION_BUS_ADDRESS` *und* ein
  `org.kde.StatusNotifierWatcher` auf dem Sitzungsbus. Ohne Bus findet der Tray
  niemanden; ohne Watcher startet er und bleibt **unsichtbar** — der
  unangenehmere Fall, weil nichts darauf hinweist.
- **macOS.** `launchctl managername` muss `Aqua` sagen, also eine
  Fenster-Sitzung. Über SSH steht dort `Background` oder `StandardIO`.
- **Windows.** `SESSIONNAME` darf nicht `Services` sein.

Die beiden letzten sind weniger genau als der Watcher: Sie sagen „hier ist eine
Sitzung", nicht „hier nimmt etwas Symbole an". Mehr ist dort nicht zu holen —
dafür ist der stille Fall, den der Watcher abfängt, dort auch nicht bekannt:
Menüleiste und Infobereich sind Teil des Systems, nicht ein Programm, das
fehlen kann. `ai-brain-tray --pruefen` beantwortet die Frage je Plattform mit
dem Rückgabewert.

- **Nie auf der codingmachine.** Dort läuft AI Brain selbst. Sie *hat* eine
  Desktop-Sitzung, die Prüfung darüber fängt sie also nicht ab; es braucht einen
  eigenen Riegel. Erkannt wird sie am Benutzernamen und am imkr-Namen, **nicht
  am Hostnamen**: der lautet `omarchy` und damit genauso wie auf jedem anderen
  Omarchy-Rechner.

#### Unsigniert — und das wird angesagt

Entscheidung vom 27.10.2026 (Bastian): **jetzt ausliefern**, unsigniert. Die
Signier- und Notarisierungsschritte stehen im Release-Workflow bereit und
laufen, sobald die Zertifikate als Secrets hinterlegt sind; fehlen sie, werden
sie übersprungen — ein Release, das ohne Zertifikat rot wird, hielte die
Auslieferung für alle auf.

Die Installationsausgabe sagt es, statt es den Nutzer finden zu lassen:

- **macOS.** Per `curl` geholt startet der Tray **ohne** Rückfrage: curl setzt
  kein `com.apple.quarantine`. Nur wenn die Datei das Merkmal doch trägt (im
  Browser geholt, aus einem Archiv entpackt), blockt Gatekeeper — dann
  `xattr -d com.apple.quarantine …` oder „Dennoch öffnen" in den
  Systemeinstellungen. Genau dieser Unterschied gehört dazu, sonst sucht man
  den Fehler an der falschen Stelle.
- **Windows.** Das Skript entfernt die Herunterladen-Markierung schon
  (`Unblock-File`), damit SmartScreen meist gar nicht erst fragt. Tut es es
  doch: „Weitere Informationen" → „Trotzdem ausführen".

Der Autostart wird **gefragt**, nicht gesetzt. Und gefragt wird über `/dev/tty`,
nicht über die Standardeingabe: Das Skript läuft über `curl | sh`, dort ist die
Standardeingabe die Leitung mit dem Skript darin — ein `read` würde das Skript
anfressen. Gibt es kein Terminal (Cron, Bereitstellung, Container), wird nicht
gefragt und nichts eingerichtet.

| Variable | Wirkung |
|---|---|
| `AI_BRAIN_TRAY=0` | nie anbieten |
| `AI_BRAIN_TRAY=1` | anbieten, auch wenn die Erkennung daneben liegt |

Auf der Instanz holt `php artisan companion:fetch` die Trays mit (Linux, beide
macOS-Architekturen, Windows); ausgeliefert werden sie unter
`/install/tray/<ziel>`, und `/install.json` nennt sie unter `tray` — getrennt
von `available`, weil das PowerShell-Skript dort den ersten Windows-Eintrag
nimmt und darin nichts liegen darf, was kein Harness ist.

## Der eigene Loop

Bis Oktober 2026 mietete der Harness fremde Schleifen: `claude` und `codex`. Das
hiess, er konnte nur, was die können — und wer ihn benutzen wollte, musste erst
die Befehlszeile eines Wettbewerbers installieren. Für OpenRouter, Ollama und
alles OpenAI-kompatible gab es überhaupt keinen Weg.

Jetzt gibt es einen eigenen. Er ist **kein Ersatz** für `claude`: Kompaktierung,
Unteragenten, Hooks und der Plan-Modus bleiben dort, und `--durchreichen` bleibt
der Weg dahin. Der eigene Loop ist der Weg zu allem anderen.

### Zwei Formen, nicht eine pro Anbieter

| Form | Weg | Deckt ab |
|---|---|---|
| `openai` | `/v1/chat/completions` | OpenRouter, Ollama, vLLM, Groq, LM Studio … |
| `anthropic` | `/v1/messages` | Anthropic mit API-Schlüssel |

```bash
ai-brain-harness schleife --eintragen ollama --form openai \
  --basis http://127.0.0.1:11434/v1 --modell llama3.1:8b

ai-brain-harness schleife --eintragen openrouter --form openai \
  --basis https://openrouter.ai/api/v1 --modell qwen/qwen3-235b-a22b \
  --schluessel-variable OPENROUTER_API_KEY
```

Der Schlüssel selbst steht **nicht** in der Ablage, nur der Name der
Umgebungsvariable, aus der er kommt.

### Erst messen, dann laufen lassen

```bash
ai-brain-harness schleife --messen            # alle Anbieter
ai-brain-harness schleife --liste             # was eingetragen ist
```

Die Messung ist kein Beiwerk, sondern der Kern. Drei Dinge lassen sich **nicht**
aus Dokumentation ableiten, und jedes hat hier eine Stunde gekostet:

**Werkzeugfähigkeit gehört zum Modell, nicht zum Anbieter.** Auf demselben
Ollama greift `llama3.1:8b` Werkzeuge und `qwen2.5:14b` zunächst nicht. Deshalb
ist die Angabe dreiwertig: `ja`, `nein`, **ungemessen** — und ein neuer Eintrag
ist ungemessen, nicht fähig.

**Eine Stichprobe von eins ist keine Messung.** Der erste Befund zu
`qwen2.5:14b` lautete „kann nicht". Bei drei Proben kamen zwei richtige Griffe
und einer als Text. Das Modell kann es, nur unzuverlässig — und das ist der
Unterschied zwischen „anderes Modell nehmen" und „Adapter härten". Darum nimmt
jede Messung mehrere Proben und nennt die Rate.

**Das Kontextfenster des Servers ist nicht das des Modells.** `llama3.1:8b`
kann 131072 Marken. Ollama wertet trotzdem nur 4096 aus, wenn
`OLLAMA_CONTEXT_LENGTH` nicht gesetzt ist — und *meldet das nicht*. Über den
OpenAI-kompatiblen Weg lässt sich das pro Anfrage auch nicht heben; geprüft sind
`options.num_ctx` und ein Feld auf oberster Ebene, beide ohne Wirkung.

Im Betrieb sah das wie Modelldummheit aus: mit 188 angebotenen Werkzeugen griff
das Modell etwas völlig Unverwandtes und reichte eine *Fehlermeldung* als
Argumente durch. Es hatte den Katalog nur zur Hälfte gesehen. Deshalb misst
`--messen` auch, wo der Server abschneidet, und der Loop hält davor an:

```
Fehler: Der Werkzeugkatalog passt nicht in das gemessene Kontextfenster von
'ollama': rund 42018 Marken bei 4096 Platz (183 Werkzeuge aus AI Brain, 5 von
diesem Rechner).

Der Server wuerde den Rest stillschweigend abschneiden — das Modell saehe einen
halben Katalog und griffe ins Leere.
```

### Laufen

```bash
ai-brain-harness schleife "Wie lange laeuft dieser Rechner?"
ai-brain-harness schleife --ohne-ai-brain "…"     # nur Rechnerwerkzeuge
ai-brain-harness schleife --rechte steuernd "…"   # darf auch anfassen
ai-brain-harness schleife --anbieter openrouter "…"
ai-brain-harness schleife --ohne-werkzeuge "…"   # reines Gespraech
ai-brain-harness schleife --still "…"            # nur die Antwort, keine Zwischenmeldungen
```

Welcher Anbieter ohne `--anbieter` genommen wird, legt `--standard` fest:

```bash
ai-brain-harness schleife --standard openrouter
```

Die Werkzeuge kommen aus beiden Quellen: AI Brain über `tools/list`, dieser
Rechner im selben Prozess. Die Rechte bleiben, wo sie waren — derselbe Riegel
wie bei der Maske entscheidet, ob ein Lauf etwas ändern darf.

Zwei Härtungen arbeiten unsichtbar mit:

- **Erfundene Argumente werden verworfen.** `llama3.1:8b` schickte
  `{"Laufzeit":"true"}` für ein Werkzeug mit *leerem* Schema. Durchgereicht
  trifft so etwas serverseitige Prüfungen, und die Meldung sieht aus, als hätte
  der Harness Unsinn gebaut. Verworfenes wird genannt, nicht geschluckt.
- **Griffe im Textfeld werden eingesammelt.** Ollama klammert Werkzeuggriffe in
  Marken und liest sie zurück; stolpert das Modell beim öffnenden Teil, landet
  der fertige Griff unzerlegt im Text — erkennbar an einem durchgesickerten
  `</tool_call>`. Das Modell hat seine Arbeit getan, die Form ist auf dem Weg
  kaputtgegangen. Eingesammelt wird nur, was auf ein **angebotenes** Werkzeug
  zeigt; sonst löste ein Modell, das über JSON spricht, echte Griffe aus.

### Sitzungen

```bash
ai-brain-harness schleife --sitzungen
ai-brain-harness schleife --fortsetzen 1792079880 "Und die Platte?"
```

Gespeichert wird **auch bei einem Fehler**: ein Lauf, der am dritten Griff
scheitert, hat zwei brauchbare Ergebnisse gesammelt. Was hier *nicht* passiert,
ist Kürzen — wächst eine Sitzung über das Fenster, hält der Riegel oben an und
sagt es. Stilles Kürzen ist genau der Fehler, der bei Ollamas Deckel so teuer
war.

## Verbrauch melden

Claude Code und Codex schreiben in ihre Protokolldateien, wie viele Token und
wie viel Zeit eine Sitzung gekostet hat. Der Harness liest das, bildet je
Sitzung eine Summe und meldet den **Endstand** an AI Brain — dort landet er
neben dem Verbrauch der Channels, in derselben Auswertung.

**Nur Zahlen.** Kein Gesprächsinhalt verlässt den Rechner: gelesen werden
ausschliesslich `usage`-Objekte, Sitzungskennungen, Arbeitsverzeichnisse und
Modellnamen.

```bash
ai-brain-harness verbrauch zeigen      # was gemeldet würde
ai-brain-harness verbrauch melden      # nachholend alles Offene
ai-brain-harness verbrauch hook ein    # am Sitzungsende automatisch
ai-brain-harness verbrauch aus         # auf diesem Rechner abschalten
```

Zusätze: `--zeigen` (nur anzeigen statt melden), `--grenze <n>` (höchstens n
Sitzungen je Lauf, Vorgabe 50), `--sitzung <id>` (nur diese eine), `--vom-hook`
(Kennung aus der Hook-Eingabe auf stdin lesen), `--konto <name>` (nur nötig,
wenn der Harness es nicht selbst aus `CLAUDE_CONFIG_DIR` sieht), `--still`.

Erfasst wird **standardmässig**, abschaltbar je Rechner (hier) und je Person
(in AI Brain). Beim ersten Lauf sagt der Harness einmal sichtbar, dass gemessen
wird und wie man es abstellt.

### Warum der Server die Differenz bildet, nicht der Harness

Eine Meldung trägt den Endstand einer Sitzung, nicht den Zuwachs eines Laufs —
der Harness liest ja eine Datei und kennt nur Summen. Würde der Server
addieren, zählte jede Nachmeldung die ganze Sitzung erneut, und nachgemeldet
wird dauernd. Also differenziert **der Server** gegen den letzten Stand
derselben Sitzung. Zweimal dasselbe melden ändert damit keine Summe.

Dass der Harness die Differenz nicht selbst bildet, ist Absicht: zwei Zähler
laufen irgendwann auseinander, und nur der Server sieht alle Rechner.

### Drei Dinge, die gemessen wurden und den Entwurf geändert haben

**Die Konten teilen sich ein Verzeichnis.** Unter
`~/.local/state/imkr/accounts/claude/<konto>/` sind fast alle Einträge Symlinks
auf `~/.claude` — `projects/` eingeschlossen. Ein Lauf „über alle
Konfigurationen" läse jede Datei mehrfach; deshalb werden die Verzeichnisse
über ihren aufgelösten Pfad entdoppelt. Aus demselben Grund steht in einem
Transcript **kein Konto**: es ist aus der Datei nicht ableitbar.

Nur der `SessionEnd`-Hook läuft innerhalb des Gesprächs und kennt
`CLAUDE_CONFIG_DIR` — er ist damit die einzige Quelle für das Konto.
Nachgeholte Sitzungen bleiben ohne, sichtbar leer statt geraten.

**Codex nennt seinen Verbrauch anders.** `turn.completed` gibt es nicht (0 von
20 Dateien); der Satz heisst `token_usage_record`. Und dort enthält
`input_tokens` die gecachten **mit** — anders als bei Claude. Ohne das
Herausrechnen zählte der Cache doppelt.

**Die Transcripts nennen keine Kosten**, weder bei Claude noch bei Codex. Über
eine vollständige Sitzung mit 4280 `usage`-Objekten geprüft: kein `costUSD`,
kein `total_cost_usd`. In `stats-cache.json` gibt es ein `costUSD`, das steht
auf einem Abo aber für jedes Modell auf `0` — und eine 0 läse sich als „hat
nichts gekostet" statt als „unbekannt".

Dafür kommt die **Zeit**: Claude schreibt die Zugdauer in eigene Sätze
(`subtype=turn_duration`), in einer Sitzung 104 Stück mit zusammen 10,1
Stunden. Auf einem Abo ist das ohnehin die aussagekräftigere Zahl.

### Der Hook steht neben den anderen

`verbrauch hook ein` hängt sich an `SessionEnd` **zusätzlich** ein; vorhandene
Einträge bleiben stehen (auf dieser Maschine etwa imkrs Zustandsmeldung). Vor
dem Schreiben wird die Datei gesichert. Entfernt wird nur der eigene Eintrag,
erkannt an seiner Marke — nicht an seiner Position.

Eingerichtet wird er **ausdrücklich**, nicht still über `sync`: ein Hook läuft
bei jedem Sitzungsende mit, und so etwas gehört nicht nebenbei installiert.

## Den Browser zeigen lassen

Ein Agent auf einem anderen Rechner startet seinen Browser **dort** — also
ohne Bildschirm. Zum Prüfen und Testen genügt das. Soll der Mensch aber zusehen,
sich einloggen oder ein Captcha lösen, muss es der Browser des Rechners sein,
**an dem er gerade sitzt**.

```bash
ai-brain-harness tunnel start           # findet den Rechner des Menschen selbst
ai-brain-harness tunnel start --von desktop
ai-brain-harness tunnel status
ai-brain-harness tunnel stop
```

Zusätze: `--port <n>` (Vorgabe 9222, je Port ein eigener Browser),
`--nach <host>` (unter welchem Namen der Rechner des Menschen diesen hier
erreicht — Vorgabe ist der eigene Hostname) und `--hier` (nicht über den
Rechner des Menschen gehen, sondern lokal ausführen; das setzt der Fernaufruf
selbst).

Danach zeigt `127.0.0.1:9222` hier auf jenen Browser:
`agent-browser --cdp 9222`, oder die `-tunnel`-MCPs.

Gewachsen aus `~/.local/bin/agent-tunnel` (chezmoi) von der Laptop-Sitzung. Im
Harness, damit `install.sh` es überall mitbringt und es **ohne chezmoi und ohne
imkr** funktioniert — imkr wird benutzt, wenn es da ist.

### Auch als Werkzeug, nicht nur als Befehl

Am `ai-brain`-MCP-Server stehen `tunnel-start`, `tunnel-status` und
`tunnel-stop`. Damit genügt „öffne den Tunnel" — jeder Agent kann es rufen,
ohne die Befehlszeile zu kennen.

**Warum im Proxy und nicht bei den Rechnerwerkzeugen.** `rechner-mcp` bekommt
nur die Maske; eine gewöhnliche Claude-Code-Sitzung sieht ausschliesslich den
`ai-brain`-Server. Und der Riegel von `rechner.rs` passt hier nicht: er trennt
lesen von ändern, aber den eigenen Browser auf dem eigenen Rechner zu öffnen ist
keine Rechteerhöhung — es ist genau das, was der Mensch gerade verlangt.

Zwei Feinheiten, die dabei zählen:

- Die drei Werkzeuge werden **vor** dem Token-Abruf bedient. Der Tunnel hat mit
  AI Brain nichts zu tun; ihn von einer erreichbaren Instanz abhängig zu machen
  wäre eine Kopplung ohne Gegenwert.
- Scheitert das Anreichern der Werkzeugliste — unerwartete Antwortform, kaputtes
  JSON —, geht die Antwort der Instanz **unverändert** durch. Eine Werkzeugliste
  halb zu ersetzen wäre schlimmer als eine ohne unsere drei.

Im MCP-Modus ist stdout der JSON-RPC-Kanal. Deshalb geben die Tunnel-Funktionen
ihren Bericht **zurück**, statt ihn zu drucken; nur die Befehlszeile druckt, und
die einzige Zwischenmeldung geht nach stderr.

### `stop` erwischt auch Tunnel ohne Merker

Ein Tunnel, den das alte Shell-Skript gestartet hat, führt keinen Merker — `stop`
meldete dort „lief nicht", während der Port weiter belegt war und für alle
anderen blockiert blieb. Jetzt sucht `stop` zusätzlich die Läufe zum Port und
beendet sie.

Gesucht wird eng: nur Prozesse, die diesen Port in der Befehlszeile tragen, und
nur die beiden Formen, die ein Tunnel hat (`remote-debugging-port=<n>` und
`<n>:127.0.0.1:<n>`). Ein weiteres Muster fände den eigenen Aufruf mit.

### Wie der Rechner des Menschen gefunden wird

In dieser Reihenfolge, und nur so:

1. `--von` — was der Mensch sagt, gilt.
2. `imkr operator` — weiss es, wenn er in imkr arbeitet. **Optional:** fehlt
   imkr, geht es weiter statt zu scheitern.
3. `SSH_CONNECTION` — der Rechner, über den er gerade hier ist; die IP wird
   über `~/.ssh/config` zum Namen aufgelöst, damit Nutzer, Schlüssel und Port
   stimmen.

Findet sich nichts, wird **nicht geraten**: ein falscher Rechner startet einen
Browser vor einem leeren Stuhl, und der Agent wartet auf etwas, das niemand
sieht. Dann fragt der Befehl nach `--von`.

### Zwei Riegel, die aus dem Betrieb kommen

**Nie das Hauptprofil.** Der Fernsteuerungs-Port gibt volle Kontrolle über den
Browser, der ihn öffnet — jede Seite, jedes Cookie, jede angemeldete Sitzung.
Deshalb ein eigenes Profil unter `~/.cache/agent-chrome`, und je Port ein
eigenes: zwei Tunnel sind zwei Browser.

**Ein fremder Browser auf dem Port ist kein Geschenk.** Antwortet dort etwas,
das nicht von diesem Befehl stammt, bricht `start` ab. Sonst reichte der Tunnel
den Browser einer anderen Sitzung durch, und der Agent dahinter steuerte fremde
Tabs. Auf einem Rechner mit parallelen Sitzungen ist das der Normalfall, nicht
die Ausnahme — am 16.10.2026 lag auf Port 9333 genau so einer.

### Unterschied zum Skript

`agent-tunnel` hält am Bildschirm Browser und Tunnel im Vordergrund, Ctrl+C
beendet beides. Hier läuft immer **abgekoppelt**, und `tunnel stop` räumt auf:
dieser Befehl wird von einem Agenten gerufen, der keine Taste drücken kann und
auf eine Antwort wartet. Ein Vordergrundprozess wäre dort ein hängender Aufruf.

### Wenn die Gegenseite alt ist

Läuft auf dem Rechner des Menschen ein Harness ohne `tunnel`, sagt der Befehl
das — und zwar als Zustand **jener** Maschine:

```
Fehler: Auf laptop laeuft ein Harness, der 'tunnel' noch nicht kennt.
Dort aktualisieren: curl -fsSL <instanz>/install.sh | sh
```

Das ist mit Absicht so deutlich. Vorher wurde die Ausgabe des Fernaufrufs
durchgereicht, und dessen „Unbekannter Befehl: tunnel" erschien **hier** — als
hätte der Befehl auf diesem Rechner nicht existiert. Eine halbe Stunde
Fehlersuche am falschen Ende, bis eine Marke im Code bewies, dass er läuft.

## Für imkr: Nachrichten und Meldungen

Zwei Befehle, die nicht für Menschen gedacht sind, sondern für imkr auf demselben
Rechner. imkr hält keine Anmeldung bei AI Brain — die hält dieser Harness. Beide
Antworten mit `--json` sind **Maschinenverträge**: imkr zerlegt sie, also werden sie
unverändert durchgereicht und nicht hier neu zusammengesetzt.

### `nachricht` — zwischen den imkrs verschiedener Personen (imkr #495)

```bash
ai-brain-harness nachricht kontakte [--json]          # wen dieser Rechner erreicht
ai-brain-harness nachricht senden <person> <text> [--von A] [--ziel S/P]
ai-brain-harness nachricht abholen [--json]           # was für diesen Rechner anliegt
ai-brain-harness nachricht bestaetigen <nummer>       # eingereiht, kommt nicht wieder
```

`abholen --json` antwortet
`{"nachrichten":[{"id","von_person","von","ziel","text","angelegt","abholungen"}]}`.

### `meldung` — imkr-Ereignisse im gemeinsamen Posteingang (imkr #536)

Wartet ein Agent auf eine Antwort, ist er fertig, abgebrochen, ist sein Konto
erschöpft oder hat ein Kontakt geschrieben, legt imkr das in den Posteingang von
AI Brain — **zusätzlich** zu Klingel und ntfy, nicht statt. Eine Klingel vergeht,
eine Zeile im Posteingang bleibt stehen, bis jemand sie abhakt.

```bash
ai-brain-harness meldung senden --art <blocked|done|failed|account|message> \
    --rechner <name> --sitzung <name> --pane <nummer> \
    [--fenster <nummer>] [--agent <name>] [--link <url>] <titel…>
    # Der Begleittext (woran der Agent arbeitet, seine Frage, der Fehler) kommt
    # auf stdin; leer ist erlaubt, über 8 KB wird gekürzt.

ai-brain-harness meldung gelesen [--json]      # was im Posteingang abgehakt wurde
ai-brain-harness meldung bestaetigen <nummer>  # imkr hat das ✦ genommen
```

`gelesen --json` antwortet
`{"gelesen":[{"id","sitzung","pane","art","rechner","abholungen"}]}`.

**Der Titel steht am Schluss.** Alles, was keine Flagge und kein Flaggenwert ist,
gehört zum Titel — eine Flagge dahinter würde von ihm verschluckt.

**Warum es einen Rückweg gibt.** imkr führt sein eigenes ✦ („da war was, du hast es
noch nicht gesehen"). Wer die Zeile im Posteingang abhakt, hat sie gesehen; ein ✦,
das danach im Terminal stehen bleibt, wäre eine zweite Wahrheit. imkr fragt deshalb
alle 30 Sekunden `gelesen`, nimmt das ✦ und bestätigt erst dann.

**Mindestens einmal, nicht genau einmal.** Eine Zeile kommt wieder, solange sie nicht
bestätigt ist. Dasselbe ✦ zweimal wegzunehmen kostet nichts, es einmal stehen zu
lassen schon. Dieselbe Mechanik wie bei `nachricht`.

Auf der Instanz hängen daran die Werkzeuge `report-imkr-event-tool`,
`fetch-imkr-read-tool` und `confirm-imkr-read-tool`, die Tabelle `imkr_meldungen` und
die Posteingangs-Art `imkr`. Abgehaktes und Bestätigtes räumt AI Brain nach 30 Tagen
weg, Ungelesenes bleibt.

## Weitere Befehle

```bash
ai-brain-harness status            # Hängt dieser Rechner? — echt geprüft
ai-brain-harness status --json     # Dasselbe für Anzeigen (imkr-Sidebar, Leisten)
ai-brain-harness configure         # Trägt den MCP-Server nachträglich ein (Claude + Codex)
ai-brain-harness disconnect        # Entfernt die lokale Verbindung
ai-brain-harness mcp               # Läuft als MCP-Server (startet Claude Code selbst)
```

### `status` — eine Antwort statt vier Befehle

```
Instanz       : AI Brain (https://brain.example.com)
Gerät         : laptop (linux x86_64)
Zugerechnet an: Bastian
MCP-Endpunkt  : https://brain.example.com/mcp/brain
Erreichbar    : ja
Leitung       : an automatisch — verbunden
Tunnel        : aus (Port 9222)
Taktgeber     : active
Läufe         : 1 laufen, 0 warten
```

**`Erreichbar` ist echt gemessen**, nicht abgelesen: ein kurzer angemeldeter
Aufruf (`GET /api/v1/geraete/ich`) mit **drei Sekunden** Frist. Bis zum
29.10.2026 zeigte `status` nur die hinterlegte Verbindung — ein Rechner, dessen
Zugang längst entzogen war, las sich dort wie ein verbundener.

`--json` liefert dieselbe Messung für Programme:

```json
{
  "verbunden": true,
  "grund": "ok",
  "meldung": null,
  "instanz": "AI Brain",
  "basis": "https://brain.example.com",
  "geraet": "laptop (linux x86_64)",
  "zugerechnet": "Bastian",
  "leitung":   { "an": true, "grund": "automatisch", "lage": { "stand": "verbunden", … } },
  "tunnel":    { "port": 9222, "browser": false, "ssh": false, "grund": "aus" },
  "taktgeber": { "eingerichtet": true, "aktiv": "active" },
  "laeufe":    { "laeuft": 1, "wartet": 0 }
}
```

`grund` ist eines von **drei Wörtern** — `ok`, `abgemeldet`, `nicht erreichbar`.
Darauf darf eine Anzeige vergleichen; der Satz daneben in `meldung` ist für
Menschen und wird umformuliert.

| `grund` | Was es heißt | Was zu tun ist |
|---|---|---|
| `ok` | Die Instanz hat auf einen angemeldeten Aufruf geantwortet | nichts |
| `abgemeldet` | Kein Zugang hinterlegt, oder die Instanz weist ihn ab (401/403) | `ai-brain-harness connect <domain>` |
| `nicht erreichbar` | Kein Netz, Zeitüberschreitung, Proxy, Neustart | warten |

**Das kostet eine Anfrage, und nur eine.** Das Token kommt aus der Ablage —
geprägt wird höchstens einmal je Stunde, und nur, wenn ohnehin keins gilt.
Alles andere (Leitung, Tunnel, Taktgeber, Läufe) liest Dateien dieses Rechners.
Gemessen am 29.10.2026 auf der Codingmachine: **90 ms** je Aufruf.

> **Warum `leitung status --json` die Erreichbarkeit weiter NICHT prüft:** Den
> liest der Tray im **Zwei**-Sekunden-Takt, und eine HTTP-Anfrage in diesem
> Takt wäre Dauerlast auf der Instanz. `status` wird alle 30 Sekunden gelesen
> (imkr-Plugin), und damit ist die echte Messung bezahlbar. Wer den Takt von
> `status` verkürzt, muss diese Rechnung neu machen.

`mcp` rufst du nicht von Hand auf — das macht Claude Code. Der Befehl hält die
Gerätezugangsdaten, besorgt sich damit Zugangstoken, erneuert sie rechtzeitig
und reicht die Aufrufe an die Instanz weiter.

Genau dafür gibt es die CLI überhaupt als dauerhaft laufendes Programm:
Ein Zugangstoken lebt eine Stunde, Claude Codes Konfiguration kann aber nur
einen festen Wert halten. Ohne etwas, das erneuert, wäre die Verbindung nach
einer Stunde tot.

## Wo was liegt

| Datei | Inhalt |
|---|---|
| `~/.config/ai-brain-cli/connection.json` | Gerätezugangsdaten, Rechte `0600` |
| `~/.claude.json` | Eintrag `mcpServers.ai-brain` |
| `~/.claude.json.vor-companion` | Sicherung, angelegt vor dem ersten Eintrag |
| `~/.local/state/ai-brain-cli/laeufe/` | Zustand der Agentenläufe |
| `~/.config/systemd/user/ai-brain-daueraufgaben.{service,timer}` | der Abholer |

Der Name `vor-companion` ist historisch: Das Programm hiess bis Oktober 2026
`ai-brain-companion`. Die Datei behaelt ihren Namen, damit auf den verbundenen
Rechnern keine zweite Sicherung neben der vorhandenen entsteht.

### Umstieg von `ai-brain-companion`

Nichts zu tun. Beim ersten Start holt `ai-brain-harness` die Ablage von
`~/.config/ai-brain-companion/` herueber — die Verbindung, den Abgleichstand
und die gesicherten Fassungen — und sagt, dass es das getan hat. Neu koppeln
ist **nicht** noetig und waere schaedlich: `connect` legte einen zweiten
Geraetezugang an, derselbe Rechner stuende zweimal unter „Meine Geraete", und
die Zeiterfassung verteilte sich auf zwei Namen.

Was schon unter dem neuen Namen liegt, bleibt unangetastet. Konnte etwas nicht
uebernommen werden, bleibt der alte Ordner stehen und das Programm nennt den
Grund — eine halb umgezogene Ablage waere schlimmer als keine.

`disconnect` entfernt nur die lokale Ablage. Der Gerätezugang besteht auf der
Instanz weiter — endgültig entziehst du ihn unter „Meine Geräte".

## Voraussetzungen auf der Instanz

Die Instanz muss den Kopplungs-Client kennen. Einmalig dort ausführen:

```bash
php artisan mcp:companion-client
```

Fehlt er, sagt die CLI genau das — statt in einen OAuth-Fehler zu laufen.

## Wenn etwas klemmt

| Meldung | Bedeutung |
|---|---|
| `command not found` | `~/.local/bin` fehlt im `PATH`. Neue Shell öffnen oder `. ~/.zshrc` |
| `… kennt noch keinen Companion-Zugang` | Auf der Instanz fehlt `php artisan mcp:companion-client` |
| `Der Code ist abgelaufen` | Zu lange gewartet. `connect` einfach neu starten |
| `Die Kopplung wurde im Browser abgelehnt` | Im Bestätigungsdialog auf „Ablehnen" geklickt |
| `Der Gerätezugang wurde abgelehnt` | Gerät wurde unter „Meine Geräte" entzogen. Neu koppeln |

Läuft die Instanz mit Octane, werden Änderungen an ihrem PHP-Code erst nach
einem Neustart des Dienstes wirksam — das betrifft dich nur, wenn du selbst an
AI Brain entwickelst.

## Sicherheit

Der Kopplungs-Client ist öffentlich und hat **kein** Secret. Das ist Absicht:
Ein Programm, das auf fremden Rechnern läuft, kann kein Geheimnis hüten. Die
Berechtigung entsteht nicht durch den Besitz des Programms, sondern durch deine
Bestätigung im Browser.

Der Token, mit dem die CLI seinen Gerätezugang abholt, wird dabei
**verbraucht**. Ein zweiter Versuch mit demselben Token läuft ins Leere.

Bekannte Grenze: Die Gerätezugangsdaten liegen als Datei mit `0600` statt im
Schlüsselbund des Betriebssystems. Das steht auf der Liste.

## Lizenz

MIT
