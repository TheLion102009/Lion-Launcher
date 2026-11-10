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
| Freunde (direkt, Ende-zu-Ende-verschlüsselt) | Freunde, Chat, geteilte Instanzen | Dein Minecraft-Name und deine UUID, Chat-Nachrichten, Inhalte geteilter Instanzen – nur an Freunde, die du selbst hinzugefügt hast |

## Automatische Verbindungen (abschaltbar)

Diese Funktionen laufen ohne direkte Aktion. Du kannst sie unter **Einstellungen → Datenschutz** abschalten:

1. **Update-Prüfung** – Beim Start und alle 6 Stunden fragt der Launcher bei GitHub
   (`api.github.com`, Repository `TheLion102009/Lion-Launcher`) nach einer neuen Version.
   Dabei werden nur deine IP-Adresse und die Programmversion (User-Agent) übertragen.
2. **Freunde-System (P2P)** – Damit Freunde dich ohne zentralen Server finden, veröffentlicht der
   Launcher deine Freundes-ID (einen öffentlichen Schlüssel) samt Erreichbarkeitsdaten über die
   öffentlichen Dienste von number 0 / iroh (Relay-Server `*.relay.n0.iroh.link` und DNS unter
   `iroh.link`, Betreiber: number 0, https://n0.computer). Die Relays leiten nur verschlüsselte
   Daten weiter und können keine Inhalte lesen. Sie sehen aber, welche Freundes-IDs miteinander
   verbunden sind, und deine IP-Adresse.

## Gespeicherte Daten

Alle Daten (Profile, Konten, Freundesliste, Chat-Verlauf, geteilte Instanzen) liegen nur lokal im
Launcher-Ordner auf deinem Rechner. Durch die Deinstallation und das Löschen dieses Ordners werden
sie vollständig entfernt.

## Kontakt

Thorben Hess – https://github.com/TheLion102009/Lion-Launcher/issues
