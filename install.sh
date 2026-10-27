#!/bin/sh
# AI Brain Harness — Installation
#
#   curl -fsSL https://raw.githubusercontent.com/peppermint-digital/ai-brain-harness/main/install.sh | sh
#
# Legt das Programm in ~/.local/bin ab und sorgt dafür, dass dieses
# Verzeichnis in zsh und bash im PATH liegt. Vorhandene Einträge werden
# erkannt, mehrfaches Ausführen ändert nichts doppelt.
#
# Bewusst POSIX-sh: das Skript läuft, bevor irgendetwas über die Shell des
# Nutzers bekannt ist.
#
# Der empfohlene Weg ist der über die eigene Instanz
# (`curl -fsSL https://deine-brain-domain.de/install.sh | sh`) — dort steckt
# die Adresse schon im Befehl. Dieses Skript ist der Weg ohne Instanz.

set -eu

REPO="peppermint-digital/ai-brain-harness"
REPO_URL="https://github.com/${REPO}.git"
BIN_NAME="ai-brain-harness"
# Der Befehl hiess bis zum 15.10.2026 `ai-brain-cli`. Davon haengen
# MCP-Eintraege, `brain`, Skripte und das imkr-Plugin ab. Der alte Name bleibt
# deshalb als Verweis, bis alle Rechner umgestellt sind.
ALT_NAME="ai-brain-cli"
TRAY_NAME="ai-brain-tray"
INSTALL_DIR="${AI_BRAIN_COMPANION_BIN_DIR:-$HOME/.local/bin}"
MARKER="# ai-brain-harness: ~/.local/bin in den PATH"

say() { printf '%s\n' "$*"; }
warn() { printf '%s\n' "$*" >&2; }
die() { printf 'Fehler: %s\n' "$*" >&2; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || die "$2"
}

# Ein Trap, einmal gesetzt. Zwei Traps in zwei Funktionen ueberschreiben sich
# gegenseitig — und wer vor dem `trap` abbricht, laesst seine Dateien liegen.
# Die Variablen werden erst beim Aufraeumen gelesen, also darf sie setzen, wer
# will.
work=""
release_json=""

aufraeumen() {
    [ -n "$work" ] && rm -rf "$work"
    [ -n "$release_json" ] && rm -f "$release_json"
    return 0
}

trap aufraeumen EXIT INT TERM

# --------------------------------------------------------------- fertig ----

# Welches gebaute Programm passt zu diesem Rechner?
ziel_erkennen() {
    system="$(uname -s)"
    arch="$(uname -m)"

    case "$system/$arch" in
        Linux/x86_64)          echo "x86_64-unknown-linux-gnu" ;;
        Darwin/arm64|Darwin/aarch64) echo "aarch64-apple-darwin" ;;
        Darwin/x86_64)         echo "x86_64-apple-darwin" ;;
        *)                     echo "" ;;
    esac
}

# Die Adresse einer Datei im letzten Release — oder nichts.
#
# Das Release wird EINMAL abgefragt und zwischengelegt: Der Tray fragt danach
# noch einmal nach seiner eigenen Datei, und die GitHub-API zaehlt Abfragen
# ohne Anmeldung (60 pro Stunde).
release_laden() {
    [ -n "$release_json" ] && return 0
    command -v curl >/dev/null 2>&1 || return 1

    release_json="$(mktemp)"

    if ! curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
            -o "$release_json" 2>/dev/null; then
        rm -f "$release_json"
        release_json=""
        return 1
    fi

    return 0
}

release_adresse() {
    datei="$1"

    release_laden || return 1

    url="$(grep -o "https://[^\"]*/${datei}" "$release_json" | head -1)"

    [ -n "$url" ] || return 1
    printf '%s\n' "$url"
}

# Versucht, das gebaute Programm aus dem letzten Release zu holen.
#
# Gelingt das, spart es dem Nutzer eine Rust-Installation und eine halbe Minute
# Bauzeit. Gelingt es nicht — kein Release, unbekannte Plattform, kein Netz —
# faellt das Skript auf den Quelltext zurueck, statt abzubrechen.
download_release() {
    ziel="$(ziel_erkennen)"

    [ -n "$ziel" ] || return 1
    command -v curl >/dev/null 2>&1 || return 1

    say "Suche ein gebautes Programm fuer $ziel …"

    # Beide Namen, neuester zuerst: Releases von vor dem 15.10.2026 tragen
    # noch `ai-brain-cli-<ziel>`. Wer nur den neuen Namen sucht, laeuft an
    # einer Datei vorbei, die daliegt.
    url="$(release_adresse "${BIN_NAME}-${ziel}" || release_adresse "${ALT_NAME}-${ziel}")" || return 1

    [ -n "$url" ] || return 1

    work="$(mktemp -d)"

    curl -fsSL "$url" -o "$work/$BIN_NAME" || return 1

    mkdir -p "$INSTALL_DIR"
    install -m 755 "$work/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"

    say "Fertiges Programm uebernommen — kein Bau noetig."
    return 0
}

# ---------------------------------------------------------------- bauen ----

build_from_source() {
    need git "git wird gebraucht, ist aber nicht installiert."
    need cargo "Rust (cargo) wird gebraucht. Installieren mit:
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
Danach eine neue Shell öffnen und dieses Skript erneut ausführen."

    # Auch bei Abbruch aufräumen — sonst bleiben Klone im temporären
    # Verzeichnis liegen, jeder ein paar hundert Megabyte nach dem Bauen. Das
    # macht der Trap oben.
    work="$(mktemp -d)"

    say "Hole den Quelltext …"
    git clone --depth 1 --quiet "$REPO_URL" "$work/src"

    say "Baue (das dauert beim ersten Mal etwa eine halbe Minute) …"
    ( cd "$work/src" && cargo build --release --quiet )

    mkdir -p "$INSTALL_DIR"
    install -m 755 "$work/src/target/release/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"

    # Der Quelltext liegt schon da — den Tray von hier zu bauen kostet nur noch
    # die Bauzeit. Gebaut wird er aber nur, wenn er auch angeboten wird.
    quelltext="$work/src"
}

# ------------------------------------------------------------------ Tray ----

# `ai-brain-tray` zeigt die Live-Leitung in der Leiste und schaltet sie. Er
# braucht eine Desktop-Sitzung; der Harness nicht. Deshalb wird er nicht
# mitinstalliert, sondern angeboten — und nur dort, wo er etwas anzeigen kann.
#
#   AI_BRAIN_TRAY=0   nie anbieten
#   AI_BRAIN_TRAY=1   anbieten, auch wenn die Erkennung unten daneben liegt

# Dieser eine Rechner nie. Auf ihm laeuft AI Brain selbst, und er HAT eine
# Desktop-Sitzung — die Pruefung darunter faengt ihn also nicht ab. Dort sitzt
# aber niemand, der ein Menue anklickt.
#
# Erkannt am Benutzernamen und am imkr-Namen, NICHT am Hostnamen: der lautet
# `omarchy` und damit genauso wie auf jedem anderen Omarchy-Rechner — eine
# Pruefung darauf wuerde fremde Laptops mitsperren.
ist_codingmachine() {
    if [ "$(id -un 2>/dev/null)" = "codingmachine" ]; then
        return 0
    fi

    conf="${XDG_CONFIG_HOME:-$HOME/.config}/imkr/config.toml"

    if [ -r "$conf" ] && grep -q '^name[[:space:]]*=[[:space:]]*"codingmachine"' "$conf" 2>/dev/null; then
        return 0
    fi

    return 1
}

# Gibt es hier eine Leiste, die ein Symbol annimmt?
#
# Die Frage ist je Plattform eine andere, und sie muss je Plattform neu
# beantwortet werden:
#
#   Linux   ein org.kde.StatusNotifierWatcher auf dem Sitzungsbus
#   macOS   eine Fenster-Sitzung (launchctl managername sagt Aqua)
#   sonst   nein
hat_tray_sitzung() {
    case "$(uname -s)" in
        Linux)  hat_tray_linux ;;
        Darwin) hat_tray_macos ;;
        *)      return 1 ;;
    esac
}

# macOS hat immer eine Menueleiste — wenn man in einer Fenster-Sitzung sitzt.
# Ueber SSH oder aus einem Dienst heraus sagt `launchctl managername`
# `Background` bzw. `StandardIO`, und dort gaebe es nichts, woran ein
# NSStatusItem haengen koennte.
#
# Das ist weniger genau als der Watcher unter Linux: Es sagt „hier ist eine
# Sitzung", nicht „hier nimmt etwas Symbole an". Mehr ist unter macOS nicht zu
# holen — dafuer ist der stille Fall, den der Watcher abfaengt (Tray startet,
# bleibt aber unsichtbar), dort nicht bekannt: Die Menueleiste ist Teil des
# Systems und kein Programm, das fehlen kann.
hat_tray_macos() {
    command -v launchctl >/dev/null 2>&1 || return 1
    [ "$(launchctl managername 2>/dev/null)" = "Aqua" ]
}

# Eine Desktop-Sitzung MIT einer Leiste, die StatusNotifierItem bedient.
#
# Beides muss stimmen. Ohne Sitzungsbus findet der Tray niemanden; ohne Watcher
# startet er und bleibt unsichtbar — der unangenehmere Fall, weil nichts
# darauf hinweist.
hat_tray_linux() {
    [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ] || return 1

    if command -v busctl >/dev/null 2>&1; then
        if busctl --user list 2>/dev/null | grep -q 'org.kde.StatusNotifierWatcher'; then
            return 0
        fi
        return 1
    fi

    if command -v gdbus >/dev/null 2>&1; then
        if gdbus call --session -d org.freedesktop.DBus -o /org/freedesktop/DBus \
                -m org.freedesktop.DBus.ListNames 2>/dev/null \
                | grep -q 'org.kde.StatusNotifierWatcher'; then
            return 0
        fi
        return 1
    fi

    if command -v dbus-send >/dev/null 2>&1; then
        if dbus-send --session --print-reply --dest=org.freedesktop.DBus \
                /org/freedesktop/DBus org.freedesktop.DBus.ListNames 2>/dev/null \
                | grep -q 'org.kde.StatusNotifierWatcher'; then
            return 0
        fi
        return 1
    fi

    # Kein Werkzeug, um zu fragen. Dann wird nicht geraten — ein Tray, der
    # unsichtbar bleibt, ist schlimmer als keiner.
    return 1
}

# Ist der Tray hier ueberhaupt am Platz?
tray_erwuenscht() {
    if [ "${AI_BRAIN_TRAY:-}" = "0" ]; then
        return 1
    fi

    if [ "${AI_BRAIN_TRAY:-}" = "1" ]; then
        return 0
    fi

    # Bis zum 27.10.2026 stand hier ein Riegel gegen alles, was nicht Linux
    # ist, mit der Begruendung: ksni spricht StatusNotifierItem ueber D-Bus,
    # und das gibt es unter macOS und Windows nicht — ein Tray dort waere ein
    # eigenes Programm mit eigener Signaturfrage.
    #
    # Die Technik stimmte, die Folgerung nicht: Der Tray haelt seit v0.3.0
    # beide Leisten in EINEM Programm (ksni unter Linux, tray-icon unter macOS
    # und Windows). Die Signaturfrage ist entschieden — unsigniert ausliefern
    # und es ehrlich ansagen, siehe `tray_signatur_hinweis`.
    #
    # Windows faellt hier trotzdem heraus: Dieses Skript ist POSIX-sh, dort
    # laeuft `install.ps1`.
    case "$(uname -s)" in
        Linux|Darwin) : ;;
        *)            return 1 ;;
    esac

    if ist_codingmachine; then
        say "Kein Tray auf der codingmachine — dort sitzt niemand vor dem Bildschirm."
        return 1
    fi

    if ! hat_tray_sitzung; then
        say "Keine Desktop-Sitzung mit StatusNotifier gefunden — kein Tray."
        say "Nachtraeglich:  AI_BRAIN_TRAY=1 sh install.sh"
        return 1
    fi

    return 0
}

tray_holen() {
    ziel="$(ziel_erkennen)"

    if [ -n "$ziel" ] && command -v curl >/dev/null 2>&1; then
        url="$(release_adresse "${TRAY_NAME}-${ziel}" || true)"

        if [ -n "${url:-}" ]; then
            tray_work="$(mktemp -d)"

            if curl -fsSL "$url" -o "$tray_work/$TRAY_NAME"; then
                mkdir -p "$INSTALL_DIR"
                install -m 755 "$tray_work/$TRAY_NAME" "$INSTALL_DIR/$TRAY_NAME"
                rm -rf "$tray_work"
                return 0
            fi

            rm -rf "$tray_work"
        fi
    fi

    # Kein Release, aber der Quelltext liegt vom Bauen noch da.
    if [ -n "${quelltext:-}" ] && [ -d "$quelltext" ]; then
        say "Baue den Tray aus dem Quelltext …"
        ( cd "$quelltext" && cargo build --release --quiet -p "$TRAY_NAME" )
        install -m 755 "$quelltext/target/release/$TRAY_NAME" "$INSTALL_DIR/$TRAY_NAME"
        return 0
    fi

    return 1
}

# Was der Nutzer ueber die fehlende Signatur wissen muss — bevor es ihn
# ueberrascht.
#
# Der Tray ist nicht signiert (Entscheidung vom 27.10.2026: jetzt ausliefern,
# Signierung im Workflow vorbereitet). Das ist kein Grund zur Sorge, aber ein
# Grund zur Ansage: Eine Warnung, die unerwartet kommt, sieht wie ein
# kaputtes Programm aus. Eine, die angekuendigt wurde, ist eine Fussnote.
#
# Unter macOS ist der haeufige Fall ausdruecklich NICHT betroffen: `curl`
# setzt kein `com.apple.quarantine`, also startet das Programm ohne
# Rueckfrage. Gatekeeper meldet sich erst, wenn die Datei das Merkmal doch
# traegt — etwa weil sie im Browser geholt oder aus einem Archiv entpackt
# wurde. Genau dieser Unterschied gehoert dazugesagt, sonst sucht man den
# Fehler an der falschen Stelle.
tray_signatur_hinweis() {
    [ "$(uname -s)" = "Darwin" ] || return 0

    say ""
    say "Hinweis: Der Tray ist nicht signiert (noch kein Apple-Zertifikat)."
    say "So, wie er hier ankommt — per curl —, startet er trotzdem ohne"
    say "Rueckfrage: curl setzt kein Quarantaene-Merkmal."
    say "Sollte Gatekeeper sich doch melden (\"kann nicht geoeffnet werden,"
    say "weil der Entwickler nicht verifiziert werden kann\"), hilft eins von:"
    say "  xattr -d com.apple.quarantine $INSTALL_DIR/$TRAY_NAME"
    say "  oder: Systemeinstellungen → Datenschutz & Sicherheit → \"Dennoch oeffnen\""
}

tray_anbieten() {
    tray_erwuenscht || return 0

    say ""
    say "Diese Sitzung kann einen Tray anzeigen — hole $TRAY_NAME …"

    if ! tray_holen; then
        say "Kein Tray verfuegbar (kein Release, kein Quelltext)."
        say "Von Hand:  cargo install --git https://github.com/${REPO} $TRAY_NAME"
        return 0
    fi

    say "Tray installiert: $INSTALL_DIR/$TRAY_NAME"

    tray_signatur_hinweis

    # „Nur auf Nachfrage" heisst: ein Mensch sagt ja.
    #
    # Dieses Skript laeuft ueber `curl | sh` — die Standardeingabe ist dann die
    # Leitung MIT DEM SKRIPT DARIN, nicht die Tastatur. Ein `read` wuerde das
    # Skript anfressen. Gefragt wird deshalb ueber /dev/tty.
    #
    # Gibt es kein Terminal (Cron, Bereitstellung, Container), wird NICHT
    # gefragt und NICHT eingerichtet: Autostart ist etwas, das jemand wollen
    # muss.
    if ! (exec 3< /dev/tty) 2>/dev/null; then
        say ""
        say "Kein Terminal zum Fragen — Autostart nicht eingerichtet."
        say "Spaeter von Hand:  $TRAY_NAME --autostart"
        return 0
    fi

    printf 'Tray beim Anmelden automatisch starten? [j/N] ' > /dev/tty
    antwort=""
    read -r antwort < /dev/tty || antwort=""

    case "$antwort" in
        j|J|ja|Ja|JA|y|Y|yes|Yes)
            # Wo der Autostart landet, sagt das Programm selbst — unter
            # Linux ~/.config/autostart, unter macOS ein LaunchAgent. Diese
            # Zeile wuerde sonst auf einem Mac etwas Falsches behaupten.
            if "$INSTALL_DIR/$TRAY_NAME" --autostart; then
                :
            else
                say "Autostart liess sich nicht einrichten."
            fi
            ;;
        *)
            say "Kein Autostart. Spaeter:  $TRAY_NAME --autostart"
            ;;
    esac

    say "Jetzt starten:  $TRAY_NAME &"
}

# ------------------------------------------------------------------ PATH ----

# Trägt die PATH-Zeile in eine Shell-Konfiguration ein — aber nur, wenn sie
# nicht schon drinsteht. Der Marker macht das Wiedererkennen unabhängig davon,
# wie die Zeile formatiert wurde.
ensure_path_in() {
    rc="$1"

    [ -e "$rc" ] || return 0

    if grep -Fq "$MARKER" "$rc" 2>/dev/null; then
        say "  $rc — Eintrag besteht bereits"
        return 0
    fi

    # Auch schon vorhandene, von Hand gesetzte Einträge respektieren, statt
    # eine zweite Zeile danebenzulegen.
    if grep -q '\.local/bin' "$rc" 2>/dev/null; then
        say "  $rc — enthält bereits einen Eintrag für ~/.local/bin, unverändert gelassen"
        return 0
    fi

    {
        printf '\n%s\n' "$MARKER"
        printf '%s\n' 'case ":$PATH:" in'
        printf '%s\n' '  *":$HOME/.local/bin:"*) ;;'
        printf '%s\n' '  *) PATH="$HOME/.local/bin:$PATH" ;;'
        printf '%s\n' 'esac'
    } >> "$rc"

    say "  $rc — Eintrag ergänzt"
    changed_rc=1
}

setup_path() {
    changed_rc=0

    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*)
            say "PATH enthält ${INSTALL_DIR} bereits."
            return 0
            ;;
    esac

    say "Trage ${INSTALL_DIR} in die Shell-Konfiguration ein:"

    ensure_path_in "$HOME/.zshrc"
    ensure_path_in "$HOME/.bashrc"

    # Manche Anmeldungen lesen nur .bash_profile bzw. .profile. Nur anfassen,
    # wenn es die Datei schon gibt — sonst legen wir Nutzern Dateien an, die
    # ihre Anmeldung plötzlich anders ablaufen lassen.
    ensure_path_in "$HOME/.bash_profile"
    ensure_path_in "$HOME/.profile"

    if [ "$changed_rc" -eq 0 ]; then
        warn ""
        warn "Keine Shell-Konfiguration gefunden, die ich ergänzen konnte."
        warn "Füge diese Zeile selbst hinzu:"
        warn "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
}

# ------------------------------------------------------------------ Lauf ----

say "AI Brain Harness — Installation"
say ""

quelltext=""

if ! download_release; then
    say "Kein passendes Release gefunden — baue aus dem Quelltext."
    build_from_source
fi

# Der Uebergangsname. Ein Symlink genuegt: Das Programm entscheidet an seinem
# ersten Argument, nicht an argv[0].
ln -sf "$INSTALL_DIR/$BIN_NAME" "$INSTALL_DIR/$ALT_NAME"

tray_anbieten

say ""
setup_path

say ""
say "Installiert: $INSTALL_DIR/$BIN_NAME"
say ""

# Dieses Skript ist auch der Update-Weg. Wer schon verbunden ist, braucht KEIN
# `connect` — das legte einen zweiten Geraetezugang an: derselbe Rechner zweimal
# unter „Meine Geraete", der neue mit der Grundausstattung statt der gepflegten
# Auswahl, und die Zeiterfassung auf zwei Namen verteilt. Der Austausch des
# Programms laesst die Verbindung unberuehrt.
#
# BEIDE Ablagen pruefen: Ein Rechner mit der Fassung vor der Umbenennung hat die
# Verbindung noch unter dem alten Namen — das Programm holt sie beim ersten Lauf
# herueber, dieses Skript laeuft davor.
conf_basis="${XDG_CONFIG_HOME:-$HOME/.config}"

if [ -f "$conf_basis/ai-brain-cli/connection.json" ] || [ -f "$conf_basis/ai-brain-companion/connection.json" ]; then
    say "Dieser Rechner ist bereits verbunden — kein 'connect' noetig."
    say ""
    say "Öffne eine neue Shell (oder:  . ~/.zshrc  bzw.  . ~/.bashrc), dann:"
    say ""
    say "  $BIN_NAME status"
    say "  $BIN_NAME sync --apply"
    say ""
else
    say "Öffne eine neue Shell (oder:  . ~/.zshrc  bzw.  . ~/.bashrc), dann:"
    say ""
    say "  $BIN_NAME connect deine-brain-domain.de"
    say ""
fi
