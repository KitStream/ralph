#!/usr/bin/env bash
# Submit a file to Apple's notary service and poll until it reaches a terminal
# state, tolerating transient network errors.
#
# `xcrun notarytool submit --wait` hangs indefinitely on transient HTTP errors
# (observed as NSURLErrorDomain -1009 "Internet connection appears to be
# offline" in CI). This script submits without --wait, then polls `notarytool
# info` in a loop that treats network failures as retryable.
#
# Required env vars:
#   APPLE_API_KEY_PATH   Path to the .p8 App Store Connect API key file
#   APPLE_API_KEY        Key ID
#   APPLE_API_ISSUER     Issuer ID
#
# Usage: notarize.sh <file-to-submit>
set -euo pipefail

: "${APPLE_API_KEY_PATH:?}"
: "${APPLE_API_KEY:?}"
: "${APPLE_API_ISSUER:?}"

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <file>" >&2
    exit 2
fi
FILE="$1"

notary() {
    xcrun notarytool "$@" \
        --key "$APPLE_API_KEY_PATH" \
        --key-id "$APPLE_API_KEY" \
        --issuer "$APPLE_API_ISSUER"
}

echo "Submitting $FILE to notary service..."
SUBMIT_JSON=$(notary submit "$FILE" --output-format json)
SUBMISSION_ID=$(echo "$SUBMIT_JSON" | /usr/bin/python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
echo "Submission ID: $SUBMISSION_ID"

# Poll up to ~2 hours, tolerating transient failures. Apple's notary
# service normally responds in 2-15 minutes, but has been observed to
# take over an hour during busy periods.
DEADLINE=$(( $(date +%s) + 7200 ))
CONSECUTIVE_ERRORS=0
MAX_CONSECUTIVE_ERRORS=10

while true; do
    if (( $(date +%s) > DEADLINE )); then
        echo "Timed out waiting for notarization" >&2
        notary log "$SUBMISSION_ID" || true
        exit 1
    fi

    sleep 30

    if INFO_JSON=$(notary info "$SUBMISSION_ID" --output-format json 2>&1); then
        CONSECUTIVE_ERRORS=0
        STATUS=$(echo "$INFO_JSON" | /usr/bin/python3 -c 'import json,sys; print(json.load(sys.stdin)["status"])' 2>/dev/null || echo "Unknown")
        echo "Status: $STATUS"
        case "$STATUS" in
            Accepted)
                echo "Notarization succeeded"
                exit 0
                ;;
            Invalid | Rejected)
                echo "Notarization failed: $STATUS" >&2
                notary log "$SUBMISSION_ID" || true
                exit 1
                ;;
            In\ Progress | Unknown)
                continue
                ;;
            *)
                echo "Unexpected status: $STATUS" >&2
                continue
                ;;
        esac
    else
        CONSECUTIVE_ERRORS=$(( CONSECUTIVE_ERRORS + 1 ))
        echo "Poll failed (attempt $CONSECUTIVE_ERRORS/$MAX_CONSECUTIVE_ERRORS): $INFO_JSON" >&2
        if (( CONSECUTIVE_ERRORS >= MAX_CONSECUTIVE_ERRORS )); then
            echo "Too many consecutive polling errors, giving up" >&2
            exit 1
        fi
    fi
done
