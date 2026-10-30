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
docov [PATHS]... [--config FILE] [--threshold PERCENT] [--scope exports|all] [--format FORMAT[=FILE]]... [--output FILE] [--badge FILE] [--compare FILE] [--omit-covered] [--quiet]
```

With no paths it scans the current folder: every `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.mts`, `.cts` and `.tsx` file, leaving out hidden files and what `.gitignore` leaves out (in a git repository or not). A file named on the command line is always scanned.

It exits 0 when every gate passes (or there isn't one), 1 when one fails, such as coverage under a threshold, and 2 when a file doesn't parse or the config or a path can't be read. Every file has to parse, since a partial one would undercount.

`--changed` scans only the files that differ from where HEAD branched off the default branch (`origin/HEAD`, or `main` without one), committed or not, and untracked ones. `--since REF` compares with another branch or commit instead. `--staged` scans only the staged files, for a pre-commit hook. Paths given on the command line still narrow the scan.

`--diff-threshold PERCENT` fails the scan when the items on lines a change added (committed or not, and every line of an untracked file) are under that documented percentage, wherever the rest of the project is. The change is from where HEAD branched off the default branch, or off `--diff-base REF`. These run `git` (2.30 or later).

### What counts

With the default scope, `exports`, an item is anything a module exports:

- functions, classes, variables, interfaces, type aliases, enums and namespaces, whether exported where they're declared, with `export { a, b as c }`, with `export default a`, or with `export = a`
- `export default` of an expression
- CommonJS: `exports.a = …`, `module.exports.a = …`, `module.exports = …`, and each property of `module.exports = { … }`
- the public members of exported classes (methods, properties, accessors, with a getter and its setter as one) and interfaces (properties and methods)

An item is documented when a `/** … */` block with something in it sits right before it, or before its `export` or decorators. Ordinary comments, an empty `/** */`, and license headers (`/** @license … */`) don't count.

`--require-description` (`"requireDescription": true`) also wants the block to describe the item: text before its first tag, or a `@description`, `@desc`, `@summary`, `@classdesc` (on a class) or `@inheritdoc` tag. Without it, `/** @type {string} */` counts. A block of `@inheritdoc` alone documents its item either way, and `"inherit": "override"` counts a TypeScript class member marked `override` as documented too, since it inherits its docs.

`--exclude-tag NAME` (repeatable) or `"excludeTags": ["internal", "ignore", "hidden"]` leaves out items whose JSDoc has one of those tags: they're out of the total, not documented. `--ignore-name REGEX` (repeatable) or `"ignore": { "names": ["^_"] }` does the same for items whose name matches, a member by its own name or as `Class.member`.

A `// docov-ignore-next-line` comment (with an optional reason, `// docov-ignore-next-line: legacy API`) takes the items on the line after it out of the total, and `/* docov-ignore-file */` anywhere at the top level does the same for the whole file. A documented item still counts as documented, as rustdoc does. Only the JSON report lists ignored items, marked with `"ignored": true` and their `reason`; the other formats leave them out. `--report-unused-ignores` (`"reportUnusedIgnores": true`) fails the scan on a `docov-ignore-next-line` whose items are documented anyway.

To adopt docov in a project that's far from documented, `--add-ignore` (or `--add-ignore=REASON`) writes a `// docov-ignore-next-line` above every undocumented item, indented like it, and reports how many it wrote instead of the coverage. It changes no other line.

Some things never count: constructors (their class's doc covers them), private members (`#x` and `private x`), an overload's implementation (only its signatures), destructured variables, `require()` bindings, `declare module` and `declare global` blocks, and re-exports from another file (`export { x } from './x'`, `export * from`), whose declarations count in their own file.

`--scope all` counts every module-level declaration, exported or not.

`"exports": { "esm": true, "cjs": true, "globals": false }` picks which ways of exporting count under the `exports` scope (`cjs` covers TypeScript's `export =` too). With `globals` on, `window.a = …` and `globalThis.a = …` at the top level export `a` the way `exports.a = …` does, under either scope.

`--kind function,class` (or `"kinds": ["function", "class"]`) counts only those kinds of item, named as the reports name them: `function`, `class`, `method`, `accessor`, `property`, `variable`, `interface`, `type`, `enum`, `namespace` and `default`. `--by-kind` adds a line of totals by kind after the text report's total, and the JSON report always has them, under `kinds`.

`--access public,protected` (or `"access": ["public", "protected", "package"]`) counts only items with those access levels, like jsdoc's `--access`: an item's level is its `@public`, `@protected`, `@package` or `@private` tag (or `@access` with one of those), else `protected` for a TypeScript `protected` member, else `public`. Without it, access tags change nothing.

`"ignoreDeprecated": true` leaves out items whose JSDoc has `@deprecated`, and `"skipGenerated": true` leaves out every item in a file whose first comment has `@generated`.

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

`threshold` can also be an object of thresholds by path, such as `{ "global": 80, "perFile": 50, "src/api/**": 100 }`. `global` is for every file together (`--threshold` sets it), `perFile` for each file on its own, and any other key is a gitignore-style glob, relative to the config's folder, for the files it matches together. A negative number is the most undocumented items that pass instead of a percentage, as in [Jest](https://jestjs.io/docs/configuration#coveragethreshold-object): `-10` allows 10. Each threshold that fails prints its own line.

`maxUndocumented` (or `--max-undocumented N`) fails the scan when more than that many items are undocumented, next to any threshold.

`baseline` (or `--baseline FILE`) names a file of known undocumented items, so new code can be held to a gate while older code catches up. `docov --write-baseline` writes every undocumented item that isn't ignored to it (to `docov-baseline.json` in the config's folder when nothing names one) and exits 0. The gates count the items in it as documented, while the report still lists them. Items are kept by path, kind and name, so moving lines doesn't make them stale, and each path is relative to the config's folder with `/` on every OS. [`docov-baseline.schema.json`](docov-baseline.schema.json) (`docov --print-schema baseline`) is its JSON Schema.

`docov --update-threshold` ratchets the threshold: when the documented percentage, rounded down to a tenth, is over the config's global threshold, it writes it there (into `global` for an object of thresholds), changing only that number so comments and formatting stay. A negative threshold stays as it is. Without a config it writes a new `docov.json` where docov runs.

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

- `text` (the default): each file's percentage and its undocumented items, then the total. `--omit-covered` leaves out fully documented files, and `--quiet` prints only the total (failed gates still go to stderr).
- `json`: every item, documented or not, with its kind, line and column, and totals per file, per kind and overall.
- `lcov`: a line record for each line with items on it, hit when all of them are documented, and each item as a function, hit when it's documented. Coverage services and editor gutters read it.
- `cobertura`: Cobertura XML (coverage-04.dtd) with the same line records, a package per folder and a class per file, for GitLab's merge request coverage and other CI coverage views.
- `github`: GitHub Actions annotations, a warning on each undocumented item and an error for each failed gate. In GitHub Actions (`GITHUB_ACTIONS=true`) with no `--format`, docov writes these after the text, the way [oxlint](https://oxc.rs/docs/guide/usage/linter/output-formats.html) does.
- `markdown`: the total, any failed gates, a table of the files, and each file's undocumented items in a folded `<details>` block. Made for `$GITHUB_STEP_SUMMARY` (`--format markdown=$GITHUB_STEP_SUMMARY`) and pull request comments. `--compare BASE.json` takes an earlier `--format json` report (from the base branch, say) and adds the percentage before and the change, for the total and each file, with files marked `new` or `gone`.
- `sarif`: SARIF 2.1.0 with one rule, `docov/undocumented`, and a result at each undocumented item, for GitHub code scanning (`github/codeql-action/upload-sarif`) and other SARIF viewers.
- `gitlab`: a GitLab Code Quality report, an issue of severity `minor` at each undocumented item. Each fingerprint is a hash of the item's path, kind and name, so GitLab tells new issues from old ones.
- `junit`: JUnit XML, a test suite per file and a test case per item that fails when the item is undocumented.
- `checkstyle`: Checkstyle XML, a warning at each undocumented item.
- `rdjson`: [reviewdog](https://github.com/reviewdog/reviewdog)'s diagnostic format (`reviewdog -f=rdjson`).
- `unix`: `path:line:column: kind name undocumented [docov]` for each undocumented item, the way compilers write warnings, for editors' problem matchers and grep.
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

Each flag and docov.json key belongs to a feature, one file in `src/feature/` that implements `Feature`. Each output format is one file in `src/report/` with a `FORMAT` naming it, describing it for `--help`, and rendering it. `build.rs` registers every file in both folders, so a new feature or format is a new file and nothing else.

```bash
cargo run -- [PATHS]...   # docov on a project
tools/check.sh            # what CI runs: tools/lint.sh (format, clippy) and tools/coverage.sh (coverage for the whole suite, for the Hegel properties alone, and for the npm launcher's tests)
git tag v1.2.3 && git push origin v1.2.3   # release Cargo.toml's version 1.2.3 to crates.io and npm
```

Pushing a version tag runs `.github/workflows/release.yml`, which builds the binaries and publishes with trusted publishing, so there are no registry tokens. The npm packages are `@docov/cli` (`npm/docov`, a launcher) and an `@docov/<platform>-<arch>` package per binary. `tools/bootstrap-publishing.sh` publishes the first version by hand and sets up the trust, which both registries only allow for a package they already have.

`cargo coverage` fails under the thresholds in `.cargo/config.toml`. On a pull request, lint and coverage are separate pass/fail checks for each OS. With `LCOV_DIR` set, `tools/coverage.sh` writes each Rust coverage run to `$LCOV_DIR/suite.lcov` and `props.lcov` in place of its table. CI sets it, puts each run's per-file numbers in the job summary, and uploads the lcov files as the `coverage-<os>` artifact. The property tests (`tests/props.rs`) use [Hegel](https://github.com/hegeldev/hegel-rust). They check scans against a model of generated modules, and the binary against generated projects.
