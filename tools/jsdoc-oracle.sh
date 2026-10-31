#!/usr/bin/env bash
# Compares docov with jsdoc on a folder of plain JavaScript, as a test oracle: runs jsdoc 4's
# `-X` (every doclet, documented or not, through npx) and `docov --scope all --format json` on the
# same files, and prints each declaration they disagree on: one of them counts it and the other
# doesn't, or they disagree on whether it's documented. Not part of the checks or CI.
# Usage: tools/jsdoc-oracle.sh [DIR]   (default: tests/fixtures/jsdoc)
set -euo pipefail
cd "$(dirname "$0")/.."

dir=${1:-tests/fixtures/jsdoc}
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT

npx --yes jsdoc@4 -X -r "$dir" > "$out/jsdoc.json"
cargo run -q -- --scope all --format json "$dir" > "$out/docov.json"

node - "$out/jsdoc.json" "$out/docov.json" <<'JS'
const fs = require("node:fs");
const path = require("node:path");
const [jsdocFile, docovFile] = process.argv.slice(2);

// Each declaration by its file, line and own name, and whether it's documented.
const key = (file, line, name) => `${path.basename(file)}:${line} ${name}`;

// jsdoc: the doclets for module-level declarations and their members, leaving out what it makes
// up for the files themselves. A symbol can have several doclets, and one with a comment counts.
const jsdoc = new Map();
for (const doclet of JSON.parse(fs.readFileSync(jsdocFile, "utf8"))) {
  if (!doclet.meta?.filename || ["package", "file"].includes(doclet.kind)) continue;
  const moduleLevel = ["global", "static", "instance"].includes(doclet.scope) || doclet.memberof?.startsWith("module:");
  if (!moduleLevel) continue;
  const at = key(doclet.meta.filename, doclet.meta.lineno, doclet.name);
  jsdoc.set(at, jsdoc.get(at) || !doclet.undocumented);
}

const docov = new Map();
for (const file of JSON.parse(fs.readFileSync(docovFile, "utf8")).files) {
  for (const item of file.items) {
    docov.set(key(file.path, item.line, item.name.split(".").pop()), item.documented);
  }
}

const said = (documented) => (documented ? "documented" : "undocumented");
const lines = [];
for (const at of new Set([...jsdoc.keys(), ...docov.keys()])) {
  if (!docov.has(at)) lines.push(`${at}: only jsdoc has it (${said(jsdoc.get(at))})`);
  else if (!jsdoc.has(at)) lines.push(`${at}: only docov has it (${said(docov.get(at))})`);
  else if (jsdoc.get(at) !== docov.get(at)) {
    lines.push(`${at}: docov says ${said(docov.get(at))}, jsdoc says ${said(jsdoc.get(at))}`);
  }
}
lines.sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
console.log(lines.length ? lines.join("\n") : "no disagreements");
JS
