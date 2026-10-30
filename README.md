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

`paths` and `exclude` (gitignore-style globs) are relative to the config's folder. Comments and trailing commas are fine in any of the three. Editors don't know `.docovrc` is JSON, so tell yours (in VS Code, `"files.associations": { ".docovrc": "jsonc" }`).

[`docov.schema.json`](docov.schema.json) is its JSON Schema, which editors use for completion and checking. The URL above is pinned to a release so it matches the docov you have; `docov --print-schema > docov.schema.json` writes your own copy to point at instead.

### Formats

`--format` picks one, and can be given more than once: one scan feeds them all. `FORMAT=FILE` writes that one to `FILE`, making its folder if needed, and the rest go to stdout one after another. `--output FILE` writes the one format without a file of its own to `FILE`.

```bash
docov --format text --format lcov=coverage/docov.lcov
```

- `text` (the default): each file's percentage and its undocumented items, then the total. `--omit-covered` leaves out fully documented files, and `--quiet` prints only the total (failed gates still go to stderr).
- `json`: every item, documented or not, with its kind, line and column, and totals per file and overall.
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
