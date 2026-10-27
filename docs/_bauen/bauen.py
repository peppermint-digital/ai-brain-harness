# -*- coding: utf-8 -*-
import sys, pathlib, re, html as H
sys.path.insert(0, '/tmp/doku')
import stil, inhalt1, inhalt2, inhalt3

VORSPANN = {
 'index': 'Was dieser Harness ist, woraus er besteht, wo das Wissen liegt und wer ihn benutzen kann.',
 'was-ist-ein-harness': 'Das Wort wird benutzt, als waere klar, was es heisst. Hier sind die sieben Teile — und welche davon wir selbst bauen.',
 'architektur': 'Drei Schichten statt einem Programm. Warum AI Brain nirgends anruft, und was wo liegt.',
 'gedaechtnis': 'Hermes merkte sich Dinge auf der Platte. Wir merken sie uns in AI Brain — mit der Person daran, die sie geschrieben hat.',
 'mehrbenutzer': 'Ja, aber die Frage ist zu grob. Drei Bedeutungen, zwei davon koennen wir.',
 'befehle': 'Achtzehn Befehle, nach Zweck sortiert, mit den Schaltern, die man wirklich braucht.',
}
RUMPF = {
 'index': inhalt1.INDEX, 'was-ist-ein-harness': inhalt1.HARNESS,
 'architektur': inhalt2.ARCHITEKTUR, 'gedaechtnis': inhalt2.GEDAECHTNIS,
 'mehrbenutzer': inhalt3.MEHRBENUTZER, 'befehle': inhalt3.BEFEHLE,
}

def nach_markdown(titel, vorspann, rumpf):
    t = rumpf
    t = re.sub(r'<figure>.*?<figcaption>(.*?)</figcaption></figure>',
               lambda m: '\n> **Schaubild:** ' + re.sub(r'\s+', ' ', m.group(1)).strip()
                         + '\n> (in der HTML-Fassung dieser Seite)\n', t, flags=re.S)
    t = re.sub(r'<div class="(merk|warn)">\s*<p>(.*?)</p>\s*</div>',
               lambda m: '\n> ' + re.sub(r'\s+', ' ', m.group(2)).strip() + '\n', t, flags=re.S)
    t = re.sub(r'<pre><code>(.*?)</code></pre>',
               lambda m: '\n```bash\n' + re.sub(r'<[^>]+>', '', m.group(1)).strip() + '\n```\n', t, flags=re.S)
    # Tabellen
    def tabelle(m):
        zeilen = re.findall(r'<tr>(.*?)</tr>', m.group(1), flags=re.S)
        aus, kopf = [], True

        # Auszeichnungen IN der Zelle zuerst uebersetzen, dann den Rest
        # strippen. Vorher fiel beides weg, weil der Strip unten alle Tags
        # nimmt — ein Befehl in einer Tabelle stand in der Markdown-Fassung
        # also als nackter Text da. Gemerkt am 27.10.2026: In befehle.md
        # waren die Backticks von Hand nachgetragen, und der dokumentierte
        # Bau-Befehl haette sie wieder entfernt.
        def zelle(c):
            c = re.sub(r'<code>(.*?)</code>', r'`\1`', c, flags=re.S)
            c = re.sub(r'<strong>(.*?)</strong>', r'**\1**', c, flags=re.S)
            c = re.sub(r'<em>(.*?)</em>', r'*\1*', c, flags=re.S)
            # Ein Pipe im Text wuerde die Tabelle sprengen.
            c = re.sub(r'<[^>]+>', '', c).replace('|', '\\|')
            return re.sub(r'\s+', ' ', c).strip()

        for z in zeilen:
            zellen = [zelle(c) for c in re.findall(r'<t[hd]>(.*?)</t[hd]>', z, flags=re.S)]
            aus.append('| ' + ' | '.join(zellen) + ' |')
            if kopf:
                aus.append('|' + '---|' * len(zellen)); kopf = False
        return '\n' + '\n'.join(aus) + '\n'
    t = re.sub(r'<table>(.*?)</table>', tabelle, t, flags=re.S)
    t = re.sub(r'<h2>(.*?)</h2>', r'\n## \1\n', t)
    t = re.sub(r'<h3>(.*?)</h3>', r'\n### \1\n', t)
    t = re.sub(r'<li>(.*?)</li>', r'- \1', t, flags=re.S)
    t = re.sub(r'</?[uo]l>', '', t)
    t = re.sub(r'<p>(.*?)</p>', r'\1\n', t, flags=re.S)
    t = re.sub(r'<strong>(.*?)</strong>', r'**\1**', t, flags=re.S)
    t = re.sub(r'<em>(.*?)</em>', r'*\1*', t, flags=re.S)
    t = re.sub(r'<code>(.*?)</code>', r'`\1`', t, flags=re.S)
    t = re.sub(r'<a href="([a-z-]+)\.html">(.*?)</a>', r'[\2](\1.md)', t, flags=re.S)
    t = re.sub(r'<a href="([^"]+)">(.*?)</a>', r'[\2](\1)', t, flags=re.S)
    t = re.sub(r'<[^>]+>', '', t)
    t = H.unescape(t)
    t = re.sub(r'\n{3,}', '\n\n', t).strip()
    return '# {}\n\n*{}*\n\n{}\n'.format(titel, vorspann, t)

ziel_html = pathlib.Path(sys.argv[1]); ziel_md = pathlib.Path(sys.argv[2])
ziel_html.mkdir(parents=True, exist_ok=True); ziel_md.mkdir(parents=True, exist_ok=True)

for datei, titel in stil.SEITEN:
    (ziel_html / (datei + '.html')).write_text(
        stil.seite(datei, titel, VORSPANN[datei], RUMPF[datei]), encoding='utf-8')
    (ziel_md / (datei + '.md')).write_text(
        nach_markdown(titel, VORSPANN[datei], RUMPF[datei]), encoding='utf-8')

print('  HTML:', ', '.join(sorted(p.name for p in ziel_html.glob('*.html'))))
print('  MD:  ', ', '.join(sorted(p.name for p in ziel_md.glob('*.md'))))
