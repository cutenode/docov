# docov

docov measures how much of a JavaScript or TypeScript project's API has doc comments, and lists each declaration that has none. It's for projects that want to hold that number in CI the way they hold test coverage, and to see what to document next. It reads JSDoc and TSDoc, and parses with [oxc](https://oxc.rs), so TypeScript, JSX, ES modules and CommonJS need no build step.

## Install

| Channel | Install | Notes |
| --- | --- | --- |
| npm | `npm install --save-dev @docov/cli` (or pnpm, yarn, bun) | Runs a prebuilt binary for macOS, Linux or Windows on x64 or arm64, from the `@docov/<platform>-<arch>` package npm installs with it. `DOCOV_BINARY` runs a different one. |
| Cargo | `cargo install docov` | Builds from source. |
| GitHub Action | `uses: cutenode/docov@vX.Y.Z` | Annotations, a job summary and a pull request comment. See [Fail CI under a threshold](#fail-ci-under-a-threshold). |
| pre-commit | `repo: https://github.com/cutenode/docov` | Installs the npm package in pre-commit's own Node environment. See [Check before each commit](#check-before-each-commit). |

## Quickstart

Run docov in a project's folder. With no config, it scans every JavaScript and TypeScript file there and reports each file's coverage, its undocumented items, and the total. These examples run on [tests/fixtures/quickstart](tests/fixtures/quickstart), a tiny project, and the output under each command is what docov printed, which a test keeps current.

```console
$ docov
  57.1%  4/7  src/cart.ts
         src/cart.ts:5:3  property Item.quantity
         src/cart.ts:18:1  class Cart
         src/cart.ts:26:3  accessor Cart.size
  50.0%  1/2  src/format.js
         src/format.js:11:1  variable CURRENCY
  55.6%  5/9  total
```

Each undocumented item is listed as `path:line:column`, the spot its doc comment goes, which terminals and editors open directly, then its kind and name. A threshold turns the number into a gate:

```console
$ docov --threshold 80
  57.1%  4/7  src/cart.ts
         src/cart.ts:5:3  property Item.quantity
         src/cart.ts:18:1  class Cart
         src/cart.ts:26:3  accessor Cart.size
  50.0%  1/2  src/format.js
         src/format.js:11:1  variable CURRENCY
  55.6%  5/9  total
docov: 55.6% documented, under the 80% threshold
```

docov exits 0 when every gate passes (or there isn't one), 1 when one fails, and 2 when a file doesn't parse or the config or a path can't be read. Every file has to parse, since a partial one would undercount.

To see why an item counts or doesn't, ask about its file:

```console
$ docov --explain src/cart.ts
src/cart.ts:2:1  interface Item  documented by the block at 1:1
src/cart.ts:4:3  property Item.sku  documented by the block at 3:3
src/cart.ts:5:3  property Item.quantity  undocumented
src/cart.ts:14:1  function total  documented by the block at 8:1
src/cart.ts:18:1  class Cart  undocumented
src/cart.ts:19:3  skipped Cart.#items: private
src/cart.ts:22:3  method Cart.add  documented by the block at 21:3
src/cart.ts:26:3  accessor Cart.size  undocumented
src/cart.ts:32:1  variable legacyTotal  ignored by the docov-ignore-next-line at 31:1 (kept for old callers)
```

`Cart.#items` is private and `legacyTotal` is ignored, so the file has 7 items, not 9. `--explain` prints this in place of a report and exits 0. It goes by the config the file is scanned with (a nested config's too), says so when a config leaves the file out, and also lists each ignore comment there that changes nothing and each declaration or block the scan passed over, with why.

To keep the number from falling, `docov init` writes a `docov.json` with the `$schema` for this version and a `threshold` of the folder's coverage now, rounded down to a whole percent. It scans the folder with the default settings, and won't write over a config that's there. (To scan a folder named `init`, use `docov ./init`.)

```console
$ docov init
docov: wrote docov.json, with a 55% threshold
$ docov --quiet
  55.6%  5/9  total
```

`docov --help` lists every flag, and [Reference](#reference) has the rest.

## How coverage is counted

Coverage is documented items over counted items. The rules below decide both sides, and `docov --explain FILE` says which rule decided each item in a file, so any number docov prints can be checked by hand.

### Files

With no paths, docov scans the folder it runs in (or the config's `paths`): every `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.mts`, `.cts` and `.tsx` file, leaving out hidden files, what `.gitignore` and `.ignore` files leave out (in a git repository or not), and the config's `exclude` globs. A file named on the command line is always scanned. `docov --list-files` prints the files a scan would read, and `--print-config` the settings it would use (the config files they came from, the paths, the excludes and each feature's setting, with the flags applied). Neither scans.

`--jsdoc-config FILE` (or `"jsdocConfig"`) scans the files a [jsdoc config](https://jsdoc.app/about-configuring-jsdoc) documents: its `source.include` are the paths, unless there are paths already, and `source.exclude` leaves files and folders out, both relative to the jsdoc config. `source.includePattern` and `excludePattern` filter files only when the config writes them, since jsdoc's default `includePattern` would leave out TypeScript. It has to be JSON, since docov can't read a `.js` config.

### Items

An item is a declaration that's part of the API. With the default scope, `exports`, that's anything a module exports:

- functions, classes, variables, interfaces, type aliases, enums and namespaces, exported where they're declared, with `export { a, b as c }`, with `export default a`, or with `export = a`
- `export default` of an expression
- CommonJS: `exports.a = …`, `module.exports.a = …`, `module.exports = …`, and each property of `module.exports = { … }`
- the public members of exported classes (methods, properties, and accessors, a getter and its setter as one) and interfaces (properties and methods)

Some declarations never count, each for a reason:

| Not counted | Why |
| --- | --- |
| Constructors | Their class's doc covers them. |
| Private members (`#x`, `private x`) | They aren't API. |
| An overload's implementation | Its signatures count instead. |
| Destructured variables (`const { a } = b`) | There's nowhere for a doc of their own. |
| `require()` bindings | They're imports. |
| `declare module` and `declare global` blocks | They declare nothing of the module's own. |
| Re-exports (`export { x } from './x'`, `export * from`) | The declaration counts once, in its own file. |

The scope changes which declarations count:

| Scope | Counts |
| --- | --- |
| `exports` (default) | What each module exports, as above. |
| `all` | Every module-level declaration, exported or not. |
| `public` | Only what the package's entry points export. |

`--scope public` reads the package.json in the config's folder (a nested config's own, for its files), or else the nearest one above the scanned paths. It takes every path in `exports` (under every condition and subpath, leaving out `*` patterns), `main`, `module`, `types` and `typings`, and finds each one's source file, back through tsconfig.json's `outDir` and `rootDir` (or `src`) for a built file, and from a `.d.ts` to the module it declares. From there it follows `export * from`, `export { x } from`, `export * as ns from` and imports exported again, the way Node and TypeScript resolve them, and counts each declaration it reaches once, in its own file. A reached file's CommonJS exports count too. An entry point with no source file is an error (exit 2).

More settings add items or narrow them, each in [docs/config.md](docs/config.md):

| Setting | Effect |
| --- | --- |
| `"members": "all"` | Also counts, where their parent counts, enum members (`Color.Red`), the members of a type alias's object type (`Options.timeout`), and the properties of an object literal whose block has `@enum`, `@namespace` or `@lends X` (`Name.prop`, or `X.prop`). A matching `@property` in the parent's block documents one. The default, `"classes"`, counts only the members of classes and interfaces. |
| `"commonjsMembers": true` | Counts `X.prototype.m = …` and `X.m = …` at the top level as members of `X`, a function or class declared in the file, where `X` counts. In a CommonJS file (`.cjs`, or `.js` with no `import` or `export`), `this.a = …` at the top level exports `a`. |
| `"fileDocs": "require"` | Makes each file an item of kind `module`, at its first line and named by its `@module` or else its path. It's documented when the file's first block has `@file`, `@fileoverview`, `@overview`, `@module` or `@packageDocumentation`, with text, and that block then documents nothing else. The first `// docov-ignore-next-line` before the file's code takes the file's item out too. |
| `"virtual": true` | Counts a JavaScript file's `@typedef` and `@callback` blocks that name a type as items of kind `type`, at the block, documented when the block or the tag describes the type. They count as exported under the `exports` scope, since JSDoc treats them as the module's public types, and then document nothing else. |
| `"exports": { "esm", "cjs", "globals" }` | Picks which ways of exporting make an item exported. With `globals`, `window.a = …` and `globalThis.a = …` at the top level export `a` the way `exports.a = …` does, under the `exports` and `all` scopes. |
| `--kind function,class` or `kinds` | Counts only those kinds, named as the reports name them: `function`, `class`, `method`, `accessor`, `property`, `variable`, `interface`, `type`, `enum`, `namespace`, `default` and `module`. |
| `--access public,protected` or `access` | Counts only those access levels, like jsdoc's `--access`: an item's `@public`, `@protected`, `@package` or `@private` tag (or `@access`), else `protected` for a TypeScript `protected` member, else `public`. |

Without `fileDocs`, a file's first `@file` block right before a declaration documents the declaration, and without `virtual`, so does a `@typedef` block. A later `@file` block, or a `@typedef` block in TypeScript or one that names no type, documents the next declaration either way.

### Documented

An item is documented when a `/** … */` block with something in it sits right before it, or before its `export` or decorators. With several, the nearest is the item's, as in jsdoc. These don't count:

- ordinary comments (`//` and `/* */`)
- an empty `/** */`
- license headers (`/** @license … */`), which are file headers
- a block whose description is just `TODO`, as `--fix` writes stubs, so a stub nobody filled in doesn't raise the number

A block of tags alone, such as `/** @type {string} */`, counts. `--require-description` (`"requireDescription": true`) asks for more: text before the first tag, or a `@description`, `@desc`, `@summary`, `@classdesc` (on a class) or `@inheritdoc` tag.

An item can inherit its docs. A block with `@inheritdoc`, or TSDoc's inline `{@inheritDoc}` (or `{@inheritDoc Ref}`) anywhere in it, documents its item, with or without a description. `"inherit": "override"` also counts a TypeScript class member marked `override` as documented, since it inherits its base's docs.

### Leaving items out

These take items out of the total, so they're neither documented nor undocumented:

| How | Leaves out |
| --- | --- |
| `// docov-ignore-next-line` (or with a reason, `// docov-ignore-next-line: legacy API`) | The items on the next line. |
| `/* docov-ignore-file */` at the top level | Every item in the file. |
| `--exclude-tag internal` or `"excludeTags": ["internal"]` | Items whose block has that tag. |
| `--ignore-name '^_'` or `"ignore": { "names": ["^_"] }` | Items whose name matches the regex, a member by its own name or as `Class.member`. |
| `"ignoreDeprecated": true` | Items whose block has `@deprecated`. |
| `"skipGenerated": true` | Every item in a file whose first comment has `@generated`. |

A documented item under an ignore comment still counts as documented, as rustdoc does it, so ignoring never hides docs that are there. Only the `json` report lists ignored items, with `"ignored": true` and their `reason`. `--report-unused-ignores` (`"reportUnusedIgnores": true`) fails the scan on each ignore comment that changes nothing: one over a blank line, over a statement that declares nothing or an item the config leaves out, over items that are documented anyway, or one that another comment covers too (of those, one is kept). In a file `skipGenerated` skips, none counts as unused.

A [baseline](#ratchet-coverage-up) is different: its items stay in the total and in the report, and only the gates count them as documented.

### Other figures

Coverage is the headline number. Four second figures each have a share, a gate, and fields in the `json` report:

| Figure | The share of | Text report | Gate | `json` fields |
| --- | --- | --- | --- | --- |
| Examples | counted items with an `@example`, as rustdoc's `--show-coverage` counts it | `--examples`, on each file's line and the total's | `--examples-threshold`, `examplesThreshold` | `withExamples`, `examplesPercent`, always |
| Complete | counted items documented with a matching `@param` for each parameter | `--complete`, a line after the total | `--complete-threshold`, `completeThreshold` | `complete`, `completePercent` |
| Types | counted JavaScript functions and methods (in `.js`, `.mjs`, `.cjs` and `.jsx` files) whose `@param`s, and `@returns` if any, have a `{type}` | `--types`, a line after the total | `--types-threshold`, `typesThreshold` | `typed`, `typesPercent` |
| Release tags | counted top-level exports with exactly one of `@public`, `@beta`, `@alpha` and `@internal`, [as API Extractor asks](https://api-extractor.com/pages/messages/ae-missing-release-tag/). Members take their container's, so they aren't counted. | `--release-tags`, a line after the total | `--release-tags-threshold`, `releaseTagsThreshold` | `releaseTagged`, `releaseTagsPercent` |

The complete, types and release-tags gates turn their figure on too; the examples gate doesn't add it to the text report. An undocumented function isn't typed. `--by-kind` adds a line of totals by kind after the text report's total, which the `json` report always has under `kinds`.

[Doc checks](docs/checks.md) go past whether a block exists to what it says (a `@param` for each parameter, a summary that ends with a period). They're findings, not part of the percentage.

## Recipes

### Fail CI under a threshold

In GitHub Actions, the Action runs docov once with annotations on each undocumented item, adds the report to the job summary, and can keep one comment on the pull request up to date:

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

It runs `npx @docov/cli` (GitHub's runners have Node) with `--format github` for annotations and `--format markdown` for the report, and fails the step with docov's exit code.

| Input | Default | What it does |
| --- | --- | --- |
| `version` | the action's own, else `latest` | The docov version to run |
| `args` | | More docov arguments, quoted as in a shell |
| `summary` | `true` | Adds the report to the job summary |
| `comment` | `false` | Posts the report on the pull request and edits that comment on later runs. It needs `pull-requests: write`, and only warns without it (from a fork) |
| `compare` | | An earlier `--format json` report to show the change from |
| `data-branch` | | A branch for the reports from the default branch, below |
| `token` | `github.token` | The token `comment` and `data-branch` use |

Its `markdown` output is the report's path. With `data-branch: docov-data`, each push to the default branch commits `docov.json` (the `json` report) and `shields.json` (the `shields` format) to that branch as github-actions[bot], so its history is the trend. That needs `contents: write`, and a `concurrency` group on the workflow keeps two pushes from racing. Pull requests then compare with its `docov.json` unless `compare` names another report. In a public repository, a badge can read the branch's file:

```markdown
![docs](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/OWNER/REPO/docov-data/shields.json)
```

Anywhere else, run docov and let the exit code fail the job: `npx docov` with `@docov/cli` in your devDependencies, or `npx @docov/cli` without. GitLab reads `--format gitlab=gl-code-quality.json` and `--format cobertura=coverage.xml`; [docs/formats.md](docs/formats.md) has every format with a sample.

A threshold can be more than one number. In docov.json, an object sets one for every file together (`global`, which `--threshold` sets), one for each file on its own (`perFile`), and one for each glob's files together. A negative number is the most undocumented items that pass (`-10` allows 10), as in [Jest](https://jestjs.io/docs/configuration#coveragethreshold-object), and `maxUndocumented` (`--max-undocumented N`) does the same next to a percentage:

```jsonc
{
  "$schema": "https://raw.githubusercontent.com/cutenode/docov/v0.1.1/docov.schema.json",
  "paths": ["src"],
  "exclude": ["**/*.test.ts"],
  "threshold": { "global": 80, "perFile": 50, "src/api/**": 100, "src/legacy/**": -10 },
}
```

Each threshold that fails prints its own line.

### Ratchet coverage up

In a project that's far from documented, hold new code to a gate while older code catches up. `docov --write-baseline` writes every undocumented item that isn't ignored to `docov-baseline.json` in the config's folder (or the file `baseline` or `--baseline` names) and exits 0:

```sh
docov --write-baseline
```

Set `"baseline": "docov-baseline.json"` in docov.json. The gates then count those items as documented, while the report and its total still list them as they are. Items are kept by path, kind and name, so moving lines doesn't make them stale. Each path is relative to the folder of the config that names the baseline (the root config's for `--baseline`), with `/` on every OS. [`docov-baseline.schema.json`](docov-baseline.schema.json) is its schema.

As coverage rises, `docov --update-threshold` raises docov.json's global threshold to the documented percentage, rounded down to a tenth, when that's higher. It changes only that number, so comments and formatting stay, and leaves a negative threshold as it is. Without a config, it writes a new `docov.json` where docov runs. Run it on the default branch, or after merging, and commit the change.

`docov --add-ignore` (or `--add-ignore=REASON`) is the blunter tool: it writes a `// docov-ignore-next-line` above every undocumented item, indented like it, and reports how many in place of the coverage. A file's own item gets one at the top, below a shebang, unless one above the file's first line of code covers it. It changes no other line. Those items then leave the total, which a baseline doesn't do.

### Cover only what changed

`--changed` scans only the files that differ from where HEAD branched off the default branch (`origin/HEAD`, or `main` without one), committed or not, and untracked ones. `--since REF` compares with another branch or commit. `--staged` scans only the staged files, for a pre-commit hook. Paths on the command line still narrow the scan.

`--diff-threshold PERCENT` gates by line instead: it fails when the items on lines a change added (committed or not, and every line of an untracked file) are under that documented percentage, wherever the rest of the project is. The change is from where HEAD branched off the default branch, or off `--diff-base REF`. These run `git` (2.30 or later). A CI checkout may have neither `origin/HEAD` nor `main`, so there, fetch the history (`fetch-depth: 0` in `actions/checkout`) and name the base:

```sh
docov --diff-threshold 100 --diff-base origin/main   # every item the change adds is documented
```

### Set it up in a monorepo

`--by-package` adds a total for each workspace package after the text report's total, and a `packages` array to the `json` report. The packages are the folders with a package.json that the globs in the package.json `workspaces`, or pnpm-workspace.yaml `packages`, of the root config's folder match.

For settings per package, put `"nested": true` in the root config. Each file is then scanned with the nearest docov.json, docov.jsonc or .docovrc above it, up to the root config's folder, merged over the root config, the way [oxlint's nested configs](https://oxc.rs/docs/guide/usage/linter/nested-config.html) work:

```jsonc
// docov.json
{ "nested": true, "threshold": 80 }
```

```jsonc
// packages/legacy/docov.json
{ "threshold": 40, "exclude": ["src/generated/**"] }
```

A nested config's `exclude` applies in its folder, and its `overrides` (in place of the root's) set its own files' scope. Its gates (`threshold`, `maxUndocumented`, `overrides`, `baseline`, the other figures' thresholds and the doc checks' errors) hold for its own files, and a failure names the config. Its `paths` aren't read. `--update-threshold` goes by the files the root config scans.

To share settings without nesting, `"extends": "../../docov.json"` (or a list) builds on other config files, relative to the one that names them. Its own keys win (`rules` by rule id), and a later file in the list wins over an earlier one. Each file's `paths`, `exclude`, `baseline` and the globs in its `threshold` and `overrides` stay relative to that file.

`overrides` changes `exclude`, `threshold` and `scope` for the files its globs match, the last matching override with the setting winning. The top-level thresholds then cover only the files no override has a threshold for. As in .gitignore, a file in a folder that's left out stays out. `--scope` wins over every override's `scope`, and `--explain` names the override that set a file's scope.

```jsonc
"overrides": [
  { "files": ["src/legacy/"], "threshold": 20 },
  { "files": ["src/internal/"], "scope": "all" },
]
```

`--print-config` prints the settings a scan would use, with the config files they came from.

### Fill gaps with `--fix`

`docov --fix` writes a doc stub above every undocumented item that has no block yet, indented like it, prefilled from the code: a `TODO` description, a `@template` for each type parameter, a `@param` for each parameter (a destructured one by its place, as `param0`), `@returns` when its body returns a value, and `@file` for a file's own item. It reports how many it wrote, leaves every existing block as it is, and skips an item that starts after something else on its line. It can't go with `--add-ignore`.

```console
$ docov --fix
docov: added 4 doc stubs
$ docov --quiet
  55.6%  5/9  total
```

A `TODO` description documents nothing, so the coverage stays where it was until each one is written. `"syntax": "tsdoc"` writes TSDoc's tags instead: `@typeParam T -`, `@param x -` and `@packageDocumentation`. It also makes `require-template` ask for `@typeParam`, and `valid-link` read the `A` in `{@link A#b}` as a package's name, which it doesn't look up. The scan and the other checks read both syntaxes the same.

### Use it in an editor

`docov lsp` is a language server on stdin and stdout. For each open JavaScript or TypeScript file, it shows an information diagnostic at each undocumented item (`docov/undocumented`) and each doc check finding at its level, from the unsaved text, by the config `docov` would use for that file (nested configs included). On an undocumented item, a code action writes the stub `--fix` would. To stub every file at once, run `docov --fix`.

It finds the config as if it ran in the workspace folder the file is in, or else in the file's own folder, and shows nothing for a file the config's `paths` or excludes leave out. It reads each file's config (and, with the `public` scope, the package's module graph) once, and keeps it until a file it came from changes or a config or package.json turns up closer. When the editor can watch files for a server, docov asks it to; otherwise, and for files outside the workspace, it compares their modified times and sizes on each edit.

VS Code needs an extension to start a language server, but editors with a generic LSP client can run it directly. In Neovim (0.10 or later):

```lua
vim.api.nvim_create_autocmd("FileType", {
  pattern = { "javascript", "javascriptreact", "typescript", "typescriptreact" },
  callback = function(args)
    vim.lsp.start({
      name = "docov",
      cmd = { "docov", "lsp" }, -- or { "npx", "docov", "lsp" } with @docov/cli in devDependencies
      root_dir = vim.fs.root(args.buf, { "docov.json", "docov.jsonc", ".docovrc", "package.json", ".git" }),
    })
  end,
})
```

In Helix's `languages.toml`, add it next to each language's own server, and the same `[[language]]` for `javascript`, `jsx` and `tsx`:

```toml
[language-server.docov]
command = "docov"
args = ["lsp"]

[[language]]
name = "typescript"
language-servers = ["typescript-language-server", "docov"]
```

To check a buffer before it's saved without LSP, `--stdin-filename PATH` scans stdin as the file at PATH, whose extension picks the language, and `--format unix` prints lines a problem matcher reads. `--stdin-filename` can't go with `--add-ignore`, `--fix`, `--write-baseline`, `--update-threshold`, `--watch`, `--changed`, `--staged` or `--explain`, which would act on that one file.

`--watch` scans again whenever a JavaScript, TypeScript or config file under the paths changes, with a line between reports, until Ctrl-C.

### Check before each commit

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

### Report to Codecov

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

### Scan faster

`--cache` keeps each file's items in `.docov-cache` in the config's folder (or the file `--cache-location` names), and the next scan reuses them for each file whose size and modified time haven't changed, as long as docov's version and the settings that change what counts are the same. Add it to `.gitignore`. `--threads N` scans N files at once, one per CPU by default, and `RAYON_NUM_THREADS` sets the same.

## Reference

| Page | What's in it |
| --- | --- |
| [docs/cli.md](docs/cli.md) | Every flag, as `docov --help` prints it |
| [docs/config.md](docs/config.md) | Every docov.json key, with its type, default and values |
| [docs/checks.md](docs/checks.md) | Every doc check, with its options and an example that fails and one that passes |
| [docs/formats.md](docs/formats.md) | Every output format, with a sample |

Tests generate these from the code, so they match the docov they ship with.

## Working on it

The toolchain is pinned in `rust-toolchain.toml`, and Node in `.node-version`. The checks need [cargo-nextest](https://nexte.st) and [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov). `tools/lint.sh` also runs [shellcheck](https://www.shellcheck.net), [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) (`deny.toml`), [zizmor](https://docs.zizmor.sh) and [actionlint](https://github.com/rhysd/actionlint) when they're installed, as they are on CI's Linux lint job.

Each flag and docov.json key belongs to a feature, one file in `src/feature/` that implements `Feature`. Each output format is one file in `src/report/` with a `FORMAT` naming it, describing it for `--help`, and rendering it. Each doc check is one file in `src/check/` that implements `Check`, its rule id the file name in kebab case. `build.rs` registers every file in the three folders, so a new feature, format or check is a new file and nothing else. A new check also needs a failing and a passing example in `tests/fixtures/checks/<rule-id>/`.

`tests/docs.rs` generates `docs/` from the schema, the registries and each format's and check's module docs, and runs the README's `console` examples on `tests/fixtures/quickstart`. It fails when any of them is stale, and `DOCOV_BLESS=1` rewrites them.

```bash
cargo run -- [PATHS]...   # docov on a project
tools/check.sh            # what CI runs: tools/lint.sh (format, clippy, rustdoc, matching crate and npm versions, and the linters above) and tools/coverage.sh (coverage for the whole suite, for the Hegel properties alone, and for the npm launcher's tests)
DOCOV_BLESS=1 cargo test --test docs   # regenerate docs/ and the README's example output
tools/jsdoc-oracle.sh [DIR]   # where docov and jsdoc disagree on a folder of plain JavaScript (default: tests/fixtures/jsdoc)
tools/npm-approve.sh 1.2.3   # put a release's staged npm packages live
```

Releases go through [uppt](https://github.com/danielroe/uppt) in `.github/workflows/release.yml`. Each push to main opens or updates a draft release PR from the conventional commits since the last tag, bumping the version in `npm/docov/package.json` and `Cargo.toml` together, and `tools/release-pr-changelog.mjs` drops any entry listed twice. Merging it tags the version, builds the binaries, publishes the crate to crates.io, and stages the npm packages, which go live once `tools/npm-approve.sh` approves them. `gh workflow run release.yml -f prerelease=beta` opens a prerelease PR instead. The npm packages are `@docov/cli` (`npm/docov`, a launcher) and an `@docov/<platform>-<arch>` package per binary. Both registries use trusted publishing, so there are no tokens. `tools/bootstrap-publishing.sh` sets it up, along with the GitHub environments the publishing jobs run in.

`cargo coverage` fails under the thresholds in `.cargo/config.toml`. On a pull request, lint and coverage are separate pass/fail checks for each OS, `msrv` builds the packaged crate with `rust-version`'s Rust, and `ci` passes when every other job did. Dependabot opens grouped updates monthly (`.github/dependabot.yml`). With `LCOV_DIR` set, `tools/coverage.sh` writes each Rust coverage run to `$LCOV_DIR/suite.lcov` and `props.lcov` as well as printing its table. CI sets it, puts each run's per-file numbers in the job summary, and uploads the lcov files as the `coverage-<os>` artifact. The property tests (`tests/props.rs`) use [Hegel](https://github.com/hegeldev/hegel-rust). They check scans against a model of generated modules, the language server over an in-memory connection, and the binary against generated projects.

`tools/jsdoc-oracle.sh` runs jsdoc 4 (`npx --yes jsdoc@4 -X`, so it needs Node and the network) and `docov --scope all --format json` on the same files, and lists each declaration only one of them counts, or that they disagree on documenting. It isn't part of the checks: some disagreements are by design (jsdoc counts constructors, `this.x =` members, object literal members, destructured variables and `/** */` and `@license` blocks), and a `@typedef` or `@file` block right before an undocumented declaration documents it for docov but not for jsdoc unless `"virtual"` or `"fileDocs"` is on.
