# docov

JSDoc coverage for JavaScript and TypeScript: how much of a project's API has a doc comment, and which declarations don't. It parses with [oxc](https://oxc.rs), so it reads TypeScript, JSX, ES modules and CommonJS without a build step.

```
$ docov --threshold 80
  50.0%  1/2  src/a.ts
         src/a.ts:3:1  variable b
 100.0%  1/1  src/c.js
  66.7%  2/3  total
docov: 66.7% documented, under the 80% threshold
```

Each undocumented item is listed as `path:line:column`, the spot its JSDoc goes, which terminals and editors open directly.

## Install

```bash
npm install --save-dev @docov/cli   # or pnpm, yarn, bun
cargo install docov
```

The npm package runs a prebuilt binary for macOS, Linux or Windows on x64 or arm64, from the `@docov/<platform>-<arch>` package npm installs alongside it. `DOCOV_BINARY` runs a different one.

## Usage

```
docov [PATHS]... [--config FILE] [--threshold PERCENT] [--scope exports|all] [--format text|json|lcov]
```

With no paths it scans the current folder: every `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.mts`, `.cts` and `.tsx` file, leaving out hidden files and what `.gitignore` leaves out (in a git repository or not). A file named on the command line is always scanned.

`docov init` writes a docov.json in the current folder with the `$schema` for this version and a `threshold` of its coverage now, rounded down, and won't write over a config file that's there. (To scan a folder named `init`, use `docov ./init`.)

It exits 0 when coverage meets the threshold (or there isn't one), 1 when it's under, and 2 when a file doesn't parse or the config or a path can't be read. Every file has to parse, since a partial one would undercount.

### What counts

With the default scope, `exports`, an item is anything a module exports:

- functions, classes, variables, interfaces, type aliases, enums and namespaces, whether exported where they're declared, with `export { a, b as c }`, with `export default a`, or with `export = a`
- `export default` of an expression
- CommonJS: `exports.a = …`, `module.exports.a = …`, `module.exports = …`, and each property of `module.exports = { … }`
- the public members of exported classes (methods, properties, accessors, with a getter and its setter as one) and interfaces (properties and methods)

An item is documented when a `/** … */` block with something in it sits right before it, or before its `export` or decorators. Ordinary comments, an empty `/** */`, and license headers (`/** @license … */`) don't count.

Some things never count: constructors (their class's doc covers them), private members (`#x` and `private x`), an overload's implementation (only its signatures), destructured variables, `require()` bindings, `declare module` and `declare global` blocks, and re-exports from another file (`export { x } from './x'`, `export * from`), whose declarations count in their own file.

`--scope all` counts every module-level declaration, exported or not.

### docov.json

`docov.json` (or `docov.jsonc`, or `.docovrc`) holds the same settings. Flags win over it. docov reads the one in the folder it runs from, or else the nearest folder above that has one, so it finds the project's config from a subfolder too. In one folder, `docov.json` comes first and `.docovrc` last. `--config FILE` names one instead.

```jsonc
{
  "$schema": "https://raw.githubusercontent.com/cutenode/docov/v0.1.1/docov.schema.json",
  // Relative to this file's folder, wherever docov runs from.
  "paths": ["src"],
  "exclude": ["**/*.test.ts", "src/generated/**"],
  "threshold": 80,
  "scope": "exports",
}
```

`paths` and `exclude` (gitignore-style globs) are relative to the config's folder. Comments and trailing commas are fine in any of the three.

`"extends": "../docov.json"` (or a list of paths) builds on other config files, relative to the one that names them. Its own keys win, and in a list a later file's win over an earlier one's. Each file's `paths` and `exclude` stay relative to that file. Editors don't know `.docovrc` is JSON, so tell yours (in VS Code, `"files.associations": { ".docovrc": "jsonc" }`).

`--jsdoc-config FILE` (or `"jsdocConfig": "jsdoc.json"`, relative to the config) scans the files a [jsdoc config](https://jsdoc.app/about-configuring-jsdoc) documents: its `source.include` are the paths, unless there are paths already, and `source.exclude` leaves files and folders out, both relative to the jsdoc config. `source.includePattern` and `excludePattern` filter files only when the config writes them, since jsdoc's default `includePattern` would leave out TypeScript. It has to be JSON; docov can't read a `.js` config.

With `"nested": true` in the root config, each file is scanned with the nearest docov.json, docov.jsonc or .docovrc in a folder above it, up to the root config's folder, merged over the root config, the way [oxlint's nested configs](https://oxc.rs/docs/guide/usage/linter/nested-config.html) work. A nested config's `exclude` applies in its folder, its `threshold` holds for its own files (a failure names the config), and its `paths` aren't read.

`--by-package` adds a total line for each workspace package after the text total, and a `packages` array to the JSON. The packages are the folders with a package.json that the globs in the root config folder's package.json `workspaces` or pnpm-workspace.yaml `packages` match.

`--print-config` prints the settings a scan would use as JSON: the config files they came from, the paths, the excludes and each feature's setting, with the flags applied. `--list-files` prints the files it would scan. Neither scans.

[`docov.schema.json`](docov.schema.json) is its JSON Schema, which editors use for completion and checking. The URL above is pinned to a release so it matches the docov you have; `docov --print-schema > docov.schema.json` writes your own copy to point at instead.

### Formats

- `text` (the default): each file's percentage and its undocumented items, then the total.
- `json`: every item, documented or not, with its kind, line and column, and totals per file and overall.
- `lcov`: a line record for each line with items on it, hit when all of them are documented. Coverage services and editor gutters read it.

## Working on it

The toolchain is pinned in `rust-toolchain.toml`, and Node in `.node-version`. The checks need [cargo-nextest](https://nexte.st) and [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov).

Each flag and docov.json key belongs to a feature, one file in `src/feature/` that implements `Feature`. `build.rs` registers every file there, so a new feature is a new file and nothing else.

```bash
cargo run -- [PATHS]...   # docov on a project
tools/check.sh            # what CI runs: tools/lint.sh (format, clippy) and tools/coverage.sh (coverage for the whole suite, for the Hegel properties alone, and for the npm launcher's tests)
git tag v1.2.3 && git push origin v1.2.3   # release Cargo.toml's version 1.2.3 to crates.io and npm
```

Pushing a version tag runs `.github/workflows/release.yml`, which builds the binaries and publishes with trusted publishing, so there are no registry tokens. The npm packages are `@docov/cli` (`npm/docov`, a launcher) and an `@docov/<platform>-<arch>` package per binary. `tools/bootstrap-publishing.sh` publishes the first version by hand and sets up the trust, which both registries only allow for a package they already have.

`cargo coverage` fails under the thresholds in `.cargo/config.toml`. On a pull request, lint and coverage are separate pass/fail checks for each OS. With `LCOV_DIR` set, `tools/coverage.sh` writes each Rust coverage run to `$LCOV_DIR/suite.lcov` and `props.lcov` in place of its table. CI sets it, puts each run's per-file numbers in the job summary, and uploads the lcov files as the `coverage-<os>` artifact. The property tests (`tests/props.rs`) use [Hegel](https://github.com/hegeldev/hegel-rust). They check scans against a model of generated modules, and the binary against generated projects.
