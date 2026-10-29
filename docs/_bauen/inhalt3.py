# -*- coding: utf-8 -*-
import svg

MEHRBENUTZER = ('''
<p>Die kurze Antwort: <strong>ja</strong>, aber die Frage ist zu grob gestellt. Mehrbenutzerfaehig
kann dreierlei heissen, und wir sind es zweimal.</p>

<figure>''' + svg.mehrbenutzer() + '''
<figcaption>Gemessen am 15.10.2026 an der echten Instanz: zwei Zugaenge auf diesem Rechner,
einer auf dem Laptop.</figcaption></figure>

<h2>Die drei Bedeutungen</h2>
<table>
<tr><th>Was gemeint sein kann</th><th>Koennen wir?</th><th>Wie</th></tr>
<tr><td>Mehrere <strong>Rechner</strong> an einer Instanz</td><td>ja</td>
    <td>Ein Geraetezugang je Rechner. Jeder sieht nur seine Auftraege.</td></tr>
<tr><td>Mehrere <strong>Konten</strong> auf einem Rechner</td><td>ja</td>
    <td>Die Ablage liegt im Heimverzeichnis. Jedes Konto koppelt sich selbst.</td></tr>
<tr><td>Zwei <strong>Menschen unter einem Konto</strong></td><td>nein</td>
    <td>Dann handeln beide als dieselbe Person. Das ist eine echte Grenze.</td></tr>
</table>

<h2>Woran die Identitaet haengt</h2>
<p>An genau einer Sache: dem <strong>Geraetezugang</strong>. Das ist ein eigener OAuth-Client,
entstanden beim Koppeln ueber den Device-Flow, mit einer Person als Besitzer. Unter
&bdquo;Meine Geraete&ldquo; ist er einzeln entziehbar.</p>
<p>Daraus folgt alles Weitere, ohne dass es irgendwo zweimal steht:</p>
<ul>
<li>welche Auftraege dieser Rechner abholen kann,</li>
<li>welche Nachrichten er bekommt (und welche er nie bekommt — seine eigenen),</li>
<li>welche Repository-Eintraege auf ihm landen,</li>
<li>wem ein geschriebener Gedanke, eine Zeitbuchung, ein Pruefspur-Eintrag gehoert.</li>
</ul>

<h2>Geteilt oder getrennt?</h2>
<table>
<tr><th>Getrennt je Geraet</th><th>Geteilt ueber alle</th></tr>
<tr><td>Zugangsdaten und Token</td><td>Projektwissen und Wiki</td></tr>
<tr><td>Repository-Auswahl</td><td>Aufgaben und Bugs</td></tr>
<tr><td>Laufregister und Protokolle</td><td>Zeiterfassung (je Person zugeordnet)</td></tr>
<tr><td>Verzeichnis-Freigaben</td><td>Pruefspur</td></tr>
</table>

<div class="warn"><p><strong>Eine Falle beim Koppeln.</strong> Der Geraetename entsteht als
<code>$USER@hostname</code>. Omarchy nennt per Vorgabe jeden Rechner <code>omarchy</code> — zwei
Rechner mit demselben Benutzernamen erzeugen also denselben Namen. Beim Koppeln einen eigenen
Namen mitgeben, sonst stehen zwei gleichnamige Zugaenge in der Liste und niemand weiss mehr,
welcher welcher ist.</p></div>

<h2>Was ein zweiter Mensch tun muss</h2>
<pre><code><span class="d"># in seinem eigenen Konto, auf demselben oder einem anderen Rechner</span>
curl -fsSL https://brain.proxy.peppermint-digital.com/install.sh | sh
ai-brain-cli connect
<span class="d"># bestaetigt im Browser mit SEINEM AI-Brain-Konto</span>
ai-brain-cli sync --apply</code></pre>
<p>Ab da schreibt er in dasselbe Projektwissen wie alle anderen, und jeder Eintrag traegt
seinen Namen. Er sieht die Aufgaben, die er sehen darf, und seine Zeit laeuft auf sein Konto.</p>

<h2>Wo es noch klemmt</h2>
<ul>
<li><strong>Ein Konto, zwei Menschen</strong> bleibt nicht trennbar. Wer das braucht, braucht
getrennte Systemkonten.</li>
<li><strong>Rechner-genaues Adressieren bei Nachrichten</strong> gibt es noch nicht — eine
Nachricht geht an eine Person, und der erste ihrer Rechner, der abholt, bekommt sie.</li>
<li><strong>Zwei Abholer je Rechner</strong> teilen sich einen Zugang und sind fuer AI Brain
nicht unterscheidbar. Eine Haltefrist von fuenf Minuten verhindert die doppelte Zustellung,
aber die Unterscheidung selbst kann die Instanz nicht treffen.</li>
</ul>
''')

BEFEHLE = ('''
<p>Achtzehn Befehle. Die Liste steht auch im Programm selbst — <code>ai-brain-cli</code> ohne
Argument zeigt sie.</p>

<h2>Einrichten</h2>
<table>
<tr><th>Befehl</th><th>Was</th></tr>
<tr><td><code>einrichten</code></td><td>Der ganze Rechner in einem Befehl: verbinden, Skills und
MCP-Server abgleichen, MCP-Eintrag in Claude Code, Codex, <code>pi</code> und die imkr-Konten,
Gedaechtnis abgleichen, Leitungsdienst und Tray anbieten &mdash; und eine Zusammenfassung, die
nachliest statt zu behaupten. Schon verbunden? Dann faellt Schritt 1 aus.</td></tr>
<tr><td><code>connect [domain]</code></td><td>Rechner mit einer Instanz verbinden. Ohne Angabe
die Instanz, von der das Programm kam.</td></tr>
<tr><td><code>status [--json]</code></td><td>Haengt dieser Rechner? Echt gemessen &mdash; ein
kurzer angemeldeter Aufruf gegen die Instanz, drei Sekunden Frist &mdash; samt Leitung, Tunnel,
Taktgeber und Laeufen. <code>grund</code> ist eines von drei Woertern: <code>ok</code>,
<code>abgemeldet</code>, <code>nicht erreichbar</code>. Die Statuszeile des imkr-Plugins liest
das alle 30 Sekunden.</td></tr>
<tr><td><code>configure</code></td><td>Traegt den MCP-Server in Claude Code und Codex ein.</td></tr>
<tr><td><code>disconnect</code></td><td>Entfernt die lokale Verbindung.</td></tr>
<tr><td><code>freigabe [add &lt;pfad&gt;]</code></td><td>Verzeichnisse, in denen Laeufe mehr duerfen
als fragen.</td></tr>
<tr><td><code>erinnerungen [--apply]</code></td><td>Das Gedaechtnis dieses Verzeichnisses mit AI
Brain abgleichen. Ohne <code>--apply</code> wird nur gezeigt, was sich bewegen wuerde. Angewendet
wird nichts ueberschrieben: Was auf beiden Seiten geaendert wurde, bleibt liegen, bis
<code>--hier-gewinnt</code> oder <code>--dort-gewinnt</code> entscheidet; geloescht wird nie.</td></tr>
</table>

<h2>Arbeiten</h2>
<table>
<tr><th>Befehl</th><th>Was</th></tr>
<tr><td><code>agent [...]</code></td><td>Einen Lauf hier starten.</td></tr>
<tr><td><code>agents</code></td><td>Welche Laeufe hier laufen.</td></tr>
<tr><td><code>logs &lt;kennung&gt;</code></td><td>Was ein Lauf geschrieben hat.</td></tr>
<tr><td><code>stop &lt;kennung&gt;</code></td><td>Einen Lauf beenden.</td></tr>
</table>
<p>Schalter fuer <code>agent</code>:</p>
<table>
<tr><th>Schalter</th><th>Wirkung</th></tr>
<tr><td><code>--aufgabe &quot;…&quot;</code></td><td>Auftrag in Worten.</td></tr>
<tr><td><code>--task &lt;nr&gt;</code></td><td>Aufgabe aus AI Brain holen.</td></tr>
<tr><td><code>--projekt &lt;slug&gt;</code></td><td>Projektbezug.</td></tr>
<tr><td><code>--modell claude|codex</code></td><td>Vorgabe ist <code>claude</code>.</td></tr>
<tr><td><code>--schreibend</code>, <code>--vollzugriff</code></td><td>Mehr Rechte — nur in
freigegebenen Verzeichnissen.</td></tr>
<tr><td><code>--verzeichnis &lt;pfad&gt;</code></td><td>Wo der Lauf arbeitet.</td></tr>
<tr><td><code>--imkr</code></td><td>In einem eigenen imkr-Pane starten.</td></tr>
<tr><td><code>--weiter</code>, <code>--sitzung &lt;id&gt;</code></td><td>Eine Sitzung fortsetzen.</td></tr>
<tr><td><code>--zeitgrenze &lt;s&gt;</code></td><td>Vorgabe 3600 Sekunden.</td></tr>
<tr><td><code>--trocken</code></td><td>Zeigt, was gestartet wuerde, ohne zu starten.</td></tr>
</table>

<h2>Aufgaben und Auftraege</h2>
<div class="merk"><p><strong>Zwei Woerter, die sich aehneln und Verschiedenes meinen.</strong>
Eine <strong>Aufgabe</strong> ist ein Eintrag aus der Projektliste, den sich jemand aussucht.
Ein <strong>Auftrag</strong> ist etwas, das AI Brain genau diesem Rechner hingelegt hat.</p></div>
<table>
<tr><th>Befehl</th><th>Was</th></tr>
<tr><td><code>aufgaben [--projekt &lt;slug&gt;] [--json]</code></td><td>Offene Aufgaben zum
Auswaehlen. Ohne Projekt projektuebergreifend.</td></tr>
<tr><td><code>auftraege</code></td><td>Was AI Brain fuer diesen Rechner bereitlegt.</td></tr>
<tr><td><code>auftrag &lt;nr&gt;</code></td><td>Annehmen und abarbeiten. Nimmt die Schalter von
<code>agent</code> mit.</td></tr>
<tr><td><code>auftrag-melden &lt;nr&gt; &lt;status&gt;</code></td><td>Ergebnis nachtragen, falls das
automatische Melden scheiterte.</td></tr>
</table>

<h2>Nachrichten</h2>
<table>
<tr><th>Befehl</th><th>Was</th></tr>
<tr><td><code>nachricht kontakte</code></td><td>Wen dieser Rechner erreichen kann.</td></tr>
<tr><td><code>nachricht senden &lt;person&gt; &lt;text&gt;</code></td><td>Mit <code>--von</code> und
<code>--ziel</code> fuer imkr-Adressen. Hoechstens 8 KB.</td></tr>
<tr><td><code>nachricht abholen</code></td><td>Was fuer diesen Rechner anliegt.</td></tr>
<tr><td><code>nachricht bestaetigen &lt;nr&gt;</code></td><td>Eingereiht — kommt nicht wieder.</td></tr>
</table>

<h2>Wissen abgleichen</h2>
<table>
<tr><th>Befehl</th><th>Was</th></tr>
<tr><td><code>sync [--apply]</code></td><td>Skills und Agents aus dem Repository holen, einzeln
gewaehlte MCP-Server in Claude Code und Codex eintragen. Ohne <code>--apply</code> wird nur
gezeigt.</td></tr>
<tr><td><code>publish &lt;name&gt;</code></td><td>Einen lokalen Eintrag zurueckgeben.</td></tr>
<tr><td><code>select</code></td><td>Waehlen, welche Eintraege dieser Rechner bekommt. MCP-Server
nur einzeln, nie ueber &bdquo;alles&ldquo;.</td></tr>
<tr><td><code>select --dazu a,b / --weg a,b</code></td><td>Ohne Terminal ergaenzen bzw. entfernen,
der Modus bleibt. Namen als <code>typ/slug</code> (<code>mcp/playwright</code>) oder eindeutiger
Slug; mehrdeutig oder unbekannt bricht ab, ohne zu speichern.</td></tr>
<tr><td><code>select --setzen a,b</code></td><td>Stellt auf Modus &bdquo;auswahl&ldquo; und ersetzt
die ganze Liste. Ein Rechner auf &bdquo;alles&ldquo; verliert damit alle uebrigen Eintraege &mdash;
zum Ergaenzen <code>--dazu</code>.</td></tr>
</table>
<p>Weitere Schalter fuer <code>sync</code>: <code>--only</code>, <code>--force</code>,
<code>--aufraeumen</code>, <code>--ja</code>.</p>

<h2>Live-Leitung</h2>
<table>
<tr><th>Befehl</th><th>Was</th></tr>
<tr><td><code>leitung an [--dauer 30m]</code></td><td>Von Hand einschalten, hoechstens 8 h. Hebt
ein vorheriges <code>aus</code> auf.</td></tr>
<tr><td><code>leitung aus</code></td><td>Auflegen &mdash; und zwar bis zum naechsten Einschalten,
nicht bis zum naechsten Herzschlag. Der <em>Dienst</em> legt dabei auf und laeuft weiter, damit ein
spaeteres <code>leitung an</code> ohne neue Anmeldung greift.</td></tr>
<tr><td><code>leitung status [--json]</code></td><td>Schalter, Grund, Prozess, Verbindung.</td></tr>
<tr><td><code>leitung dienst</code></td><td>Haelt die Leitung und schaltet sie selbst, nach der
Freigabe in AI Brain.</td></tr>
<tr><td><code>leitung autostart</code></td><td>Den Dienst beim Anmelden starten.
<code>kein-autostart</code> bestellt es ab.</td></tr>
<tr><td><code>leitung laufen</code></td><td>Die Leitung im Vordergrund halten, ohne Dienst.</td></tr>
</table>
<p>Die Leitung zeigt ein imkr-Pane dieses Rechners in AI Brain und nimmt Tastendruecke von dort
an. Der Rechner verbindet sich <strong>ausgehend</strong>; AI Brain ruft hier nie etwas auf.</p>
<p><strong>Drei Zustaende, und die Reihenfolge ist die Regel.</strong> Ein <em>Veto</em> (aus
<code>leitung aus</code> oder dem Haekchen im Tray-Menue) schlaegt alles und hat keine Frist &mdash;
sonst waere &bdquo;aus&ldquo; eine Bitte und keine Anweisung. Darunter gilt der Hand-Schalter mit
seiner Restzeit, darunter die automatische Leitung. Sie hat keine Frist, sondern eine Bedingung:
Mindestens ein Channel muss diesen Rechner in AI Brain freigegeben haben. Faellt die Freigabe,
legt sie binnen einer Minute selbst auf. Ohne Freigabe bleibt sie aus.</p>
<p><strong>Das verschiebt keine Sperre.</strong> Ob ein Agent etwas darf, prueft AI Brain bei jedem
Aufruf gegen die Freigabe je Channel und Geraet. Eine offene Leitung ohne Freigabe ist nutzlos,
nicht erlaubt.</p>
<p>Ueber dieselbe Leitung kommen die Auftraege der imkr-Werkzeuge eines Channels. Jeder ist eine
eigene Art, und der Rechner baut seine Befehlszeile selbst: <code>sitzungen</code> fuehrt
<code>imkr ls --json</code> aus (gefiltert, ohne Bildschirminhalte), <code>lesen</code> fuehrt
<code>imkr read</code> aus, und nur <code>befehl</code> fuehrt einen mitgeschickten Text aus &mdash;
und haengt an einer eigenen Freigabe.</p>

<h2>Als MCP-Server</h2>
<pre><code>ai-brain-cli mcp</code></pre>
<p>Laeuft als MCP-Server und gibt Claude Code und Codex alle AI-Brain-Werkzeuge. Wird von
<code>configure</code> eingetragen; man ruft es selten von Hand auf.</p>
''')
