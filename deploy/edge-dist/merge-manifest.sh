#!/bin/sh
# Merge ONE platform's entry into /ibbf/edge-dist/manifest.json.
#
#   merge-manifest.sh <platform> <version> <file> <sha256>
#
# The manifest is what control advertises as "latest". Before it existed, the advertised version came
# from `sentinel-edge/Cargo.toml` via deploy.yml — i.e. from SOURCE, not from anything published — so
# bumping the crate and pushing any unrelated change told every appliance a version existed that may
# never have been built. macOS failed loudly on a 404; Linux failed SILENTLY, because its filename
# carried no version and the box just re-downloaded whatever was sitting there.
#
# MERGE, never replace. macOS is built out-of-band (release.yml has no macOS runner yet), so a Linux
# release and a macOS release write this file at different times. A whole-file write would delete the
# other platform's entry and re-create the exact bug this exists to remove.
#
# Written atomically (temp + mv) because control reads this file on every /edge-latest; a half-written
# manifest would be a parse error served to every appliance at once.
set -eu

PLATFORM="${1:?platform, e.g. linux-x86_64 | linux-aarch64 | macos | windows}"
VERSION="${2:?version, e.g. 0.5.65}"
FILE="${3:?filename as served under /edge/}"
SHA="${4:?sha256 of the file}"

DIR="${EDGE_DIST_DIR:-/ibbf/edge-dist}"
MANIFEST="$DIR/manifest.json"

[ -f "$DIR/$FILE" ] || { echo "refusing: $DIR/$FILE does not exist" >&2; exit 1; }

TMP="$(mktemp "$DIR/.manifest.XXXXXX")"
trap 'rm -f "$TMP"' EXIT

MANIFEST="$MANIFEST" PLATFORM="$PLATFORM" VERSION="$VERSION" FILE="$FILE" SHA="$SHA" TMP="$TMP" \
python3 - <<'PY'
import json, os, datetime

path = os.environ["MANIFEST"]
try:
    with open(path) as fh:
        doc = json.load(fh)
except (OSError, ValueError):
    doc = {}

platforms = doc.get("platforms") or {}
platforms[os.environ["PLATFORM"]] = {
    "version": os.environ["VERSION"],
    "file": os.environ["FILE"],
    "sha256": os.environ["SHA"],
    "published_at": datetime.datetime.now(datetime.timezone.utc)
        .replace(microsecond=0).isoformat(),
}
doc["platforms"] = platforms
doc["generated_at"] = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat()

with open(os.environ["TMP"], "w") as fh:
    json.dump(doc, fh, indent=2, sort_keys=True)
    fh.write("\n")
PY

chmod 0644 "$TMP"
mv "$TMP" "$DIR/manifest.json"
trap - EXIT
echo "manifest: $PLATFORM -> $VERSION ($FILE)"
