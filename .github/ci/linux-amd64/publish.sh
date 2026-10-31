#!/usr/bin/env bash
# Lemon Studio - publish a packaged Linux amd64 build to the nginx download endpoint.
#
# Run by .github/workflows/deploy-linux-amd64.yml on the self-hosted macOS
# runner after the native Ubuntu x86_64 build job has uploaded its artifact:
#
#     .github/ci/linux-amd64/publish.sh <artifact-dir>
#
# The artifact directory contains the versioned tarball and BUILD-INFO (see the
# workflow). This script:
#   1. verifies the artifact against BUILD-INFO,
#   2. installs it as the versioned and stable tarballs under dist/linux/amd64,
#   3. rewrites SHA256SUMS, latest.json and dist/index.html,
#   4. checks the public endpoint with HTTP Basic auth.
#
# Publishing is a file copy: nothing touches nginx, so no reload is needed.
# Set LEMON_DIST_DIR to publish elsewhere (dry runs), and
# LEMON_SKIP_HTTP_CHECK=1 to skip the public endpoint check.
set -euo pipefail
# English month names in the landing page, whatever the machine's locale is.
export LC_TIME=C

ARTIFACT_DIR="${1:-}"
if [[ -z "$ARTIFACT_DIR" || ! -d "$ARTIFACT_DIR" ]]; then
  echo "usage: $0 <artifact-dir>" >&2
  exit 2
fi

DIST_DIR="${LEMON_DIST_DIR:-/Users/mac/Projects/Lemon-Studio/dist}"
LINUX_DIR="$DIST_DIR/linux/amd64"
PUBLIC_HOST="${LEMON_PUBLIC_HOST:-lemon.tristanmasse.space}"
PUBLIC_DIR="https://$PUBLIC_HOST/download/linux/amd64"
PUBLIC_URL="$PUBLIC_DIR/lemonstudio-linux-amd64.tar.gz"
CREDENTIALS="${LEMON_CREDENTIALS:-$HOME/.config/lemon-studio/download-credentials.txt}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
TEMPLATE="$SCRIPT_DIR/index.html"
BUILD_INFO="$ARTIFACT_DIR/BUILD-INFO"

fail() { echo "error: $*" >&2; exit 1; }

[[ -s "$BUILD_INFO" ]] || fail "$BUILD_INFO is missing; the build job did not upload BUILD-INFO"
[[ -s "$TEMPLATE" ]] || fail "$TEMPLATE is missing"

read_value() {
  local value
  value="$(sed -n "s/^$1=//p" "$BUILD_INFO" | head -n 1)"
  [[ -n "$value" ]] || fail "$BUILD_INFO has no $1"
  printf '%s' "$value"
}

VERSION="$(read_value version)"
COMMIT="$(read_value commit)"
BUILT_AT="$(read_value built_at)"
TARBALL="$(read_value tarball)"
SHA256="$(read_value sha256)"
SIZE="$(read_value size)"
RUN_ID="$(read_value run_id)"
RUN_NUMBER="$(read_value run_number)"

[[ "$VERSION" =~ ^[0-9A-Za-z.+-]+$ ]] || fail "unexpected version: $VERSION"
[[ "$COMMIT" =~ ^[0-9a-f]+$ ]] || fail "unexpected commit: $COMMIT"
[[ "$TARBALL" =~ ^lemonstudio-[0-9A-Za-z.+-]+-linux-amd64\.tar\.gz$ ]] || fail "unexpected tarball name: $TARBALL"
[[ "$SHA256" =~ ^[0-9a-f]{64}$ ]] || fail "unexpected sha256: $SHA256"
[[ "$SIZE" =~ ^[0-9]+$ ]] || fail "unexpected size: $SIZE"
[[ "$RUN_ID" =~ ^[0-9]+$ ]] || fail "unexpected run id: $RUN_ID"
[[ "$RUN_NUMBER" =~ ^[0-9]+$ ]] || fail "unexpected run number: $RUN_NUMBER"

ARTIFACT_TARBALL="$ARTIFACT_DIR/$TARBALL"
[[ -s "$ARTIFACT_TARBALL" ]] || fail "$ARTIFACT_TARBALL is missing"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/lemon-publish.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

actual_sha="$(shasum -a 256 "$ARTIFACT_TARBALL" | awk '{print $1}')"
[[ "$actual_sha" == "$SHA256" ]] || fail "artifact sha256 $actual_sha != $SHA256"
actual_size="$(stat -f%z "$ARTIFACT_TARBALL")"
[[ "$actual_size" == "$SIZE" ]] || fail "artifact size $actual_size != $SIZE"
tar -tzf "$ARTIFACT_TARBALL" > "$TMP_DIR/tarball.txt"
grep -qx 'lemonstudio/lemonstudio' "$TMP_DIR/tarball.txt" || fail "tarball does not contain lemonstudio/lemonstudio"

# Install both names atomically: the stable name is what the landing page links,
# the versioned name keeps the build identifiable.
mkdir -p "$LINUX_DIR"
versioned_tmp="$LINUX_DIR/.${TARBALL}.$$"
stable_tmp="$LINUX_DIR/.lemonstudio-linux-amd64.tar.gz.$$"
install -m 644 "$ARTIFACT_TARBALL" "$versioned_tmp"
install -m 644 "$ARTIFACT_TARBALL" "$stable_tmp"
mv -f "$versioned_tmp" "$LINUX_DIR/$TARBALL"
mv -f "$stable_tmp" "$LINUX_DIR/lemonstudio-linux-amd64.tar.gz"

(
  cd "$LINUX_DIR"
  shasum -a 256 "$TARBALL" lemonstudio-linux-amd64.tar.gz > SHA256SUMS
  shasum -a 256 -c SHA256SUMS
)

# latest.json mirrors Sinog's manifest shape: enough for a future updater to
# check the published build without scraping the landing page.
jq -n \
  --arg name "Lemon Studio" \
  --arg platform "linux-amd64" \
  --arg version "$VERSION" \
  --arg commit "$COMMIT" \
  --arg builtAt "$BUILT_AT" \
  --arg sha256 "$SHA256" \
  --arg url "$PUBLIC_URL" \
  --arg runUrl "https://github.com/ms-tristan/Lemon-Studio/actions/runs/$RUN_ID" \
  --argjson size "$SIZE" \
  --argjson runNumber "$RUN_NUMBER" \
  '{
    name: $name,
    platform: $platform,
    version: $version,
    commit: $commit,
    builtAt: $builtAt,
    size: $size,
    sha256: $sha256,
    url: $url,
    run: { number: $runNumber, url: $runUrl }
  }' > "$DIST_DIR/latest.json.tmp"
jq -e . "$DIST_DIR/latest.json.tmp" >/dev/null
mv -f "$DIST_DIR/latest.json.tmp" "$DIST_DIR/latest.json"

built_human="$(date -u -j -f "%Y-%m-%dT%H:%M:%SZ" "$BUILT_AT" "+%d %B %Y at %H:%M UTC" 2>/dev/null || printf '%s' "$BUILT_AT")"
size_human="$(awk -v bytes="$SIZE" 'BEGIN { printf "%.1f MiB", bytes/1048576 }')"
sed \
  -e "s|@@VERSION@@|$VERSION|g" \
  -e "s|@@COMMIT@@|$COMMIT|g" \
  -e "s|@@BUILT_AT@@|$built_human|g" \
  -e "s|@@SIZE@@|$size_human|g" \
  -e "s|@@SHA256@@|$SHA256|g" \
  -e "s|@@RUN_NUMBER@@|$RUN_NUMBER|g" \
  -e "s|@@RUN_URL@@|https://github.com/ms-tristan/Lemon-Studio/actions/runs/$RUN_ID|g" \
  "$TEMPLATE" > "$DIST_DIR/index.html.tmp"
if grep -q '@@' "$DIST_DIR/index.html.tmp"; then
  fail "landing page still contains placeholders"
fi
mv -f "$DIST_DIR/index.html.tmp" "$DIST_DIR/index.html"

ls -lh "$LINUX_DIR" "$DIST_DIR/index.html" "$DIST_DIR/latest.json"

if [[ "${LEMON_SKIP_HTTP_CHECK:-0}" != "1" ]]; then
  code="$(curl -sk -o /dev/null -w '%{http_code}' "$PUBLIC_URL")"
  [[ "$code" == "401" ]] || fail "expected 401 without credentials from $PUBLIC_URL, got $code"

  if [[ -f "$CREDENTIALS" ]]; then
    user="$(sed -n 's/^user=//p' "$CREDENTIALS" | head -n 1)"
    password="$(sed -n 's/^password=//p' "$CREDENTIALS" | head -n 1)"
    if [[ -n "$user" && -n "$password" ]]; then
      netrc="$TMP_DIR/netrc"
      ( umask 077; printf 'machine %s\n  login %s\n  password %s\n' "$PUBLIC_HOST" "$user" "$password" > "$netrc" )
      curl -skSf --netrc-file "$netrc" -o "$TMP_DIR/downloaded.tar.gz" "$PUBLIC_URL"
      downloaded_sha="$(shasum -a 256 "$TMP_DIR/downloaded.tar.gz" | awk '{print $1}')"
      [[ "$downloaded_sha" == "$SHA256" ]] || fail "public download sha256 $downloaded_sha != $SHA256"
      curl -skSf --netrc-file "$netrc" -o "$TMP_DIR/latest.json" "https://$PUBLIC_HOST/latest.json?build=$RUN_ID"
      jq -e --arg sha256 "$SHA256" '.sha256 == $sha256' "$TMP_DIR/latest.json" >/dev/null \
        || fail "published latest.json does not describe this build"
      echo "verified public download and latest.json (HTTP Basic auth)"
    else
      echo "warning: $CREDENTIALS has no user/password; skipped the authenticated check"
    fi
  else
    echo "warning: $CREDENTIALS is missing; skipped the authenticated check"
  fi
fi

{
  echo "### Linux amd64 published"
  echo
  echo "- Version: \`$VERSION\` (commit \`$COMMIT\`)"
  echo "- Tarballs: \`$TARBALL\` and \`lemonstudio-linux-amd64.tar.gz\`"
  echo "- SHA-256: \`$SHA256\`"
  echo "- Download: <$PUBLIC_URL>"
  echo "- Run: <https://github.com/ms-tristan/Lemon-Studio/actions/runs/$RUN_ID>"
} >> "${GITHUB_STEP_SUMMARY:-/dev/null}"

echo
echo "==> published"
echo "    dir      : $LINUX_DIR"
echo "    version  : $VERSION (commit $COMMIT)"
echo "    sha256   : $SHA256"
echo "    download : $PUBLIC_URL"
echo "    metadata : https://$PUBLIC_HOST/latest.json"
