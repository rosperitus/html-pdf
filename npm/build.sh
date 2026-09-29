#!/bin/sh
# Builds the Node.js (CommonJS) and web (ES module) packages from ../wasm.
set -e
cd "$(dirname "$0")"
for target in nodejs web; do
    out=$([ "$target" = nodejs ] && echo node || echo web)
    rm -rf "$out"
    wasm-pack build ../wasm --release --target "$target" --out-dir "../npm/$out" --out-name html_pdf --no-pack
    rm -f "$out/.gitignore"
done
# Nested package.json files tell Node.js how to load each directory's .js files.
echo '{ "type": "commonjs" }' > node/package.json
echo '{ "type": "module" }' > web/package.json
cp ../README.md ../LICENSE .
