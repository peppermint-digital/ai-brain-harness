# AI Brain Harness — Installation für Windows
#
#   $env:AI_BRAIN_INSTANZ = 'https://brain.example.com'
#   irm https://raw.githubusercontent.com/peppermint-digital/ai-brain-harness/main/install.ps1 | iex
#
# Dieses Skript installiert nichts selbst. Es holt das Installationsskript
# deiner AI-Brain-Instanz und führt es aus.
#
# Warum der Umweg: Ohne AI Brain ist der Harness nutzlos, und die Instanz
# kennt ihre eigene Adresse — sie kann sie deshalb in ihr Skript schreiben,
# und `connect` kommt danach ohne Argument aus. Ein Skript aus einem
# Repository weiß nicht, mit welcher Instanz du dich verbinden willst.
#
# Die frühere Fassung holte aus dem GitHub-Release. Damals gab es keins, also
# lief sie jedes Mal ins Leere — nach außen sah das aus wie ein kaputtes
# Programm, dabei war es nur eine leere Bezugsquelle. Seit v0.2.0 gibt es
# Releases; der Umweg über die Instanz bleibt trotzdem der richtige, denn nur
# sie kennt ihre Adresse.
#
# Einen Tray gibt es hier nicht: Er spricht StatusNotifierItem über D-Bus, das
# gibt es unter Windows nicht. Ein eigenes Programm dafür bräuchte eine
# Signatur — noch nicht entschieden.

$ErrorActionPreference = 'Stop'

# Windows PowerShell 5.1 verhandelt je nach Systemstand noch TLS 1.0.
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Instanz = $env:AI_BRAIN_INSTANZ

if (-not $Instanz) {
    Write-Host 'Adresse deiner AI-Brain-Instanz (z.B. https://brain.example.com):'
    $Instanz = Read-Host '  Adresse'
}

$Instanz = $Instanz.Trim().TrimEnd('/')

if (-not $Instanz) {
    Write-Host 'Ohne Adresse geht es nicht.'
    exit 1
}

# Fehlt das Schema, waere die zusammengesetzte URL ungueltig — ein Vertipper,
# der sonst als unverstaendlicher Verbindungsfehler herauskaeme.
if ($Instanz -notmatch '^https?://') {
    $Instanz = "https://$Instanz"
}

Write-Host "Hole das Installationsskript von $Instanz …"

try {
    $skript = Invoke-RestMethod -Uri "$Instanz/install.ps1"
} catch {
    Write-Host ''
    Write-Host "$Instanz liefert kein Installationsskript."
    Write-Host $_.Exception.Message
    Write-Host ''
    Write-Host 'Pruefe die Adresse. Laeuft dort eine aeltere AI-Brain-Fassung,'
    Write-Host 'gibt es die Auslieferung noch nicht — dann baust du aus dem'
    Write-Host 'Quelltext (Rust von https://rustup.rs vorausgesetzt):'
    Write-Host ''
    Write-Host '  cargo install --git https://github.com/peppermint-digital/ai-brain-harness'
    exit 1
}

Invoke-Expression $skript
