// docov's npm launcher, run the way npm runs it. Node stands in for the docov binary, and
// NODE_PATH for the node_modules folder npm would install the platform package into.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { constants, copyFileSync, mkdirSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const launcher = fileURLToPath(new URL("docov/bin/docov", import.meta.url));
const pkg = `@docov/${process.platform}-${process.arch}`;

function launch(args, env) {
  // The test runner sets FORCE_COLOR, which would color what Node prints.
  const { DOCOV_BINARY, NODE_PATH, FORCE_COLOR, ...rest } = process.env;
  return spawnSync(process.execPath, [launcher, ...args], { encoding: "utf8", env: { ...rest, ...env } });
}

test("runs the binary from this platform's package with the arguments it was given", () => {
  const modules = mkdtempSync(join(tmpdir(), "docov-"));
  mkdirSync(join(modules, pkg), { recursive: true });
  const exe = join(modules, pkg, process.platform === "win32" ? "docov.exe" : "docov");
  copyFileSync(process.execPath, exe, constants.COPYFILE_FICLONE);

  const result = launch(["-p", "40 + 2"], { NODE_PATH: modules });
  assert.equal(result.stdout, "42\n");
  assert.equal(result.status, 0);
});

test("runs DOCOV_BINARY and exits with its status", () => {
  const result = launch(["-e", "process.exit(3)"], { DOCOV_BINARY: process.execPath });
  assert.equal(result.status, 3);
});

test("exits 1 when the binary is killed", () => {
  const result = launch(["-e", "process.kill(process.pid, 'SIGKILL')"], { DOCOV_BINARY: process.execPath });
  assert.equal(result.status, 1);
});

test("says which package is missing", () => {
  const result = launch([], { NODE_PATH: mkdtempSync(join(tmpdir(), "docov-")) });
  assert.match(result.stderr, new RegExp(`${pkg} isn't installed`));
  assert.equal(result.status, 2);
});

test("says when the binary can't run", () => {
  const result = launch([], { DOCOV_BINARY: join(tmpdir(), "no-such-docov") });
  assert.match(result.stderr, /couldn't run .*no-such-docov/);
  assert.equal(result.status, 2);
});
