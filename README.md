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
docov [PATHS]... [--config FILE] [--threshold PERCENT] [--scope exports|all|public] [--format FORMAT[=FILE]]... [--output FILE] [--badge FILE] [--compare FILE] [--omit-covered] [--quiet]
docov init
```

`docov --help` lists every flag.

With no paths it scans the current folder: every `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.mts`, `.cts` and `.tsx` file, leaving out hidden files and what `.gitignore` leaves out (in a git repository or not). A file named on the command line is always scanned.

It exits 0 when every gate passes (or there isn't one), 1 when one fails, such as coverage under a threshold, and 2 when a file doesn't parse or the config or a path can't be read. Every file has to parse, since a partial one would undercount.

`--changed` scans only the files that differ from where HEAD branched off the default branch (`origin/HEAD`, or `main` without one), committed or not, and untracked ones. `--since REF` compares with another branch or commit instead. `--staged` scans only the staged files, for a pre-commit hook. Paths given on the command line still narrow the scan.

`--diff-threshold PERCENT` fails the scan when the items on lines a change added (committed or not, and every line of an untracked file) are under that documented percentage, wherever the rest of the project is. The change is from where HEAD branched off the default branch, or off `--diff-base REF`. These run `git` (2.30 or later).

`docov init` writes a docov.json in the current folder with the `$schema` for this version and a `threshold` of its coverage now, rounded down, and won't write over a config file that's there. (To scan a folder named `init`, use `docov ./init`.)

`--watch` scans again whenever a JavaScript, TypeScript or config file under the paths changes, with a line between reports, until Ctrl-C.

`--stdin-filename PATH` scans stdin in place of the paths, as the file at PATH, whose extension picks the language, for editors to check a buffer before it's saved. It can't go with `--add-ignore`, `--fix`, `--write-baseline`, `--update-threshold`, `--watch`, `--changed` or `--staged`, which would act on that one file.

`--cache` keeps each file's items in `.docov-cache` in the config's folder (or the file `--cache-location` names), and the next scan reuses them for each file whose size and modified time haven't changed, as long as docov's version and the settings that change what counts are the same. Add it to `.gitignore`.

`--threads N` scans N files at once, one per CPU by default. `RAYON_NUM_THREADS` sets the same.

### What counts

With the default scope, `exports`, an item is anything a module exports:

- functions, classes, variables, interfaces, type aliases, enums and namespaces, whether exported where they're declared, with `export { a, b as c }`, with `export default a`, or with `export = a`
- `export default` of an expression
- CommonJS: `exports.a = …`, `module.exports.a = …`, `module.exports = …`, and each property of `module.exports = { … }`
- the public members of exported classes (methods, properties, accessors, with a getter and its setter as one) and interfaces (properties and methods)

An item is documented when a `/** … */` block with something in it sits right before it, or before its `export` or decorators. Ordinary comments, an empty `/** */`, license headers (`/** @license … */`) and a block whose description is just `TODO`, as `--fix` writes them, don't count.

`--require-description` (`"requireDescription": true`) also wants the block to describe the item: text before its first tag, or a `@description`, `@desc`, `@summary`, `@classdesc` (on a class) or `@inheritdoc` tag. Without it, `/** @type {string} */` counts. A block of `@inheritdoc` alone documents its item either way, and `"inherit": "override"` counts a TypeScript class member marked `override` as documented too, since it inherits its docs.

`--exclude-tag NAME` (repeatable) or `"excludeTags": ["internal", "ignore", "hidden"]` leaves out items whose JSDoc has one of those tags: they're out of the total, not documented. `--ignore-name REGEX` (repeatable) or `"ignore": { "names": ["^_"] }` does the same for items whose name matches, a member by its own name or as `Class.member`.

A `// docov-ignore-next-line` comment (with an optional reason, `// docov-ignore-next-line: legacy API`) takes the items on the line after it out of the total, and `/* docov-ignore-file */` anywhere at the top level does the same for the whole file. A documented item still counts as documented, as rustdoc does. Only the JSON report lists ignored items, marked with `"ignored": true` and their `reason`; the other formats leave them out. `--report-unused-ignores` (`"reportUnusedIgnores": true`) fails the scan on a `docov-ignore-next-line` whose items are documented anyway.

To adopt docov in a project that's far from documented, `--add-ignore` (or `--add-ignore=REASON`) writes a `// docov-ignore-next-line` above every undocumented item, indented like it, and reports how many it wrote instead of the coverage. It changes no other line.

`--fix` writes a JSDoc stub above every undocumented item that has no block yet, indented like it, for filling in: a `TODO` description, a `@template` for each type parameter, a `@param` for each parameter (a destructured one by its place, as `param0`), `@returns` when its body returns a value, and `@file` for a file's own item. A `TODO` description documents nothing, so the coverage stays where it was until each one is written. It reports how many stubs it wrote, leaves every existing block as it is, and skips an item that starts after something else on its line. It can't go with `--add-ignore`.

`--explain FILE` prints, in place of a report, a line for each item in `FILE` saying whether it counts by the config `FILE` is scanned with (a nested config's too) and why (documented by the block at a line, ignored by a comment, or left out by a rule), and one for each declaration or block the scan passed over there and why (a re-export, a constructor, an overload's implementation, a destructured variable, a `require()` binding, a license header, a stub, a private member). When a nested config leaves `FILE` out, it says so. It exits 0.

Some things never count: constructors (their class's doc covers them), private members (`#x` and `private x`), an overload's implementation (only its signatures), destructured variables, `require()` bindings, `declare module` and `declare global` blocks, and re-exports from another file (`export { x } from './x'`, `export * from`), whose declarations count in their own file.

`--scope all` counts every module-level declaration, exported or not.

`--scope public` counts only a package's public API: what its entry points export. It reads the package.json in the config's folder (a nested config's own, for its files), or else the nearest one above the scanned paths, takes every path in its `exports` (under every condition and subpath, leaving out `*` patterns), `main`, `module`, `types` and `typings`, and finds each one's source file, back through tsconfig.json's `outDir` and `rootDir` (or `src`) for a built file, and from a `.d.ts` to the module it declares. From there it follows `export * from`, `export { x } from`, `export * as ns from` and imports exported again through the module graph, the way Node and TypeScript resolve them, and counts each declaration it reaches once, in its own file. A reached file's CommonJS exports count too. An entry point with no source file is an error (exit 2).

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
"rules": { "require-param": "error", "param-names": "warn", "summary-sentence": ["warn", { "maxLength": 80 }] }
```

Each finding prints to stderr as `docov: path:line:column: rule-id: message`, and the `github`, `sarif`, `unix` and `markdown` formats list them too. A finding at `error` fails the scan (exit 1). A flag's level wins over the config's, which keeps its options, and a config's `rules` replaces the whole `rules` of one it extends or nests under. An item a `docov-ignore` comment takes out of the total isn't checked. The checks that want a tag leave a block with `@inheritdoc` alone.

- `require-param`: each parameter of a function or method has a matching `@param`, by name, or for a destructured one, at its place (as `paramsDocumented` counts them).
- `param-names`: each `@param` names a parameter, so none is left over from a rename. A destructured parameter takes the name of the `@param` at its place, and `@param options.a` belongs to `options`.
- `require-returns`: a function or method whose own body returns a value has `@returns`, unless its return type is `void`, `undefined` or `Promise<void>`.
- `require-yields`: a generator that yields a value has `@yields`.
- `require-throws`: an exported function or method with a `throw` of its own (outside a `try` that catches it) has `@throws`.
- `require-template`: each type parameter of a declaration has a `@template`.
- `empty-tag-description`: each `@param`, `@returns`, `@throws` and `@yields` says something past its type and name, except a `@returns` of nothing.
- `informative-docs`: a description says more than its item's name (`/** The user name. */` on `userName` doesn't).
- `valid-link`: each `{@link}`, `{@linkcode}` and `{@linkplain}` is closed, has a target and no unescaped `{`, and each `@see` has text. A target that's a name, or a namepath like `A#b` by its first part, is declared or imported in the same file or is a JavaScript, browser or Node.js global. Names in other files aren't looked up.
- `summary-sentence`: a description's first sentence is at most `maxLength` characters (120) and, with `period` (on by default), ends with `.`, `!` or `?`.
- `doc-markdown`: a description's words that look like code (`snake_case`, `a::b`, `camelCase`) are in backticks, and its lists have no lazy continuation lines. `allowlist` adds words to the default list (`JavaScript`, `GitHub` and others).

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

`paths` and `exclude` (gitignore-style globs) are relative to the config's folder. Comments and trailing commas are fine in any of the three.

`"extends": "../docov.json"` (or a list of paths) builds on other config files, relative to the one that names them. Its own keys win, and in a list a later file's win over an earlier one's. Each file's `paths`, `exclude`, `baseline` and the globs in its `threshold` and `overrides` stay relative to that file. Editors don't know `.docovrc` is JSON, so tell yours (in VS Code, `"files.associations": { ".docovrc": "jsonc" }`).

`--jsdoc-config FILE` (or `"jsdocConfig": "jsdoc.json"`, relative to the config) scans the files a [jsdoc config](https://jsdoc.app/about-configuring-jsdoc) documents: its `source.include` are the paths, unless there are paths already, and `source.exclude` leaves files and folders out, both relative to the jsdoc config. `source.includePattern` and `excludePattern` filter files only when the config writes them, since jsdoc's default `includePattern` would leave out TypeScript. It has to be JSON; docov can't read a `.js` config.

With `"nested": true` in the root config, each file is scanned with the nearest docov.json, docov.jsonc or .docovrc in a folder above it, up to the root config's folder, merged over the root config, the way [oxlint's nested configs](https://oxc.rs/docs/guide/usage/linter/nested-config.html) work. A nested config's `exclude` applies in its folder, its gates (`threshold`, `maxUndocumented`, `overrides`, `baseline`, the examples, release-tags, types and complete thresholds, and the doc checks' errors) hold for its own files (a failure names the config), and its `paths` aren't read.

`--by-package` adds a total line for each workspace package after the text total, and a `packages` array to the JSON. The packages are the folders with a package.json that the globs in the root config folder's package.json `workspaces` or pnpm-workspace.yaml `packages` match.

`--print-config` prints the settings a scan would use as JSON: the config files they came from, the paths, the excludes and each feature's setting, with the flags applied. `--list-files` prints the files it would scan. Neither scans.

`threshold` can also be an object of thresholds by path, such as `{ "global": 80, "perFile": 50, "src/api/**": 100 }`. `global` is for every file together (`--threshold` sets it), `perFile` for each file on its own, and any other key is a gitignore-style glob, relative to the config's folder, for the files it matches together. A negative number is the most undocumented items that pass instead of a percentage, as in [Jest](https://jestjs.io/docs/configuration#coveragethreshold-object): `-10` allows 10. Each threshold that fails prints its own line.

`maxUndocumented` (or `--max-undocumented N`) fails the scan when more than that many items are undocumented, next to any threshold.

`baseline` (or `--baseline FILE`) names a file of known undocumented items, so new code can be held to a gate while older code catches up. `docov --write-baseline` writes every undocumented item that isn't ignored to it (to `docov-baseline.json` in the config's folder when nothing names one) and exits 0. The gates count the items in it as documented, while the report still lists them. Items are kept by path, kind and name, so moving lines doesn't make them stale, and each path is relative to the folder of the config that names the baseline (the root config's for `--baseline`) with `/` on every OS. [`docov-baseline.schema.json`](docov-baseline.schema.json) (`docov --print-schema baseline`) is its JSON Schema.

`docov --update-threshold` ratchets the threshold: when the documented percentage, rounded down to a tenth, is over the config's global threshold, it writes it there (into `global` for an object of thresholds), changing only that number so comments and formatting stay. A negative threshold stays as it is. Without a config it writes a new `docov.json` where docov runs. With nested configs, it goes by the files the root config scans.

`overrides` changes settings for some paths: each has `files` (gitignore-style globs) and any of `exclude` and `threshold`, which replace the top-level ones for those files. For each file and setting, the last override that matches the file and has the setting wins. The top-level thresholds then cover only the files no override has a threshold for. As in .gitignore, a file in a folder that's left out stays out.

```jsonc
"overrides": [
  { "files": ["src/legacy/"], "exclude": [], "threshold": 20 },
]
```

[`docov.schema.json`](docov.schema.json) is its JSON Schema, which editors use for completion and checking. The URL above is pinned to a release so it matches the docov you have; `docov --print-schema > docov.schema.json` writes your own copy to point at instead.

### Formats

`--format` picks one, and can be given more than once: one scan feeds them all. `FORMAT=FILE` writes that one to `FILE`, making its folder if needed, and the rest go to stdout one after another. `--output FILE` writes the one format without a file of its own to `FILE`.

```bash
docov --format text --format lcov=coverage/docov.lcov
```

- `text` (the default): each file's percentage and its undocumented items, then the total. `--omit-covered` leaves out fully documented files, and `--quiet` prints only the total (failed gates still go to stderr). `--sort priority` lists each file's undocumented items by what to document first: exported functions and methods (more parameters first), then classes, then the rest. On a terminal, a percentage under the threshold is red and the undocumented items are dim. `--color always` or `never` says otherwise, and `NO_COLOR` turns it off.
- `json`: every item, documented or not, with its kind, line and column, and totals per file, per kind and overall. Each item also says whether its JSDoc describes it (`hasDescription`) and has an `@example` (`hasExample`), whether each parameter has a matching `@param` (`paramsDocumented`, `null` for what isn't a function or method), whether it's `deprecated`, and its `since` text (or `null`).
- `lcov`: a line record for each line with items on it, hit when all of them are documented, and each item as a function, hit when it's documented. Coverage services and editor gutters read it.
- `cobertura`: Cobertura XML (coverage-04.dtd) with the same line records, a package per folder and a class per file, for GitLab's merge request coverage and other CI coverage views.
- `github`: GitHub Actions annotations, a warning on each undocumented item, one at its level on each doc check finding, and an error for each failed gate. In GitHub Actions (`GITHUB_ACTIONS=true`) with no `--format`, docov writes these after the text, the way [oxlint](https://oxc.rs/docs/guide/usage/linter/output-formats.html) does.
- `markdown`: the total, any failed gates, a table of the files, and each file's undocumented items in a folded `<details>` block, then the doc checks' findings in one. Made for `$GITHUB_STEP_SUMMARY` (`--format markdown=$GITHUB_STEP_SUMMARY`) and pull request comments. `--compare BASE.json` takes an earlier `--format json` report (from the base branch, say) and adds the percentage before and the change, for the total and each file, with files marked `new` or `gone`.
- `sarif`: SARIF 2.1.0, a result at each undocumented item under the rule `docov/undocumented` and one at its level for each doc check finding under the check's own rule (`docov/require-param`), for GitHub code scanning (`github/codeql-action/upload-sarif`) and other SARIF viewers.
- `gitlab`: a GitLab Code Quality report, an issue of severity `minor` at each undocumented item. Each fingerprint is a hash of the item's path, kind and name, so GitLab tells new issues from old ones.
- `junit`: JUnit XML, a test suite per file and a test case per item that fails when the item is undocumented.
- `checkstyle`: Checkstyle XML, a warning at each undocumented item.
- `rdjson`: [reviewdog](https://github.com/reviewdog/reviewdog)'s diagnostic format (`reviewdog -f=rdjson`).
- `unix`: `path:line:column: kind name undocumented [docov]` for each undocumented item and `path:line:column: rule-id: message` for each doc check finding, the way compilers write warnings, for editors' problem matchers and grep.
- `html`: one self-contained page (`--format html=report.html`) with a table of the files and each file's source, its undocumented declarations highlighted. It has no scripts and follows the system's light or dark mode.
- `badge`: an SVG badge of the total, drawn without a network call. `--badge FILE` is short for `--format badge=FILE`.
- `shields`: the JSON a [shields.io endpoint badge](https://shields.io/badges/endpoint-badge) reads, for a badge from a file you publish.
- `benchmark`: the total as `[{"name":"docov","unit":"%","value":66.7}]`, for [github-action-benchmark](https://github.com/benchmark-action/github-action-benchmark)'s `customBiggerIsBetter`, to chart it over time.

Badges are red under the threshold, yellow up to 10 points over it, and green past that. With no threshold they're blue.

## CI

### GitHub Action

```yaml
permissions:
  contents: read
  pull-requests: write # for `comment`
steps:
  - uses: actions/checkout@v7
  - uses: cutenode/docov@vX.Y.Z # a release tag, or its commit SHA
    with:
      args: src --threshold 80
      comment: true
```

The action runs `npx @docov/cli` once (GitHub's runners have Node), with `--format github` for annotations and `--format markdown` for the report, and fails the step with docov's exit code.

| Input | Default | What it does |
| --- | --- | --- |
| `version` | the action's own, else `latest` | The docov version to run |
| `args` | | More docov arguments, quoted as in a shell |
| `summary` | `true` | Adds the report to the job summary |
| `comment` | `false` | Posts the report on the pull request and edits that comment on later runs. It needs `pull-requests: write`, and only warns without it (from a fork) |
| `compare` | | An earlier `--format json` report to show the change from |
| `data-branch` | | A branch for the reports from the default branch, below |
| `token` | `github.token` | The token `comment` and `data-branch` use |

The `markdown` output is the report's path.

With `data-branch: docov-data`, each push to the default branch commits `docov.json` (the json report) and `shields.json` (the `shields` format) to that branch as github-actions[bot], so its history is the trend. It needs `contents: write`, and a `concurrency` group keeps two pushes from racing. Pull requests then compare with its `docov.json` unless `compare` names another report. In a public repository a badge can read the branch's raw file:

```markdown
![docs](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/OWNER/REPO/docov-data/shields.json)
```

### pre-commit and lefthook

```yaml
# .pre-commit-config.yaml
repos:
  - repo: https://github.com/cutenode/docov
    rev: vX.Y.Z
    hooks:
      - id: docov
        args: [--quiet]
```

The hook installs `@docov/cli` in pre-commit's own Node environment, so it needs nothing installed and runs on pre-commit.ci. It scans the whole project when a script or docov's config is staged. With `@docov/cli` in your devDependencies, [lefthook](https://lefthook.dev) runs that one:

```yaml
# lefthook.yml
pre-commit:
  commands:
    docov:
      glob: "*.{js,mjs,cjs,jsx,ts,mts,cts,tsx}"
      run: npx docov --quiet
```

### Codecov

Upload docov's lcov under its own flag, and carry the flag forward so uploads without it keep its last numbers:

```yaml
- run: npx docov --format lcov=docov.lcov
- uses: codecov/codecov-action@v5
  with:
    files: docov.lcov
    flags: docs # -F docs with Codecov's CLI
```

```yaml
# codecov.yml
flag_management:
  individual_flags:
    - name: docs
      carryforward: true
      statuses:
        - type: project
          target: 80%
```

Codecov [merges every upload](https://docs.codecov.com/docs/merging-reports) for a commit, so if you upload test coverage too, the repository's headline number mixes the two on the same lines: an undocumented function your tests cover counts as covered. The `docs` flag's own status and numbers are the doc coverage alone.

## Working on it

The toolchain is pinned in `rust-toolchain.toml`, and Node in `.node-version`. The checks need [cargo-nextest](https://nexte.st) and [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov).

Each flag and docov.json key belongs to a feature, one file in `src/feature/` that implements `Feature`. Each output format is one file in `src/report/` with a `FORMAT` naming it, describing it for `--help`, and rendering it. Each doc check is one file in `src/check/` that implements `Check`, its rule id the file name in kebab case. `build.rs` registers every file in the three folders, so a new feature, format or check is a new file and nothing else.

```bash
cargo run -- [PATHS]...   # docov on a project
tools/check.sh            # what CI runs: tools/lint.sh (format, clippy) and tools/coverage.sh (coverage for the whole suite, for the Hegel properties alone, and for the npm launcher's tests)
tools/jsdoc-oracle.sh [DIR]   # where docov and jsdoc disagree on a folder of plain JavaScript (default: tests/fixtures/jsdoc)
git tag v1.2.3 && git push origin v1.2.3   # release Cargo.toml's version 1.2.3 to crates.io and npm
```

Pushing a version tag runs `.github/workflows/release.yml`, which builds the binaries and publishes with trusted publishing, so there are no registry tokens. The npm packages are `@docov/cli` (`npm/docov`, a launcher) and an `@docov/<platform>-<arch>` package per binary. `tools/bootstrap-publishing.sh` publishes the first version by hand and sets up the trust, which both registries only allow for a package they already have.

`cargo coverage` fails under the thresholds in `.cargo/config.toml`. On a pull request, lint and coverage are separate pass/fail checks for each OS. With `LCOV_DIR` set, `tools/coverage.sh` writes each Rust coverage run to `$LCOV_DIR/suite.lcov` and `props.lcov` in place of its table. CI sets it, puts each run's per-file numbers in the job summary, and uploads the lcov files as the `coverage-<os>` artifact. The property tests (`tests/props.rs`) use [Hegel](https://github.com/hegeldev/hegel-rust). They check scans against a model of generated modules, and the binary against generated projects.

`tools/jsdoc-oracle.sh` runs jsdoc 4 (`npx --yes jsdoc@4 -X`, so it needs Node and the network) and `docov --scope all --format json` on the same files, and lists each declaration only one of them counts, or that they disagree on documenting. It isn't part of the checks: some disagreements are by design (jsdoc counts constructors, `this.x =` members, object literal members, destructured variables and `/** */` and `@license` blocks), and a `@typedef` or `@file` block right before an undocumented declaration documents it for docov but not for jsdoc unless `"virtual"` or `"fileDocs"` is on.
