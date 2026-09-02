#!/usr/bin/env bash
# Spec 0022, Abschnitt 4: erzeugt (einmalig, idempotent) ein selbstsigniertes
# Code-Signing-Zertifikat mit stabilem Common Name im Login-Schlüsselbund,
# damit `cargo tauri dev` über mehrere Neubauten hinweg dieselbe
# Code-Signatur trägt statt bei jedem Build eine neue Ad-hoc-Signatur zu
# bekommen (macOS bindet eine "Immer erlauben"-Keychain-Freigabe an die
# Signatur der zugreifenden App — eine bei jedem Build wechselnde
# Ad-hoc-Signatur invalidiert vorherige Freigaben). Details/Begründung:
# docs/adr/0022-stable-dev-code-signature.md.
#
# Nur für macOS relevant — auf anderen Plattformen ein No-op. Wird
# automatisch von scripts/tauri-dev.sh aufgerufen, kann aber auch separat
# ausgeführt werden.

set -euo pipefail

if [[ "$(uname)" != "Darwin" ]]; then
  echo "Nicht macOS — nichts zu tun." >&2
  exit 0
fi

# Muss exakt mit der Identität übereinstimmen, mit der
# scripts/tauri-dev-stable-signing-runner.sh die Binary signiert.
CERT_NAME="Smart SSH Dev Signing"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

if security find-certificate -c "$CERT_NAME" "$KEYCHAIN" >/dev/null 2>&1; then
  # Bereits vorhanden — bewusst nicht neu erzeugen: ein neues Zertifikat
  # hätte eine neue Identität und würde denselben "invalidiert bei jedem
  # Neubau"-Effekt reproduzieren, den dieses Skript gerade vermeiden soll.
  exit 0
fi

echo "Erzeuge selbstsigniertes Dev-Code-Signing-Zertifikat '$CERT_NAME' im Login-Schlüsselbund …" >&2

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

openssl req -x509 -newkey rsa:2048 -days 3650 \
  -keyout "$TMP_DIR/dev.key" -out "$TMP_DIR/dev.crt" -nodes \
  -subj "/CN=$CERT_NAME" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=codeSigning"

openssl pkcs12 -export -legacy \
  -in "$TMP_DIR/dev.crt" -inkey "$TMP_DIR/dev.key" \
  -out "$TMP_DIR/dev.p12" -password pass:dev-signing

# `-T /usr/bin/codesign`: erlaubt `codesign` den Zugriff auf den privaten
# Schlüssel ohne wiederholte Passwort-Abfrage — eine andere Kategorie von
# Prompt als das eigentliche Problem (App-Keychain-Zugriffsfreigaben), aber
# ebenfalls störend, wenn nicht vorab freigegeben.
security import "$TMP_DIR/dev.p12" -k "$KEYCHAIN" \
  -P dev-signing -T /usr/bin/codesign

# Entspricht dem manuellen "Trust > Code Signing > Always Trust" in
# Keychain Access, nur skriptbar. Ohne `-d`: Vertrauenseinstellung nur für
# den aktuellen Nutzer, kein Admin-/sudo-Zugriff nötig.
security add-trusted-cert -p codeSign -k "$KEYCHAIN" "$TMP_DIR/dev.crt"

echo "Fertig — '$CERT_NAME' ist als vertrauenswürdige Code-Signing-Identität im Login-Schlüsselbund hinterlegt." >&2
