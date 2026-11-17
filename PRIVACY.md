# Datenschutzerklärung / Privacy Policy – Lion Launcher

Der Lion Launcher hat keinen eigenen Server und sammelt keine Nutzungs- oder Telemetriedaten.
Er verbindet sich nur mit den unten genannten Diensten.

## Verbindungen, die du selbst auslöst

| Dienst | Wofür | Was übertragen wird |
|---|---|---|
| Microsoft / Xbox Live / Mojang | Anmeldung mit deinem Microsoft-Konto, Skins | Anmeldedaten im Microsoft-Login, Zugriffstoken |
| Modrinth (api.modrinth.com, cdn.modrinth.com) | Mods, Resource Packs, Shader suchen und herunterladen; exakte Versionen beim Teilen bestimmen | Suchbegriffe, Projekt-IDs, Prüfsummen (SHA-1) installierter Dateien |
| Mojang / Fabric / Quilt / Forge / NeoForge | Minecraft und Mod-Loader herunterladen | Angefragte Versionen |
| mc-heads.net | Spielerköpfe/Skins anzeigen | Minecraft-Name bzw. UUID |
| Horizon (GitHub: `api.github.com`, `github.com/horizon-rust`, `horizon-rust.github.io`) | Nur bei Horizon-Profilen: neuesten Horizon-Build und Horizon-Mods/Resource Packs/Shader laden | Angefragte Dateien; die Minecraft-Dateien (inkl. Server-JAR) kommen wie bei Vanilla direkt von Mojang |
| Native Horizon-Mods (nur nach deiner Zustimmung) | Die Werkzeuge des Horizon-Launchers bauen die Mods auf deinem PC: sie laden den festgelegten Rust-Compiler und Horizons Quellcode/SDK (GitHub, rust-lang.org) | Angefragte Dateien |
| Freunde (direkt, Ende-zu-Ende-verschlüsselt) | Freunde, Chat, geteilte Instanzen | Dein Minecraft-Name und deine UUID, Chat-Nachrichten, Inhalte geteilter Instanzen – nur an Freunde, die du selbst hinzugefügt hast |

## Automatische Verbindungen (abschaltbar)

Diese Funktionen laufen ohne direkte Aktion. Du kannst sie in den **Einstellungen** abschalten (Update-Prüfung unter „Updates & Info“,
Freunde-System unter „Freunde & Geräte“):

1. **Update-Prüfung** – Beim Start und alle 6 Stunden fragt der Launcher bei GitHub
   (`api.github.com`, Repository `TheLion102009/Lion-Launcher`) nach einer neuen Version.
   Dabei werden nur deine IP-Adresse und die Programmversion (User-Agent) übertragen.
2. **Freunde-System (P2P)** – Damit Freunde dich ohne zentralen Server finden, veröffentlicht der
   Launcher deine Freundes-ID (einen öffentlichen Schlüssel) samt Erreichbarkeitsdaten über die
   öffentlichen Dienste von number 0 / iroh (Relay-Server `*.relay.n0.iroh.link` und DNS unter
   `iroh.link`, Betreiber: number 0, https://n0.computer). Die Relays leiten nur verschlüsselte
   Daten weiter und können keine Inhalte lesen. Sie sehen aber, welche Freundes-IDs miteinander
   verbunden sind, und deine IP-Adresse.
3. **Abgleich zwischen eigenen Geräten** (Teil des Freunde-Systems, nur mit Microsoft-Account) –
   Damit sich deine PCs mit demselben Minecraft-Account finden, trägt der Launcher die Freundes-IDs
   deiner Geräte in einen signierten Eintrag beim pkarr-Dienst von number 0 (`dns.iroh.link`) ein.
   Der Schlüssel dieses Eintrags wird aus deiner Minecraft-UUID abgeleitet. Bevor zwei Geräte ihre
   Freundesliste abgleichen, beweisen beide über den Mojang-Sitzungsserver
   (`sessionserver.mojang.com`, dasselbe Verfahren wie beim Betreten eines Minecraft-Servers), mit
   demselben Account angemeldet zu sein. Dein Zugriffstoken geht dabei nur an Mojang, nie an das
   andere Gerät.

## Gespeicherte Daten

Alle Daten (Profile, Konten, Freundesliste, Chat-Verlauf, geteilte Instanzen) liegen nur lokal im
Launcher-Ordner auf deinem Rechner. Durch die Deinstallation und das Löschen dieses Ordners werden
sie vollständig entfernt.

## Kontakt

Thorben Hess – https://github.com/TheLion102009/Lion-Launcher/issues
