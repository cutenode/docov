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
cargo install --git https://github.com/cutenode/docov
```

## Usage

```
docov [PATHS]... [--config FILE] [--threshold PERCENT] [--scope exports|all] [--format text|json|lcov]
```

With no paths it scans the current folder: every `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.mts`, `.cts` and `.tsx` file, leaving out hidden files and what `.gitignore` leaves out (in a git repository or not). A file named on the command line is always scanned.

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

`docov.json` (or `docov.jsonc`) holds the same settings. Flags win over it. docov reads the one in the folder it runs from, or else the nearest folder above that has one, so it finds the project's config from a subfolder too. `--config FILE` names one instead.

```jsonc
{
  "$schema": "https://raw.githubusercontent.com/cutenode/docov/v0.1.0/docov.schema.json",
  // Relative to this file's folder, wherever docov runs from.
  "paths": ["src"],
  "exclude": ["**/*.test.ts", "src/generated/**"],
  "threshold": 80,
  "scope": "exports",
}
```

`paths` and `exclude` (gitignore-style globs) are relative to the config's folder. Comments and trailing commas are fine in either name.

[`docov.schema.json`](docov.schema.json) is its JSON Schema, which editors use for completion and checking. The URL above is pinned to a release so it matches the docov you have; `docov --print-schema > docov.schema.json` writes your own copy to point at instead.

### Formats

- `text` (the default): each file's percentage and its undocumented items, then the total.
- `json`: every item, documented or not, with its kind, line and column, and totals per file and overall.
- `lcov`: a line record for each line with items on it, hit when all of them are documented. Coverage services and editor gutters read it.

## Working on it

The toolchain is pinned in `rust-toolchain.toml`. The checks need [cargo-nextest](https://nexte.st) and [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov).

```bash
cargo run -- [PATHS]...   # docov on a project
tools/check.sh            # what CI runs: format, clippy, and coverage for the whole suite and for the Hegel properties alone
cargo publish             # publish to crates.io
```

`cargo coverage` fails under the thresholds in `.cargo/config.toml`. The property tests (`tests/props.rs`) use [Hegel](https://github.com/hegeldev/hegel-rust). They check scans against a model of generated modules, and the binary against generated projects.
