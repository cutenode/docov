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
docov [PATHS]... [--config FILE] [--threshold PERCENT] [--scope exports|all|public] [--format text|json|lcov]
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

`--require-description` (`"requireDescription": true`) also wants the block to describe the item: text before its first tag, or a `@description`, `@desc`, `@summary`, `@classdesc` (on a class) or `@inheritdoc` tag. Without it, `/** @type {string} */` counts. A block of `@inheritdoc` alone documents its item either way, and `"inherit": "override"` counts a TypeScript class member marked `override` as documented too, since it inherits its docs.

`--exclude-tag NAME` (repeatable) or `"excludeTags": ["internal", "ignore", "hidden"]` leaves out items whose JSDoc has one of those tags: they're out of the total, not documented. `--ignore-name REGEX` (repeatable) or `"ignore": { "names": ["^_"] }` does the same for items whose name matches, a member by its own name or as `Class.member`.

A `// docov-ignore-next-line` comment (with an optional reason, `// docov-ignore-next-line: legacy API`) takes the items on the line after it out of the total, and `/* docov-ignore-file */` anywhere at the top level does the same for the whole file. A documented item still counts as documented, as rustdoc does. The text report doesn't list ignored items, and the JSON report marks them with `"ignored": true` and their `reason`. `--report-unused-ignores` (`"reportUnusedIgnores": true`) fails the scan on a `docov-ignore-next-line` whose items are documented anyway.

To adopt docov in a project that's far from documented, `--add-ignore` (or `--add-ignore=REASON`) writes a `// docov-ignore-next-line` above every undocumented item, indented like it, and reports how many it wrote instead of the coverage. It changes no other line.

`--explain FILE` prints, in place of a report, a line for each item in `FILE` saying whether it counts and why (documented by the block at a line, ignored by a comment, or left out by a rule), and one for each declaration or block the scan passed over there and why (a re-export, a constructor, an overload's implementation, a destructured variable, a `require()` binding, a license header, a private member). It exits 0.

Some things never count: constructors (their class's doc covers them), private members (`#x` and `private x`), an overload's implementation (only its signatures), destructured variables, `require()` bindings, `declare module` and `declare global` blocks, and re-exports from another file (`export { x } from './x'`, `export * from`), whose declarations count in their own file.

`--scope all` counts every module-level declaration, exported or not.

`--scope public` counts only a package's public API: what its entry points export. It reads the package.json in the config's folder (or else the nearest one above the scanned paths), takes every path in its `exports` (under every condition and subpath, leaving out `*` patterns), `main`, `module`, `types` and `typings`, and finds each one's source file, back through tsconfig.json's `outDir` and `rootDir` (or `src`) for a built file, and from a `.d.ts` to the module it declares. From there it follows `export * from`, `export { x } from`, `export * as ns from` and imports exported again through the module graph, the way Node and TypeScript resolve them, and counts each declaration it reaches once, in its own file. A reached file's CommonJS exports count too. An entry point with no source file is an error (exit 2).

`"members": "all"` counts more members, where their parent counts: TypeScript enum members (`Color.Red`), the members of a type alias's object type (`Options.timeout`), and the properties of an object literal whose block has `@enum`, `@namespace` or `@lends X` (`Name.prop`, or `X.prop`). A matching `@property` in the parent's block documents one of these. The default, `"classes"`, counts only the members of classes and interfaces.

`"commonjsMembers": true` counts `X.prototype.m = …` and `X.m = …` at the top level as members of `X` (`X.m`, a method or property by its value) when `X` is a function or class declared in the file, counting where `X` does. In a CommonJS file (`.cjs`, or a `.js` file with no `import` or `export`), `this.a = …` at the top level then exports `a` the way `exports.a = …` does.

`"fileDocs": "require"` makes each file an item of its own, of kind `module`, at its first line and named by its `@module` or else its path. It's documented when the file's first JSDoc block has `@file`, `@fileoverview`, `@overview`, `@module` or `@packageDocumentation`, with text, and that block then documents nothing else. Without it, a block like that right before a declaration documents the declaration.

`"virtual": true` counts a JavaScript file's `@typedef` and `@callback` blocks that name a type as items of kind `type`, at the block, documented when the block or the tag describes the type. They count as exported under the `exports` scope, since JSDoc treats them as the module's public types, and they then document nothing else. Without it, such a block right before a declaration documents the declaration.

`"exports": { "esm": true, "cjs": true, "globals": false }` picks which ways of exporting count under the `exports` scope (`cjs` covers TypeScript's `export =` too). With `globals` on, `window.a = …` and `globalThis.a = …` at the top level export `a` the way `exports.a = …` does, under either scope.

`--kind function,class` (or `"kinds": ["function", "class"]`) counts only those kinds of item, named as the reports name them: `function`, `class`, `method`, `accessor`, `property`, `variable`, `interface`, `type`, `enum`, `namespace`, `default` and `module`. `--by-kind` adds a line of totals by kind after the text report's total, and the JSON report always has them, under `kinds`.

`--access public,protected` (or `"access": ["public", "protected", "package"]`) counts only items with those access levels, like jsdoc's `--access`: an item's level is its `@public`, `@protected`, `@package` or `@private` tag (or `@access` with one of those), else `protected` for a TypeScript `protected` member, else `public`. Without it, access tags change nothing.

Example coverage is a second figure: the share of counted items whose JSDoc has an `@example`, as rustdoc's `--show-coverage` counts it. The JSON report always has it (`withExamples` and `examplesPercent`, for each file and in all), `--examples` adds it to each line of the text report, and `--examples-threshold PERCENT` (`"examplesThreshold"`) fails the scan under it.

`--release-tags` adds release-tag coverage: the share of counted top-level exports with exactly one of `@public`, `@beta`, `@alpha` and `@internal`, which [API Extractor asks for](https://api-extractor.com/pages/messages/ae-missing-release-tag/) (members take their container's, so they aren't counted). It's a line after the text report's total and `releaseTagged` and `releaseTagsPercent` in the JSON report, and `--release-tags-threshold PERCENT` (`"releaseTagsThreshold"`, which turns it on too) fails the scan under it.

`--types` adds type coverage for JavaScript (`.js`, `.mjs`, `.cjs` and `.jsx` files): the share of counted functions and methods whose JSDoc gives each `@param` a `{type}`, and its `@returns` one when it has one. An undocumented function isn't typed. It's a line after the text report's total and `typed` and `typesPercent` in the JSON report, and `--types-threshold PERCENT` (`"typesThreshold"`, which turns it on too) fails the scan under it.

`--complete` adds complete coverage: the share of counted items that are documented with a matching `@param` for each parameter. It's a line after the text report's total and `complete` and `completePercent` in the JSON report, and `--complete-threshold PERCENT` (`"completeThreshold"`, which turns it on too) fails the scan under it.

`"ignoreDeprecated": true` leaves out items whose JSDoc has `@deprecated`, and `"skipGenerated": true` leaves out every item in a file whose first comment has `@generated`.

### Doc checks

Checks on what each counted item's JSDoc block says. Each is off until `--rule ID=LEVEL` (repeatable) or docov.json's `rules` turns it on, at `warn` or `error`, with its options if it has some:

```jsonc
"rules": { "require-param": "error", "param-names": ["warn"] }
```

Each finding prints to stderr as `docov: path:line:column: rule-id: message`, and one at `error` fails the scan (exit 1). A flag's level wins over the config's, which keeps its options. The checks that want a tag leave a block with `@inheritdoc` alone.

- `require-param`: each parameter of a function or method has a matching `@param`, by name, or for a destructured one, at its place (as `paramsDocumented` counts them).
- `param-names`: each `@param` names a parameter, so none is left over from a rename. A destructured parameter takes the name of the `@param` at its place, and `@param options.a` belongs to `options`.
- `require-returns`: a function or method whose own body returns a value has `@returns`, unless its return type is `void`, `undefined` or `Promise<void>`.
- `require-yields`: a generator that yields a value has `@yields`.
- `require-throws`: an exported function or method with a `throw` of its own (outside a `try` that catches it) has `@throws`.
- `require-template`: each type parameter of a declaration has a `@template`.

`const f = () => …` is a variable to docov, so the checks on parameters, returns and throws don't look at it.

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

`paths` and `exclude` (gitignore-style globs) are relative to the config's folder. Comments and trailing commas are fine in any of the three. Editors don't know `.docovrc` is JSON, so tell yours (in VS Code, `"files.associations": { ".docovrc": "jsonc" }`).

[`docov.schema.json`](docov.schema.json) is its JSON Schema, which editors use for completion and checking. The URL above is pinned to a release so it matches the docov you have; `docov --print-schema > docov.schema.json` writes your own copy to point at instead.

### Formats

- `text` (the default): each file's percentage and its undocumented items, then the total. `--sort priority` lists each file's undocumented items by what to document first: exported functions and methods (more parameters first), then classes, then the rest.
- `json`: every item, documented or not, with its kind, line and column, and totals per file, per kind and overall. Each item also says whether its JSDoc describes it (`hasDescription`) and has an `@example` (`hasExample`), whether each parameter has a matching `@param` (`paramsDocumented`, `null` for what isn't a function or method), whether it's `deprecated`, and its `since` text (or `null`).
- `lcov`: a line record for each line with items on it, hit when all of them are documented. Coverage services and editor gutters read it.

## Working on it

The toolchain is pinned in `rust-toolchain.toml`, and Node in `.node-version`. The checks need [cargo-nextest](https://nexte.st) and [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov).

Each flag and docov.json key belongs to a feature, one file in `src/feature/` that implements `Feature`, and each doc check is one file in `src/check/` that implements `Check`, its rule id the file name in kebab case. `build.rs` registers every file in both, so a new feature or check is a new file and nothing else.

```bash
cargo run -- [PATHS]...   # docov on a project
tools/check.sh            # what CI runs: tools/lint.sh (format, clippy) and tools/coverage.sh (coverage for the whole suite, for the Hegel properties alone, and for the npm launcher's tests)
tools/jsdoc-oracle.sh [DIR]   # where docov and jsdoc disagree on a folder of plain JavaScript (default: tests/fixtures/jsdoc)
git tag v1.2.3 && git push origin v1.2.3   # release Cargo.toml's version 1.2.3 to crates.io and npm
```

Pushing a version tag runs `.github/workflows/release.yml`, which builds the binaries and publishes with trusted publishing, so there are no registry tokens. The npm packages are `@docov/cli` (`npm/docov`, a launcher) and an `@docov/<platform>-<arch>` package per binary. `tools/bootstrap-publishing.sh` publishes the first version by hand and sets up the trust, which both registries only allow for a package they already have.

`cargo coverage` fails under the thresholds in `.cargo/config.toml`. On a pull request, lint and coverage are separate pass/fail checks for each OS. With `LCOV_DIR` set, `tools/coverage.sh` writes each Rust coverage run to `$LCOV_DIR/suite.lcov` and `props.lcov` in place of its table. CI sets it, puts each run's per-file numbers in the job summary, and uploads the lcov files as the `coverage-<os>` artifact. The property tests (`tests/props.rs`) use [Hegel](https://github.com/hegeldev/hegel-rust). They check scans against a model of generated modules, and the binary against generated projects.

`tools/jsdoc-oracle.sh` runs jsdoc 4 (`npx --yes jsdoc@4 -X`, so it needs Node and the network) and `docov --scope all --format json` on the same files, and lists each declaration only one of them counts, or that they disagree on documenting. It isn't part of the checks: some disagreements are by design (jsdoc counts constructors, `this.x =` members, object literal members, destructured variables and `/** */` and `@license` blocks), and a `@typedef` or `@file` block right before an undocumented declaration documents it for docov but not for jsdoc unless `"virtual"` or `"fileDocs"` is on.
