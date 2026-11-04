// tools/release-pr-changelog.mjs on the changelog uppt writes, down to the link after each entry.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { dropRepeatedEntries } from "./release-pr-changelog.mjs";

const entry = (text, sha) => `- ${text} ([\`${sha}\`](https://github.com/cutenode/docov/commit/${sha}))`;

// A stand-in for gh that lists $PRS and records what it's asked to write. The script is found by
// its shebang, which Windows doesn't have.
test("rewrites the open release PRs of this repository that have repeats", { skip: process.platform === "win32" }, () => {
  const dir = mkdtempSync(join(tmpdir(), "docov-"));
  const writes = join(dir, "writes");
  writeFileSync(
    join(dir, "gh"),
    `#!/usr/bin/env node
const { appendFileSync, readFileSync } = require("node:fs");
const args = process.argv.slice(2);
if (args[0] === "pr") console.log(process.env.PRS);
else appendFileSync(${JSON.stringify(writes)}, args[3] + " " + readFileSync(0, "utf8") + "\\n");
`,
    { mode: 0o755 },
  );
  const repeated = [entry("add docov lsp", "f199ab1"), entry("add docov lsp", "e90c0bb")].join("\n");
  const pr = (number, headRefName, isCrossRepository, body) => ({ number, headRefName, isCrossRepository, body });
  const prs = [
    pr(1, "release/v1.0.0", false, repeated),
    pr(2, "release/v2.0.0", false, entry("add docov lsp", "f199ab1")),
    pr(3, "release/v3.0.0", true, repeated),
    pr(4, "bnb/feature", false, repeated),
  ];

  execFileSync(process.execPath, [fileURLToPath(new URL("release-pr-changelog.mjs", import.meta.url))], {
    env: { ...process.env, PATH: `${dir}${delimiter}${process.env.PATH}`, PRS: JSON.stringify(prs) },
  });

  const body = JSON.stringify({ body: entry("add docov lsp", "f199ab1") });
  assert.equal(readFileSync(writes, "utf8"), `repos/{owner}/{repo}/pulls/1 ${body}\n`);
});

test("keeps the first of the entries that differ only by commit", () => {
  const lsp = entry("**lsp:** add docov lsp", "f199ab1");
  const stubs = entry("**fix:** write stubs", "b3c9f34");
  const body = ["### 🚀 Enhancements", "", lsp, stubs, entry("**lsp:** add docov lsp", "e90c0bb")];

  assert.equal(dropRepeatedEntries(body.join("\n")), ["### 🚀 Enhancements", "", lsp, stubs].join("\n"));
});

test("keeps the same words in different sections, which are different commits", () => {
  const body = ["### 🩹 Fixes", "", entry("handle paths", "aaaaaaa"), "", "### ✅ Tests", "", entry("handle paths", "bbbbbbb")];

  assert.equal(dropRepeatedEntries(body.join("\n")), body.join("\n"));
});

test("leaves everything that isn't a commit entry alone", () => {
  const body = [
    "> v0.2.0 is the next major release.",
    "",
    "### 🎉 New Contributors",
    "",
    "- dependabot[bot] (@dependabot[bot])",
    "- dependabot[bot] (@dependabot[bot])",
    "- a line with a pull request (#12)",
    "- a line with a pull request (#12)",
  ];

  assert.equal(dropRepeatedEntries(body.join("\n")), body.join("\n"));
});
