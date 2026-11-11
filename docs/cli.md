<!-- Generated from `docov --help` by tests/docs.rs: run `DOCOV_BLESS=1 cargo test --test docs`. -->

# Command line

`docov --help` prints this, and `docov -h` a summary. Flags win over the config file's settings, which [config.md](config.md) lists.

```text
Checks how much of a JavaScript or TypeScript project's API has JSDoc.

Exits 1 when a gate fails (a threshold, say), 2 when a file or the config can't be read.

Usage: docov [OPTIONS] [PATHS]...
       docov <COMMAND>

Commands:
  init  Write a docov.json here with a threshold of the current coverage, rounded down
  lsp   Run a language server on stdin and stdout, for editors

Arguments:
  [PATHS]...
          Files and folders to scan [default: the config's `paths`, or the current folder]

Options:
      --config <FILE>
          The config file [default: docov.json, docov.jsonc or .docovrc here or in the nearest
          folder above]

      --format <FORMAT[=FILE]>
          How to write the report, repeatable: a format, or FORMAT=FILE to write it to FILE
          [default: text]
          
          - badge: An SVG badge of the total, colored by the threshold
          - benchmark: The total for github-action-benchmark's customBiggerIsBetter
          - checkstyle: Checkstyle XML: undocumented items and doc check findings
          - cobertura: Cobertura XML: the lcov line records, a package per folder
          - github: GitHub Actions annotations: undocumented items, findings and failed gates
          - gitlab: A GitLab Code Quality report: undocumented items and findings
          - html: A page of each file's source, with undocumented items and findings marked
          - json: Every item, documented or not, with totals
          - junit: JUnit XML: a test per item, and a failed one per doc check error
          - lcov: A line record for each line with items, for coverage services and editor gutters
          - markdown: A summary for $GITHUB_STEP_SUMMARY or a pull request comment
          - rdjson: reviewdog's diagnostic format (rdjson)
          - sarif: SARIF 2.1.0, for GitHub code scanning
          - shields: JSON for a shields.io endpoint badge of the total
          - text: Each file's percentage, its undocumented items, and the total (the default)
          - unix: path:line:column: what's wrong, per undocumented item and doc check finding

      --output <FILE>
          Write the one format without a file of its own to FILE

      --badge <FILE>
          Write an SVG badge of the total to FILE, as `--format badge=FILE` does

      --compare <FILE>
          Show the change from FILE, an earlier `--format json` report, in the markdown report

      --omit-covered
          Leave fully documented files out of the text report

      --quiet
          Print only the total, and why the scan fails, in the text report

      --by-kind
          After the text report's total, a line of totals by kind

      --print-schema [<FILE>]
          Print the config file's JSON Schema, or the baseline file's, and exit

          Possible values:
          - config:   docov.json
          - baseline: The file `--write-baseline` writes

      --access <LEVEL>
          Count only items with these access levels, comma-separated [default: the config's, or
          every item]

          Possible values:
          - public:    No access tag, or `@public`
          - protected: `@protected`, or a TypeScript `protected` member
          - package:   `@package`
          - private:   `@private`

      --add-ignore[=<REASON>]
          Write `// docov-ignore-next-line: REASON` (or with no reason) above each undocumented
          item, and report nothing else

      --baseline <FILE>
          A file of undocumented items the gates let pass [default: the config's `baseline`]

      --write-baseline
          Write every undocumented item to the baseline file and exit 0

      --by-package
          Add each workspace package's total to the text and JSON reports

      --cache
          Keep each file's items, and reuse them while the file's size and modified time stay the
          same

      --cache-location <FILE>
          The cache file [default: .docov-cache in the config's folder]

      --changed
          Scan only files changed since HEAD branched from the default branch, committed or not

      --since <REF>
          With --changed, the branch or commit to compare with [default: origin/HEAD, or main]

      --staged
          Scan only files staged for the next commit

      --color <COLOR>
          When to color the text report

          Possible values:
          - auto:   When stdout is a terminal and `NO_COLOR` isn't set
          - always: Always
          - never:  Never
          
          [default: auto]

      --complete
          Also report how many items are documented with a @param for each parameter

      --complete-threshold <PERCENT>
          Fail under this percentage of complete items

      --diff-base <REF>
          With --diff-threshold, the branch or commit the change is from [default: origin/HEAD, or
          main]

      --diff-threshold <PERCENT>
          Fail when the items on lines a change added are under this documented percentage

      --examples
          On each line of the text report, how many of its items have an @example

      --examples-threshold <PERCENT>
          Fail under this percentage of items with an @example

      --exclude-tag <NAME>
          Leave out items whose JSDoc has this tag (`internal` for @internal), repeatable [default:
          the config's]

      --explain <FILE>
          Say why each item in FILE counts or doesn't and is documented or isn't, and what the scan
          passed over there, in place of a report

      --fix
          Write a JSDoc stub, prefilled from the code, above each undocumented item with no block,
          and report nothing else

      --ignore-name <REGEX>
          Leave out items whose name (or member name) matches this regex, repeatable [default: the
          config's]

      --jsdoc-config <FILE>
          Scan the files this jsdoc.json documents [default: the config's `jsdocConfig`]

      --kind <KIND>
          Count only these kinds of item, comma-separated [default: the config's, or every kind]

          Possible values:
          - function:  `function f() {}`, or a function assigned to an export
          - class:     `class C {}`, or a class assigned to an export
          - method:    A class method or an interface method signature, or a function member of a
            tagged object literal, an object type or a CommonJS export (with `members` or
            `commonjsMembers`)
          - accessor:  A getter (with its setter, if any), a lone setter, or an `accessor` property
          - property:  A class field or an interface property signature, or another member that
            isn't a function (with `members` or `commonjsMembers`)
          - variable:  `const`, `let` or `var`, or another value CommonJS exports by name
          - interface: `interface I {}`
          - type:      `type T = …`
          - enum:      `enum E {}`
          - namespace: `namespace N {}`
          - default:   `export default` or `module.exports =` of an expression
          - module:    A file, with `fileDocs`

      --list-files
          Print the files that would be scanned, one per line, and exit

      --max-undocumented <N>
          Fail with more than this many undocumented items

      --print-config
          Print the settings, merged from the config files and flags, as JSON and exit

      --release-tags
          Also report how many top-level exports have exactly one of @public, @beta, @alpha and
          @internal

      --release-tags-threshold <PERCENT>
          Fail under this percentage of release-tagged top-level exports

      --report-unused-ignores
          Fail on a docov-ignore comment that changes nothing

      --require-description
          Count a JSDoc block only with a description: text before its first tag, or @description,
          @summary, @classdesc or @inheritdoc

      --rule <ID=LEVEL>
          Set a doc check's level (off, warn or error), over the config's (repeatable)

      --scope <SCOPE>
          Which declarations count, over the config's and its overrides' [default: the config's, or
          exports]

          Possible values:
          - exports: What a module exports, with the public members of exported classes and
            interfaces
          - all:     Every module-level declaration, exported or not
          - public:  What the package's entry points (package.json's `exports`, `main`, `module`,
            `types` and `typings`) export, following re-exports through the module graph

      --sort <SORT>
          The order of the text report's undocumented items

          Possible values:
          - position: Where they are in the file
          - priority: What to document first: exported functions and methods, those with more
            parameters first, then classes, then the rest, each in file order
          
          [default: position]

      --stdin-filename <PATH>
          Scan stdin as the file at this path, whose extension picks the language, in place of the
          paths

      --threads <N>
          Scan this many files at once [default: RAYON_NUM_THREADS, or one per CPU]

      --threshold <PERCENT>
          Fail under this documented percentage, overall

      --types
          Also report how many JavaScript functions have a {type} on each @param and @returns

      --types-threshold <PERCENT>
          Fail under this percentage of typed JavaScript functions

      --update-threshold
          Raise the config's threshold to the documented percentage, rounded down, when that's
          higher

      --watch
          Scan again whenever a file under the paths changes, until Ctrl-C

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```
