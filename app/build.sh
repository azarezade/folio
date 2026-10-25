#!/bin/sh
# Builds Folio.app, ad-hoc signed.
#   app/build.sh              this Mac's architecture  → target/release/bundle/macos/
#   app/build.sh --dmg        plus a .dmg              → target/release/bundle/dmg/
#   app/build.sh --universal  Apple silicon + Intel, with a .dmg
#                             → target/universal-apple-darwin/release/bundle/{macos,dmg}/
set -e
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
(cd ui && npm install --no-audit --no-fund && npm run build)
python3 ../tools/notices.py
case "$1" in
  --dmg) ui/node_modules/.bin/tauri build --bundles app,dmg ;;
  --universal) ui/node_modules/.bin/tauri build --target universal-apple-darwin --bundles app,dmg ;;
  *) ui/node_modules/.bin/tauri build --bundles app ;;
esac
