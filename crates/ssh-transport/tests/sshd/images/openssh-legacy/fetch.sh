#!/bin/sh
# Usage: fetch.sh <file> <sha256> <url> [<url>...]
# Downloads <file> from the first URL that delivers a file with the expected
# SHA-256; each URL is retried on transient errors. A wrong checksum is never
# accepted from any source. On failure prints "DOWNLOAD-FAILED" and every URL tried.
file="$1"; sum="$2"; shift 2
for url in "$@"; do
  echo "fetch: trying $url" >&2
  rm -f "$file"
  if curl -fsSL --retry 5 --retry-delay 3 --retry-all-errors --connect-timeout 20 -o "$file" "$url" \
     && echo "$sum  $file" | sha256sum -c - >&2; then
    exit 0
  fi
  echo "fetch: $url failed (download error or checksum mismatch)" >&2
done
echo "DOWNLOAD-FAILED: $file; tried: $*" >&2
exit 1
