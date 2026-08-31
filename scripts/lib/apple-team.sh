#!/usr/bin/env bash
#
# apple-team.sh — resolve the Apple *team ID* used to sign iOS builds.
#
# Sourced by ios-doctor.sh and ios-install.sh; not runnable on its own.
#
# The team ID is the Organizational Unit of the signing certificate — *not* the
# identifier printed in parentheses in its common name, which is a per-Apple-ID
# value and will not sign anything. A free Apple ID is issued a "personal team"
# that has no Membership page to look the ID up on, so the only reliable source
# is the certificate sitting in the keychain.
#
# Everything here has to run on a stock macOS bash 3.2: no `mapfile`, no
# associative arrays.

# Prints one team ID per line, deduplicated. Silent (and returns 0) when the
# machine has no usable signing certificate.
detect_apple_teams() {
  local valid pem line fingerprint

  # Only certificates the keychain still considers usable. Without this filter
  # an expired certificate from a previous team turns a perfectly good machine
  # into "several teams found" and forces the user to disambiguate by hand.
  valid="$(security find-identity -v -p codesigning 2>/dev/null | awk '$2 ~ /^[0-9A-F]{40}$/ { print $2 }')"
  [[ -n "$valid" ]] || return 0

  {
    security find-certificate -a -c "Apple Development" -p 2>/dev/null
    security find-certificate -a -c "iPhone Developer" -p 2>/dev/null
  } | {
    # Split the concatenated PEM stream by hand: `openssl x509` reads only the
    # first certificate, and splitting on a NUL separator does not survive the
    # awk that macOS ships.
    pem=""
    while IFS= read -r line; do
      pem="$pem$line"$'\n'
      [[ "$line" == "-----END CERTIFICATE-----" ]] || continue
      fingerprint="$(printf '%s' "$pem" | openssl x509 -noout -fingerprint -sha1 2>/dev/null | tr -d ':' | sed 's/.*=//')"
      if [[ -n "$fingerprint" ]] && grep -qF "$fingerprint" <<<"$valid"; then
        printf '%s' "$pem" | openssl x509 -noout -subject 2>/dev/null
      fi
      pem=""
    done
  } | sed -n 's/.*OU *= *\([A-Za-z0-9]*\).*/\1/p' | sort -u
}
