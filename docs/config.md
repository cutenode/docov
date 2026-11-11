<!-- Generated from docov.schema.json by tests/docs.rs: run `DOCOV_BLESS=1 cargo test --test docs`. -->

# docov.json

docov reads its settings from `docov.json`, `docov.jsonc` or `.docovrc`, in the folder it runs from or else the nearest folder above that has one, so a subfolder finds its project's config. In one folder, `docov.json` comes first and `.docovrc` last. `--config FILE` names one instead, and flags win over it. Comments and trailing commas are fine in all three. Its paths and globs are relative to its own folder, wherever docov runs from.

[`docov.schema.json`](../docov.schema.json) is its JSON Schema, which editors use for completion and checking. Point `$schema` at the copy for your release, `https://raw.githubusercontent.com/cutenode/docov/vX.Y.Z/docov.schema.json`, so it matches the docov you run, or write your own with `docov --print-schema > docov.schema.json`. Editors don't know `.docovrc` is JSON, so tell yours (in VS Code, `"files.associations": { ".docovrc": "jsonc" }`).

`docov --print-config` prints the settings a scan would use, merged from the config files and the flags.

## `$schema`

This file's JSON Schema, for editors.

Type: string.

## `access`

Count only items with these access levels, by their access tags. Without it, access tags change nothing.

Type: array of string.

| Value | Meaning |
| --- | --- |
| `public` | No access tag, or `@public`. |
| `protected` | `@protected`, or a TypeScript `protected` member. |
| `package` | `@package`. |
| `private` | `@private`. |

## `baseline`

A file of undocumented items the gates let pass, relative to this file's folder, as `docov --write-baseline` writes it.

Type: string.

## `commonjsMembers`

Count `X.prototype.m = …` and `X.m = …` at the top level as members of X, a function or class declared there, and `this.a = …` in a CommonJS file as an export.

Type: boolean. Default: `false`.

## `completeThreshold`

The lowest percentage of counted items documented with a @param for each parameter that passes.

Type: number.

## `examplesThreshold`

The lowest percentage of counted items with an @example that passes.

Type: number.

## `exclude`

Gitignore-style globs of files to leave out, relative to this file's folder, on top of .gitignore.

Type: array of string. Default: `[]`.

## `excludeTags`

Leave out items whose JSDoc has one of these tags, named without the @ (`internal` for @internal).

Type: array of string. Default: `[]`.

## `exports`

Which ways of exporting count under the `exports` scope. Globals are items under either scope only when on.

Type: object.

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `cjs` | boolean | `true` | CommonJS: `exports.a = …`, `module.exports = …` and TypeScript's `export =`. |
| `esm` | boolean | `true` | ES modules: `export`, `export { a }` and `export default`. |
| `globals` | boolean | `false` | Globals: `window.a = …` and `globalThis.a = …` at the top level. |

## `extends`

Config files this one builds on, relative to its folder. Its keys win over theirs, and a later one's over an earlier one's.

Type: string or array of string.

- string: One file.
- array of string: Files, each winning over the ones before it.

## `fileDocs`

Whether each file needs a JSDoc block of its own (`@file`, `@fileoverview`, `@overview`, `@module` or `@packageDocumentation`, with text), counted as an item of kind `module`.

Type: string. Default: `"off"`.

| Value | Meaning |
| --- | --- |
| `off` | Files aren't items. |
| `require` | Each file is an item of kind `module`, documented when its first JSDoc block has `@file`, `@fileoverview`, `@overview`, `@module` or `@packageDocumentation`, with text. That block then documents nothing else. |

## `ignore`

What to leave out.

Type: object.

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `names` | array of string |  | Regexes: an item whose name matches one doesn't count. A member matches by its own name or as `Class.member`. |

## `ignoreDeprecated`

Leave out items whose JSDoc has @deprecated.

Type: boolean. Default: `false`.

## `inherit`

What documents an item by inheriting its docs.

Type: string. Default: `"tag"`.

| Value | Meaning |
| --- | --- |
| `tag` | An `@inheritdoc` tag, or an inline `{@inheritDoc}` or `{@inheritDoc Ref}` anywhere in the block. |
| `override` | The same, or TypeScript's `override` on a class member. |

## `jsdocConfig`

A jsdoc.json, relative to this file's folder, whose `source` picks the files: its `include` is the paths unless `paths` is set, and its `exclude`, `includePattern` and `excludePattern` leave files out.

Type: string.

## `kinds`

Count only these kinds of item. Every kind counts without it.

Type: array of string.

| Value | Meaning |
| --- | --- |
| `function` | `function f() {}`, or a function assigned to an export. |
| `class` | `class C {}`, or a class assigned to an export. |
| `method` | A class method or an interface method signature, or a function member of a tagged object literal, an object type or a CommonJS export (with `members` or `commonjsMembers`). |
| `accessor` | A getter (with its setter, if any), a lone setter, or an `accessor` property. |
| `property` | A class field or an interface property signature, or another member that isn't a function (with `members` or `commonjsMembers`). |
| `variable` | `const`, `let` or `var`, or another value CommonJS exports by name. |
| `interface` | `interface I {}`. |
| `type` | `type T = …`. |
| `enum` | `enum E {}`. |
| `namespace` | `namespace N {}`. |
| `default` | `export default` or `module.exports =` of an expression. |
| `module` | A file, with `fileDocs`. |

## `maxUndocumented`

The most undocumented items that pass.

Type: number.

## `members`

Which members count: those of classes and interfaces, or all of them, with enum members, a type alias's object members and the properties of an object literal tagged @enum, @namespace or @lends.

Type: string. Default: `"classes"`.

| Value | Meaning |
| --- | --- |
| `classes` | The public members of classes and interfaces. |
| `all` | Those, and enum members, the members of a type alias's object type, and the properties of an object literal tagged `@enum`, `@namespace` or `@lends`. A matching `@property` in the parent's block documents one of these. |

## `nested`

Scan each file with the nearest docov.json, docov.jsonc or .docovrc above it, up to this file's folder, merged over this one. Only the root config's counts.

Type: boolean. Default: `false`.

## `overrides`

Settings for the files globs match, in place of the top-level ones. For each file and setting, the last override that matches it and has the setting wins.

Type: array of object. Default: `[]`.

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `exclude` | array of string |  | In place of the top-level `exclude`, for these files. |
| `files` | array of string |  | Gitignore-style globs of the files it's for, relative to this file's folder. |
| `scope` | string |  | In place of the top-level `scope`, for these files. `--scope` wins over it. |
| `threshold` | number or object |  | In place of the top-level `threshold`, for these files. |

## `paths`

Files and folders to scan, relative to this file's folder. Defaults to the folder docov runs in.

Type: array of string. Default: `[]`.

## `releaseTagsThreshold`

The lowest percentage of top-level exports with exactly one of @public, @beta, @alpha and @internal that passes.

Type: number.

## `reportUnusedIgnores`

Fail on a docov-ignore comment that changes nothing.

Type: boolean. Default: `false`.

## `requireDescription`

Count a JSDoc block only with text before its first tag, or @description, @summary, @classdesc or @inheritdoc.

Type: boolean. Default: `false`.

## `rules`

Doc checks to turn on, by rule id: "off", "warn" or "error", or that level with the check's options, as ["warn", { … }]. Every check is off by default. A rule set here replaces that rule's setting in a config this one extends or nests under, keeping its options if this gives a level alone, and leaves that config's other rules be.

Type: object.

The checks, and their options, are in [checks.md](checks.md).

## `scope`

Which declarations count.

Type: string. Default: `"exports"`.

| Value | Meaning |
| --- | --- |
| `exports` | What a module exports, with the public members of exported classes and interfaces. |
| `all` | Every module-level declaration, exported or not. |
| `public` | What the package's entry points (package.json's `exports`, `main`, `module`, `types` and `typings`) export, following re-exports through the module graph. |

## `skipGenerated`

Leave out files whose first comment has @generated.

Type: boolean. Default: `false`.

## `syntax`

The doc comment syntax docov writes and reads `#` in: `jsdoc` (stubs with `@template T`, `@param x` and `@file`, and `A#b` is a class's member) or `tsdoc` (stubs with `@typeParam T -`, `@param x -` and `@packageDocumentation`, `require-template` asking for `@typeParam`, and `pkg#A` a package's export, which `valid-link` doesn't look up).

Type: string. Default: `"jsdoc"`.

| Value | Meaning |
| --- | --- |
| `jsdoc` | JSDoc's: stubs have `@template T`, `@param x` and `@file`, `require-template` asks for `@template`, and `valid-link` looks up the class in `A#b`. |
| `tsdoc` | TSDoc's: stubs have `@typeParam T -`, `@param x -` and `@packageDocumentation`, `require-template` asks for `@typeParam`, and `valid-link` takes the `A` in `A#b` for a package and doesn't look it up. |

## `threshold`

The lowest documented percentage that passes, or thresholds by path: `global` for every file together, `perFile` for each file, and gitignore-style globs for the files each matches together.

Type: number or object.

- number: A documented percentage from 0 to 100, or a negative whole number for at most that many undocumented items.

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `global` | number |  | For every file together. |
| `perFile` | number |  | For each file on its own. |
| any other key | number |  | For the files a gitignore-style glob matches, relative to this file's folder, together. |

## `typesThreshold`

The lowest percentage of JavaScript functions and methods with a {type} on each @param and @returns that passes.

Type: number.

## `virtual`

Count a JavaScript file's named @typedef and @callback blocks as items of kind `type`, documented when they have a description. They then document nothing else.

Type: boolean. Default: `false`.
