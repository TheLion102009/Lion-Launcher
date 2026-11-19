#!/usr/bin/env bash
# Installiert Pakete auf den Ubuntu-Runnern und übersteht kaputte Spiegel.
#
# Die Runner laden über einen Azure-Spiegel (mirror+file:/etc/apt/apt-mirrors.txt). Hängt der
# nach Ubuntu-Updates hinterher, nennt die Paketliste schon neue Versionen, deren .deb-Dateien
# es dort noch nicht gibt -> "404 Not Found". Dann: Paketquellen neu laden, erneut versuchen und
# ab dem zweiten Versuch direkt von archive.ubuntu.com laden.
#
# Aufruf: bash .github/scripts/apt-install.sh paket1 paket2 ...
set -u

MAX_ATTEMPTS=4
APT_OPTS=(-o Acquire::Retries=3 -o Acquire::http::Timeout=30)

for attempt in $(seq 1 "$MAX_ATTEMPTS"); do
  if sudo apt-get "${APT_OPTS[@]}" update &&
     sudo apt-get "${APT_OPTS[@]}" install -y "$@"; then
    exit 0
  fi

  if [ "$attempt" -eq "$MAX_ATTEMPTS" ]; then
    echo "::error::apt-Installation nach $MAX_ATTEMPTS Versuchen fehlgeschlagen"
    exit 1
  fi

  echo "::warning::apt fehlgeschlagen (Versuch $attempt/$MAX_ATTEMPTS) – Paketquellen werden neu geladen"
  if [ -f /etc/apt/apt-mirrors.txt ]; then
    # Azure-Spiegel umgehen: nur noch das offizielle Ubuntu-Archiv verwenden
    printf 'http://archive.ubuntu.com/ubuntu/\tpriority:1\n' | sudo tee /etc/apt/apt-mirrors.txt >/dev/null
  fi
  sudo apt-get clean
  sleep $((attempt * 15))
done
