#!/usr/bin/env node
// Runs the docov binary from the package npm installed for this platform, one of docov's
// optionalDependencies. DOCOV_BINARY runs another binary instead.
const { spawnSync } = require("node:child_process");

const pkg = `@docov/${process.platform}-${process.arch}`;
let binary = process.env.DOCOV_BINARY;
if (!binary) {
  try {
    binary = require.resolve(`${pkg}/docov${process.platform === "win32" ? ".exe" : ""}`);
  } catch {
    console.error(
      `docov: ${pkg} isn't installed. npm installs it with @docov/cli unless optional dependencies are left out (--omit=optional). ` +
        "If docov has no binary for this platform, run `cargo install docov` and set DOCOV_BINARY to its path.",
    );
    process.exit(2);
  }
}
const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`docov: couldn't run ${binary}: ${result.error.message}`);
  process.exit(2);
}
process.exit(result.status ?? 1);
