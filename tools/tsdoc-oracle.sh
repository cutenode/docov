#!/usr/bin/env bash
# Records what @microsoft/tsdoc, the parser eslint-plugin-tsdoc runs, finds in each comment of
# tests/fixtures/tsdoc/corpus.json, as a test oracle: each entry's `messages` becomes its message
# ids, texts and byte offsets into the comment, with the entry's `config` (a tsdoc.json's keys) set
# up the way @microsoft/tsdoc-config does, and `throws` says when the parser throws instead. The
# docov tests check docov's TSDoc parser against the file. Not part of the checks or CI.
# Usage: tools/tsdoc-oracle.sh [CORPUS]
set -euo pipefail
cd "$(dirname "$0")/.."

corpus=${1:-tests/fixtures/tsdoc/corpus.json}
deps=$(mktemp -d)
trap 'rm -rf "$deps"' EXIT

npm install --silent --no-save --prefix "$deps" @microsoft/tsdoc@0.17.1 @microsoft/tsdoc-config@0.18.3 >/dev/null

NODE_PATH="$deps/node_modules" node - "$corpus" <<'JS'
const fs = require("node:fs");
const { TSDocConfiguration, TSDocParser } = require("@microsoft/tsdoc");
const { TSDocConfigFile } = require("@microsoft/tsdoc-config");
const [file] = process.argv.slice(2);

const entries = JSON.parse(fs.readFileSync(file, "utf8"));
for (const entry of entries) {
  delete entry.messages;
  delete entry.throws;
  const configuration = new TSDocConfiguration();
  if (entry.config) {
    const config = TSDocConfigFile.loadFromObject({ $schema: TSDocConfigFile.CURRENT_SCHEMA_URL, ...entry.config });
    config.configureParser(configuration);
  }
  try {
    const { log } = new TSDocParser(configuration).parseString(entry.comment);
    // Offsets in UTF-8 bytes, as Rust's strings index.
    const at = (pos) => Buffer.byteLength(entry.comment.slice(0, pos));
    entry.messages = log.messages.map((m) => ({ id: m.messageId, text: m.unformattedText, at: at(m.textRange.pos) }));
  } catch (error) {
    entry.throws = error.message;
  }
}
fs.writeFileSync(file, JSON.stringify(entries, null, 2) + "\n");
JS
