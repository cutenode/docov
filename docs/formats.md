<!-- Generated from src/report/ by tests/docs.rs: run `DOCOV_BLESS=1 cargo test --test docs`. -->

# Output formats

`--format` picks one, and can be given more than once: one scan feeds them all. `FORMAT=FILE` writes that one to `FILE`, making its folder if needed, and the rest go to stdout one after another. `--output FILE` writes the one format without a file of its own to `FILE`.

```sh
docov --format text --format lcov=coverage/docov.lcov
```

Each sample is the real output of `docov --threshold 80 --rule require-param=warn --format NAME` on [tests/fixtures/quickstart](../tests/fixtures/quickstart), a project with undocumented items, a doc check finding and a failed gate. docov's version is shown as `X.Y.Z`, and cobertura's timestamp as `TIMESTAMP`.

| Format | What it writes |
| --- | --- |
| [`badge`](#badge) | An SVG badge of the total, colored by the threshold |
| [`benchmark`](#benchmark) | The total for github-action-benchmark's customBiggerIsBetter |
| [`checkstyle`](#checkstyle) | Checkstyle XML: undocumented items and doc check findings |
| [`cobertura`](#cobertura) | Cobertura XML: the lcov line records, a package per folder |
| [`github`](#github) | GitHub Actions annotations: undocumented items, findings and failed gates |
| [`gitlab`](#gitlab) | A GitLab Code Quality report: undocumented items and findings |
| [`html`](#html) | A page of each file's source, with undocumented items and findings marked |
| [`json`](#json) | Every item, documented or not, with totals |
| [`junit`](#junit) | JUnit XML: a test per item, and a failed one per doc check error |
| [`lcov`](#lcov) | A line record for each line with items, for coverage services and editor gutters |
| [`markdown`](#markdown) | A summary for $GITHUB_STEP_SUMMARY or a pull request comment |
| [`rdjson`](#rdjson) | reviewdog's diagnostic format (rdjson) |
| [`sarif`](#sarif) | SARIF 2.1.0, for GitHub code scanning |
| [`shields`](#shields) | JSON for a shields.io endpoint badge of the total |
| [`text`](#text) | Each file's percentage, its undocumented items, and the total (the default) |
| [`unix`](#unix) | path:line:column: what's wrong, per undocumented item and doc check finding |

## `badge`

An SVG badge of the total, drawn without a network call.

`--badge FILE` is short for `--format badge=FILE`. The badge is red under the global threshold, yellow up to 10 points over it, and green past that. With no threshold, or a negative one (a count of undocumented items), it's blue.

```xml
<svg xmlns="http://www.w3.org/2000/svg" width="82" height="20" role="img" aria-label="docs: 55.6%">
  <title>docs: 55.6%</title>
  <linearGradient id="shine" x2="0" y2="100%">
    <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>
    <stop offset="1" stop-opacity=".1"/>
  </linearGradient>
  <clipPath id="round">
    <rect width="82" height="20" rx="3" fill="#fff"/>
  </clipPath>
  <g clip-path="url(#round)">
    <rect width="37" height="20" fill="#555"/>
    <rect x="37" width="45" height="20" fill="#e05d44"/>
    <rect width="82" height="20" fill="url(#shine)"/>
  </g>
  <g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="11">
    <text x="18.5" y="15" fill="#010101" fill-opacity=".3">docs</text>
    <text x="18.5" y="14">docs</text>
    <text x="59.5" y="15" fill="#010101" fill-opacity=".3">55.6%</text>
    <text x="59.5" y="14">55.6%</text>
  </g>
</svg>
```

## `benchmark`

The total for github-action-benchmark, to chart it over time.

[github-action-benchmark](https://github.com/benchmark-action/github-action-benchmark) reads it with `tool: customBiggerIsBetter`.

```json
[{"name":"docov","unit":"%","value":55.6}]
```

## `checkstyle`

Checkstyle XML, which many CI tools and editors read.

It has a warning at each undocumented item, with the source `docov/undocumented`, and one at its level for each doc check finding, with the source `docov.<rule-id>` as ESLint's Checkstyle formatter writes it, such as `docov.require-param`.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<checkstyle version="4.3">
  <file name="src/cart.ts">
    <error line="5" column="3" severity="warning" message="property Item.quantity is undocumented" source="docov/undocumented"/>
    <error line="18" column="1" severity="warning" message="class Cart is undocumented" source="docov/undocumented"/>
    <error line="26" column="3" severity="warning" message="accessor Cart.size is undocumented" source="docov/undocumented"/>
    <error line="21" column="3" severity="warning" message="missing @param for `item`" source="docov.require-param"/>
  </file>
  <file name="src/format.js">
    <error line="11" column="1" severity="warning" message="variable CURRENCY is undocumented" source="docov/undocumented"/>
  </file>
</checkstyle>
```

## `cobertura`

Cobertura XML, for GitLab's merge request coverage and other CI coverage views.

It follows coverage-04.dtd, with the line records `lcov` writes, a package per folder and a class per file.

<details>
<summary>Sample</summary>

```xml
<?xml version="1.0" ?>
<!DOCTYPE coverage SYSTEM "http://cobertura.sourceforge.net/xml/coverage-04.dtd">
<coverage line-rate="0.5555555555555556" branch-rate="0" lines-covered="5" lines-valid="9" branches-covered="0" branches-valid="0" complexity="0" version="X.Y.Z" timestamp="TIMESTAMP">
  <sources>
    <source>.</source>
  </sources>
  <packages>
    <package name="src" line-rate="0.5555555555555556" branch-rate="0" complexity="0">
      <classes>
        <class name="cart.ts" filename="src/cart.ts" line-rate="0.5714285714285715" branch-rate="0" complexity="0">
          <methods/>
          <lines>
            <line number="2" hits="1"/>
            <line number="4" hits="1"/>
            <line number="5" hits="0"/>
            <line number="14" hits="1"/>
            <line number="18" hits="0"/>
            <line number="22" hits="1"/>
            <line number="26" hits="0"/>
          </lines>
        </class>
        <class name="format.js" filename="src/format.js" line-rate="0.5" branch-rate="0" complexity="0">
          <methods/>
          <lines>
            <line number="7" hits="1"/>
            <line number="11" hits="0"/>
          </lines>
        </class>
      </classes>
    </package>
  </packages>
</coverage>
```

</details>

## `github`

GitHub Actions annotations, a warning on each undocumented item, one at its level on each doc check finding, and an error for each failed gate.

In GitHub Actions (`GITHUB_ACTIONS=true`) with no `--format`, docov writes these after the text report, the way [oxlint](https://oxc.rs/docs/guide/usage/linter/output-formats.html) does.

```text
::warning file=src/cart.ts,line=5,col=3,title=docov::property Item.quantity is undocumented
::warning file=src/cart.ts,line=18,col=1,title=docov::class Cart is undocumented
::warning file=src/cart.ts,line=26,col=3,title=docov::accessor Cart.size is undocumented
::warning file=src/format.js,line=11,col=1,title=docov::variable CURRENCY is undocumented
::warning file=src/cart.ts,line=21,col=3,title=docov/require-param::missing @param for `item`
::error title=docov::55.6%25 documented, under the 80%25 threshold
```

## `gitlab`

A GitLab Code Quality report, for merge request widgets.

It has an issue of severity `minor` at each undocumented item, and one at each doc check finding under the check's rule, such as `docov/require-param`: `minor` at `warn`, `major` at `error`. Each fingerprint is a hash of the item's path, kind and name, or the finding's path, line, rule and message, so GitLab tells new issues from old ones.

<details>
<summary>Sample</summary>

```json
[
  {
    "check_name": "docov/undocumented",
    "description": "property Item.quantity is undocumented",
    "fingerprint": "94996ebb8140fa82",
    "location": {
      "lines": {
        "begin": 5
      },
      "path": "src/cart.ts"
    },
    "severity": "minor"
  },
  {
    "check_name": "docov/undocumented",
    "description": "class Cart is undocumented",
    "fingerprint": "85f2d54f391b4a51",
    "location": {
      "lines": {
        "begin": 18
      },
      "path": "src/cart.ts"
    },
    "severity": "minor"
  },
  {
    "check_name": "docov/undocumented",
    "description": "accessor Cart.size is undocumented",
    "fingerprint": "dc1e28ba53d45dc9",
    "location": {
      "lines": {
        "begin": 26
      },
      "path": "src/cart.ts"
    },
    "severity": "minor"
  },
  {
    "check_name": "docov/undocumented",
    "description": "variable CURRENCY is undocumented",
    "fingerprint": "3878c48164043597",
    "location": {
      "lines": {
        "begin": 11
      },
      "path": "src/format.js"
    },
    "severity": "minor"
  },
  {
    "check_name": "docov/require-param",
    "description": "missing @param for `item`",
    "fingerprint": "ac0d0885c64c758c",
    "location": {
      "lines": {
        "begin": 21
      },
      "path": "src/cart.ts"
    },
    "severity": "minor"
  }
]
```

</details>

## `html`

One self-contained page, with a table of the files and each file's source, its undocumented declarations highlighted and its doc check findings marked.

A line is marked missed when one of its items is undocumented and covered when all of them are documented. A list of the file's findings follows its source. The page has no scripts and follows the system's light or dark mode. Give it a file, as in `--format html=report.html`.

<details>
<summary>Sample</summary>

```html
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<title>docov: 55.6% documented</title>
<style>
:root {
  color-scheme: light dark;
  --bg: #ffffff;
  --fg: #1f2328;
  --muted: #59636e;
  --line: #d1d9e0;
  --miss: #ffebe9;
  --miss-edge: #cf222e;
  --hit: #dafbe1;
  --hit-edge: #1a7f37;
  --warn: #9a6700;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #0d1117;
    --fg: #e6edf3;
    --muted: #9198a1;
    --line: #3d444d;
    --miss: #4b1d22;
    --miss-edge: #f85149;
    --hit: #12301d;
    --hit-edge: #3fb950;
    --warn: #d29922;
  }
}
* { box-sizing: border-box; }
body { margin: 0 auto; max-width: 72rem; padding: 2rem 1rem; background: var(--bg); color: var(--fg); font: 15px/1.5 system-ui, sans-serif; }
h1 { font-size: 1.5rem; margin: 0 0 .25rem; }
h2 { font-size: 1rem; margin: 2.5rem 0 .5rem; overflow-wrap: anywhere; }
a { color: inherit; }
.total { color: var(--muted); margin: 0 0 1.5rem; }
.failed { color: var(--miss-edge); margin: 0 0 .5rem; }
table { border-collapse: collapse; width: 100%; }
.files th, .files td { border-bottom: 1px solid var(--line); padding: .375rem .75rem; text-align: left; }
.files .num { font-variant-numeric: tabular-nums; text-align: right; white-space: nowrap; width: 1%; }
.files td:last-child { overflow-wrap: anywhere; }
.scroll { overflow-x: auto; border: 1px solid var(--line); border-radius: 6px; }
.source { font: 12px/1.6 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
.source td { padding: 0 .75rem; vertical-align: top; white-space: pre; }
.source .ln { color: var(--muted); text-align: right; user-select: none; width: 4em; border-right: 1px solid var(--line); }
.source .note { color: var(--muted); font-family: system-ui, sans-serif; width: 1%; }
.source .miss { background: var(--miss); }
.source .miss .ln { border-right: 3px solid var(--miss-edge); }
.source .hit .ln { border-right: 3px solid var(--hit-edge); }
.source .finding .note { color: var(--warn); }
.missing { color: var(--muted); }
.findings { margin: .5rem 0 0; padding-left: 1.25rem; font: 12px/1.6 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; overflow-wrap: anywhere; }
.findings .warning { color: var(--warn); }
.findings .error { color: var(--miss-edge); }
</style>
</head>
<body>
<h1>docov</h1>
<p class="total">55.6% documented (5/9)</p>
<p class="failed">Failed: 55.6% documented, under the 80% threshold</p>
<table class="files">
<thead><tr><th class="num">Documented</th><th class="num">Items</th><th>File</th></tr></thead>
<tbody>
<tr><td class="num">57.1%</td><td class="num">4/7</td><td><a href="#file-0">src/cart.ts</a></td></tr>
<tr><td class="num">50.0%</td><td class="num">1/2</td><td><a href="#file-1">src/format.js</a></td></tr>
</tbody>
</table>
<h2 id="file-0">src/cart.ts (57.1%)</h2>
<div class="scroll">
<table class="source">
<tbody>
<tr><td class="ln">1</td><td>/** A line in a shopping cart. */</td><td class="note"></td></tr>
<tr class="hit"><td class="ln">2</td><td>export interface Item {</td><td class="note"></td></tr>
<tr><td class="ln">3</td><td>  /** The product&#39;s SKU. */</td><td class="note"></td></tr>
<tr class="hit"><td class="ln">4</td><td>  sku: string;</td><td class="note"></td></tr>
<tr class="miss"><td class="ln">5</td><td>  quantity: number;</td><td class="note">property Item.quantity</td></tr>
<tr><td class="ln">6</td><td>}</td><td class="note"></td></tr>
<tr><td class="ln">7</td><td></td><td class="note"></td></tr>
<tr><td class="ln">8</td><td>/**</td><td class="note"></td></tr>
<tr><td class="ln">9</td><td> * Adds up a cart.</td><td class="note"></td></tr>
<tr><td class="ln">10</td><td> *</td><td class="note"></td></tr>
<tr><td class="ln">11</td><td> * @param items - What&#39;s in the cart.</td><td class="note"></td></tr>
<tr><td class="ln">12</td><td> * @param prices - Each SKU&#39;s price, in cents.</td><td class="note"></td></tr>
<tr><td class="ln">13</td><td> */</td><td class="note"></td></tr>
<tr class="hit"><td class="ln">14</td><td>export function total(items: Item[], prices: Map&lt;string, number&gt;): number {</td><td class="note"></td></tr>
<tr><td class="ln">15</td><td>  return items.reduce((sum, item) =&gt; sum + (prices.get(item.sku) ?? 0) * item.quantity, 0);</td><td class="note"></td></tr>
<tr><td class="ln">16</td><td>}</td><td class="note"></td></tr>
<tr><td class="ln">17</td><td></td><td class="note"></td></tr>
<tr class="miss"><td class="ln">18</td><td>export class Cart {</td><td class="note">class Cart</td></tr>
<tr><td class="ln">19</td><td>  #items: Item[] = [];</td><td class="note"></td></tr>
<tr><td class="ln">20</td><td></td><td class="note"></td></tr>
<tr class="finding"><td class="ln">21</td><td>  /** Adds an item to the cart. */</td><td class="note">warning require-param: missing @param for `item`</td></tr>
<tr class="hit"><td class="ln">22</td><td>  add(item: Item): void {</td><td class="note"></td></tr>
<tr><td class="ln">23</td><td>    this.#items.push(item);</td><td class="note"></td></tr>
<tr><td class="ln">24</td><td>  }</td><td class="note"></td></tr>
<tr><td class="ln">25</td><td></td><td class="note"></td></tr>
<tr class="miss"><td class="ln">26</td><td>  get size(): number {</td><td class="note">accessor Cart.size</td></tr>
<tr><td class="ln">27</td><td>    return this.#items.length;</td><td class="note"></td></tr>
<tr><td class="ln">28</td><td>  }</td><td class="note"></td></tr>
<tr><td class="ln">29</td><td>}</td><td class="note"></td></tr>
<tr><td class="ln">30</td><td></td><td class="note"></td></tr>
<tr><td class="ln">31</td><td>// docov-ignore-next-line: kept for old callers</td><td class="note"></td></tr>
<tr><td class="ln">32</td><td>export const legacyTotal = total;</td><td class="note"></td></tr>
</tbody>
</table>
</div>
<ul class="findings">
<li class="warning">21:3 warning require-param: missing @param for `item`</li>
</ul>
<h2 id="file-1">src/format.js (50.0%)</h2>
<div class="scroll">
<table class="source">
<tbody>
<tr><td class="ln">1</td><td>/**</td><td class="note"></td></tr>
<tr><td class="ln">2</td><td> * Formats cents as dollars.</td><td class="note"></td></tr>
<tr><td class="ln">3</td><td> *</td><td class="note"></td></tr>
<tr><td class="ln">4</td><td> * @param {number} cents</td><td class="note"></td></tr>
<tr><td class="ln">5</td><td> * @returns {string}</td><td class="note"></td></tr>
<tr><td class="ln">6</td><td> */</td><td class="note"></td></tr>
<tr class="hit"><td class="ln">7</td><td>export function dollars(cents) {</td><td class="note"></td></tr>
<tr><td class="ln">8</td><td>  return `$${(cents / 100).toFixed(2)}`;</td><td class="note"></td></tr>
<tr><td class="ln">9</td><td>}</td><td class="note"></td></tr>
<tr><td class="ln">10</td><td></td><td class="note"></td></tr>
<tr class="miss"><td class="ln">11</td><td>export const CURRENCY = &#39;USD&#39;;</td><td class="note">variable CURRENCY</td></tr>
<tr><td class="ln">12</td><td></td><td class="note"></td></tr>
<tr><td class="ln">13</td><td>function round(n) {</td><td class="note"></td></tr>
<tr><td class="ln">14</td><td>  return Math.round(n);</td><td class="note"></td></tr>
<tr><td class="ln">15</td><td>}</td><td class="note"></td></tr>
</tbody>
</table>
</div>
</body>
</html>
```

</details>

## `json`

Every item, documented or not, with totals. `--compare` reads one back.

Each item has its kind, line and column, and there are totals per file, per kind and overall. Each item also says whether its JSDoc describes it (`hasDescription`) and has an `@example` (`hasExample`), whether each parameter has a matching `@param` (`paramsDocumented`, `null` for what isn't a function or method), whether it's `deprecated`, and its `since` text (or `null`). With doc checks on, an item they found something in lists its findings in `diagnostics`, each with its `line`, `column`, `rule`, `severity` and `message`. An undocumented item a `docov-ignore` comment leaves out of the totals has `ignored: true`, and the comment's `reason` when it gives one.

<details>
<summary>Sample</summary>

```json
{
  "documented": 5,
  "total": 9,
  "percent": 55.55555555555556,
  "withExamples": 0,
  "examplesPercent": 0.0,
  "kinds": {
    "function": {
      "documented": 2,
      "total": 2,
      "percent": 100.0
    },
    "class": {
      "documented": 0,
      "total": 1,
      "percent": 0.0
    },
    "method": {
      "documented": 1,
      "total": 1,
      "percent": 100.0
    },
    "accessor": {
      "documented": 0,
      "total": 1,
      "percent": 0.0
    },
    "property": {
      "documented": 1,
      "total": 2,
      "percent": 50.0
    },
    "variable": {
      "documented": 0,
      "total": 1,
      "percent": 0.0
    },
    "interface": {
      "documented": 1,
      "total": 1,
      "percent": 100.0
    }
  },
  "files": [
    {
      "path": "src/cart.ts",
      "documented": 4,
      "total": 7,
      "percent": 57.142857142857146,
      "withExamples": 0,
      "examplesPercent": 0.0,
      "items": [
        {
          "name": "Item",
          "kind": "interface",
          "line": 2,
          "column": 1,
          "documented": true,
          "hasDescription": true,
          "hasExample": false,
          "paramsDocumented": null,
          "deprecated": false,
          "since": null
        },
        {
          "name": "Item.sku",
          "kind": "property",
          "line": 4,
          "column": 3,
          "documented": true,
          "hasDescription": true,
          "hasExample": false,
          "paramsDocumented": null,
          "deprecated": false,
          "since": null
        },
        {
          "name": "Item.quantity",
          "kind": "property",
          "line": 5,
          "column": 3,
          "documented": false,
          "hasDescription": false,
          "hasExample": false,
          "paramsDocumented": null,
          "deprecated": false,
          "since": null
        },
        {
          "name": "total",
          "kind": "function",
          "line": 14,
          "column": 1,
          "documented": true,
          "hasDescription": true,
          "hasExample": false,
          "paramsDocumented": true,
          "deprecated": false,
          "since": null
        },
        {
          "name": "Cart",
          "kind": "class",
          "line": 18,
          "column": 1,
          "documented": false,
          "hasDescription": false,
          "hasExample": false,
          "paramsDocumented": null,
          "deprecated": false,
          "since": null
        },
        {
          "name": "Cart.add",
          "kind": "method",
          "line": 22,
          "column": 3,
          "documented": true,
          "hasDescription": true,
          "hasExample": false,
          "paramsDocumented": false,
          "deprecated": false,
          "since": null,
          "diagnostics": [
            {
              "line": 21,
              "column": 3,
              "rule": "require-param",
              "severity": "warn",
              "message": "missing @param for `item`"
            }
          ]
        },
        {
          "name": "Cart.size",
          "kind": "accessor",
          "line": 26,
          "column": 3,
          "documented": false,
          "hasDescription": false,
          "hasExample": false,
          "paramsDocumented": null,
          "deprecated": false,
          "since": null
        },
        {
          "name": "legacyTotal",
          "kind": "variable",
          "line": 32,
          "column": 1,
          "documented": false,
          "hasDescription": false,
          "hasExample": false,
          "paramsDocumented": null,
          "deprecated": false,
          "since": null,
          "ignored": true,
          "reason": "kept for old callers"
        }
      ]
    },
    {
      "path": "src/format.js",
      "documented": 1,
      "total": 2,
      "percent": 50.0,
      "withExamples": 0,
      "examplesPercent": 0.0,
      "items": [
        {
          "name": "dollars",
          "kind": "function",
          "line": 7,
          "column": 1,
          "documented": true,
          "hasDescription": true,
          "hasExample": false,
          "paramsDocumented": true,
          "deprecated": false,
          "since": null
        },
        {
          "name": "CURRENCY",
          "kind": "variable",
          "line": 11,
          "column": 1,
          "documented": false,
          "hasDescription": false,
          "hasExample": false,
          "paramsDocumented": null,
          "deprecated": false,
          "since": null
        }
      ]
    }
  ]
}
```

</details>

## `junit`

JUnit XML, for CI test report views.

It has a test suite per file and a test case per item, which fails when the item is undocumented, followed by a failed test case for each doc check finding at `error` in it. Its findings at `warn` go in its test case's `<system-out>`, so they show without failing it.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="docov" tests="9" failures="4">
  <testsuite name="src/cart.ts" tests="7" failures="3">
    <testcase name="interface Item" classname="src/cart.ts" file="src/cart.ts" line="2"/>
    <testcase name="property Item.sku" classname="src/cart.ts" file="src/cart.ts" line="4"/>
    <testcase name="property Item.quantity" classname="src/cart.ts" file="src/cart.ts" line="5">
      <failure message="property Item.quantity is undocumented" type="undocumented">src/cart.ts:5:3</failure>
    </testcase>
    <testcase name="function total" classname="src/cart.ts" file="src/cart.ts" line="14"/>
    <testcase name="class Cart" classname="src/cart.ts" file="src/cart.ts" line="18">
      <failure message="class Cart is undocumented" type="undocumented">src/cart.ts:18:1</failure>
    </testcase>
    <testcase name="method Cart.add" classname="src/cart.ts" file="src/cart.ts" line="22">
      <system-out>src/cart.ts:21:3: require-param: missing @param for `item`&#10;</system-out>
    </testcase>
    <testcase name="accessor Cart.size" classname="src/cart.ts" file="src/cart.ts" line="26">
      <failure message="accessor Cart.size is undocumented" type="undocumented">src/cart.ts:26:3</failure>
    </testcase>
  </testsuite>
  <testsuite name="src/format.js" tests="2" failures="1">
    <testcase name="function dollars" classname="src/format.js" file="src/format.js" line="7"/>
    <testcase name="variable CURRENCY" classname="src/format.js" file="src/format.js" line="11">
      <failure message="variable CURRENCY is undocumented" type="undocumented">src/format.js:11:1</failure>
    </testcase>
  </testsuite>
</testsuites>
```

## `lcov`

A line record for each line with items, for coverage services and editor gutters.

A line is hit when all of its items are documented. Each item is also a function, hit when it's documented.

<details>
<summary>Sample</summary>

```text
TN:docov
SF:src/cart.ts
FN:2,Item
FN:4,Item.sku
FN:5,Item.quantity
FN:14,total
FN:18,Cart
FN:22,Cart.add
FN:26,Cart.size
FNDA:1,Item
FNDA:1,Item.sku
FNDA:0,Item.quantity
FNDA:1,total
FNDA:0,Cart
FNDA:1,Cart.add
FNDA:0,Cart.size
FNF:7
FNH:4
DA:2,1
DA:4,1
DA:5,0
DA:14,1
DA:18,0
DA:22,1
DA:26,0
LH:4
LF:7
end_of_record
TN:docov
SF:src/format.js
FN:7,dollars
FN:11,CURRENCY
FNDA:1,dollars
FNDA:0,CURRENCY
FNF:2
FNH:1
DA:7,1
DA:11,0
LH:1
LF:2
end_of_record
```

</details>

## `markdown`

A summary for `$GITHUB_STEP_SUMMARY` or a pull request comment.

It has the total, any failed gates, a table of the files, and each file's undocumented items in a folded `<details>` block, then the doc checks' findings in one. In GitHub Actions, `--format markdown=$GITHUB_STEP_SUMMARY` adds it to the step's summary.

`--compare BASE.json` takes an earlier `--format json` report (from the base branch, say) and adds the percentage before and the change in points, for the total and each file, with files marked `new` or `gone`.

<details>
<summary>Sample</summary>

```markdown
## docov

**55.6%** documented (5/9)

**Failed:** 55.6% documented, under the 80% threshold

| Documented | Items | File |
| ---: | ---: | :--- |
| 57.1% | 4/7 | `src/cart.ts` |
| 50.0% | 1/2 | `src/format.js` |

<details>
<summary><code>src/cart.ts</code>: 3 undocumented</summary>

- 5:3 property `Item.quantity`
- 18:1 class `Cart`
- 26:3 accessor `Cart.size`

</details>

<details>
<summary><code>src/format.js</code>: 1 undocumented</summary>

- 11:1 variable `CURRENCY`

</details>

<details>
<summary>Doc checks: 1 found</summary>

- `src/cart.ts`:21:3 warning `require-param`: missing @param for \`item\`

</details>
```

</details>

## `rdjson`

Reviewdog's diagnostic format, for review comments.

[reviewdog](https://github.com/reviewdog/reviewdog) reads it with `reviewdog -f=rdjson`. It has a diagnostic at each undocumented item, with the code `docov/undocumented`, and one at its level for each doc check finding, with the check's rule as its code, such as `docov/require-param`.

<details>
<summary>Sample</summary>

```json
{
  "diagnostics": [
    {
      "code": {
        "value": "docov/undocumented"
      },
      "location": {
        "path": "src/cart.ts",
        "range": {
          "start": {
            "column": 3,
            "line": 5
          }
        }
      },
      "message": "property Item.quantity is undocumented",
      "severity": "WARNING"
    },
    {
      "code": {
        "value": "docov/undocumented"
      },
      "location": {
        "path": "src/cart.ts",
        "range": {
          "start": {
            "column": 1,
            "line": 18
          }
        }
      },
      "message": "class Cart is undocumented",
      "severity": "WARNING"
    },
    {
      "code": {
        "value": "docov/undocumented"
      },
      "location": {
        "path": "src/cart.ts",
        "range": {
          "start": {
            "column": 3,
            "line": 26
          }
        }
      },
      "message": "accessor Cart.size is undocumented",
      "severity": "WARNING"
    },
    {
      "code": {
        "value": "docov/undocumented"
      },
      "location": {
        "path": "src/format.js",
        "range": {
          "start": {
            "column": 1,
            "line": 11
          }
        }
      },
      "message": "variable CURRENCY is undocumented",
      "severity": "WARNING"
    },
    {
      "code": {
        "value": "docov/require-param"
      },
      "location": {
        "path": "src/cart.ts",
        "range": {
          "start": {
            "column": 3,
            "line": 21
          }
        }
      },
      "message": "missing @param for `item`",
      "severity": "WARNING"
    }
  ],
  "severity": "WARNING",
  "source": {
    "name": "docov",
    "url": "https://github.com/cutenode/docov"
  }
}
```

</details>

## `sarif`

SARIF 2.1.0, for GitHub code scanning and other static analysis viewers.

It has a result at each undocumented item under the rule `docov/undocumented`, and one at its level for each doc check finding under the check's own rule, such as `docov/require-param`. `github/codeql-action/upload-sarif` uploads it to code scanning.

<details>
<summary>Sample</summary>

```json
{
  "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
  "runs": [
    {
      "results": [
        {
          "level": "warning",
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": {
                  "uri": "src/cart.ts"
                },
                "region": {
                  "startColumn": 3,
                  "startLine": 5
                }
              }
            }
          ],
          "message": {
            "text": "property Item.quantity is undocumented"
          },
          "ruleId": "docov/undocumented",
          "ruleIndex": 0
        },
        {
          "level": "warning",
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": {
                  "uri": "src/cart.ts"
                },
                "region": {
                  "startColumn": 1,
                  "startLine": 18
                }
              }
            }
          ],
          "message": {
            "text": "class Cart is undocumented"
          },
          "ruleId": "docov/undocumented",
          "ruleIndex": 0
        },
        {
          "level": "warning",
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": {
                  "uri": "src/cart.ts"
                },
                "region": {
                  "startColumn": 3,
                  "startLine": 26
                }
              }
            }
          ],
          "message": {
            "text": "accessor Cart.size is undocumented"
          },
          "ruleId": "docov/undocumented",
          "ruleIndex": 0
        },
        {
          "level": "warning",
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": {
                  "uri": "src/format.js"
                },
                "region": {
                  "startColumn": 1,
                  "startLine": 11
                }
              }
            }
          ],
          "message": {
            "text": "variable CURRENCY is undocumented"
          },
          "ruleId": "docov/undocumented",
          "ruleIndex": 0
        },
        {
          "level": "warning",
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": {
                  "uri": "src/cart.ts"
                },
                "region": {
                  "startColumn": 3,
                  "startLine": 21
                }
              }
            }
          ],
          "message": {
            "text": "missing @param for `item`"
          },
          "ruleId": "docov/require-param",
          "ruleIndex": 1
        }
      ],
      "tool": {
        "driver": {
          "informationUri": "https://github.com/cutenode/docov",
          "name": "docov",
          "rules": [
            {
              "defaultConfiguration": {
                "level": "warning"
              },
              "id": "docov/undocumented",
              "name": "Undocumented",
              "shortDescription": {
                "text": "A declaration without JSDoc"
              }
            },
            {
              "id": "docov/require-param",
              "shortDescription": {
                "text": "Each parameter of a function or method has a matching @param."
              }
            }
          ],
          "version": "X.Y.Z"
        }
      }
    }
  ],
  "version": "2.1.0"
}
```

</details>

## `shields`

A shields.io endpoint badge's JSON, for a badge of the total from a served file.

Publish the file and point a [shields.io endpoint badge](https://shields.io/badges/endpoint-badge) at its URL. Its color is the one `badge` picks.

```json
{"color":"red","label":"docs","message":"55.6%","schemaVersion":1}
```

## `text`

Each file's percentage and its undocumented items, then the total. It's the default.

`--omit-covered` leaves out fully documented files, and `--quiet` leaves out every file, so only the total and the totals after it are left. Failed gates go to stderr either way. `--sort priority` lists each file's undocumented items by what to document first: exported functions and methods (more parameters first), then classes, then the rest.

On a terminal, a percentage under the global threshold is red and the undocumented items are dim. `--color always` or `--color never` says otherwise, and `NO_COLOR` turns the colors off unless `--color always` is given. Written to a file, the report has no colors.

```text
  57.1%  4/7  src/cart.ts
         src/cart.ts:5:3  property Item.quantity
         src/cart.ts:18:1  class Cart
         src/cart.ts:26:3  accessor Cart.size
  50.0%  1/2  src/format.js
         src/format.js:11:1  variable CURRENCY
  55.6%  5/9  total
```

## `unix`

A line per undocumented item and per doc check finding, the way compilers write warnings, for editors and grep.

An undocumented item's line is `path:line:column: kind name undocumented [docov]`, and a finding's is `path:line:column: rule-id: message`, which editors' problem matchers can read.

```text
src/cart.ts:5:3: property Item.quantity undocumented [docov]
src/cart.ts:18:1: class Cart undocumented [docov]
src/cart.ts:26:3: accessor Cart.size undocumented [docov]
src/format.js:11:1: variable CURRENCY undocumented [docov]
src/cart.ts:21:3: require-param: missing @param for `item`
```
