<!-- Generated from src/check/ and tests/fixtures/checks/ by tests/docs.rs: run `DOCOV_BLESS=1 cargo test --test docs`. -->

# Doc checks

Checks on what each counted item's JSDoc block says, past whether it has one. Each is off until docov.json's `rules` or `--rule ID=LEVEL` (repeatable) turns it on, at `warn` or `error`, with its options if it has some:

```jsonc
"rules": {
  "require-param": "error",
  "param-names": "warn",
  "summary-sentence": ["warn", { "maxLength": 80 }],
}
```

Each finding prints to stderr as `docov: path:line:column: rule-id: message`, and the `json`, `github`, `markdown`, `sarif`, `gitlab`, `junit`, `checkstyle`, `rdjson`, `unix` and `html` [formats](formats.md) list it too. A finding at `error` fails the scan (exit 1). A flag's level wins over the config's, which keeps its options. A config's `rules` merges by rule id over those of a config it extends or nests under, the way ESLint's flat config does: its setting for a rule replaces that one's, keeping its options when it gives a level alone, and that one's other rules stay.

An item a `docov-ignore` comment takes out of the total isn't checked. The checks that want a tag, and `summary-sentence`, leave a block that inherits its docs (`@inheritdoc` or `{@inheritDoc}`) alone. `const f = () => …` is a variable to docov, so the checks on parameters, returns and throws don't look at it.

Each example below is a file docov scans with that one check at `error`, and what it prints.

| Rule | What it wants |
| --- | --- |
| [`doc-markdown`](#doc-markdown) | A description's code is in backticks, and its lists have no lazy continuation lines. |
| [`empty-tag-description`](#empty-tag-description) | Each @param, @typeParam, @returns, @throws and @yields tag has a description. |
| [`informative-docs`](#informative-docs) | A description says more than its item's name. |
| [`param-names`](#param-names) | Each @param names a parameter of its function or method. |
| [`require-param`](#require-param) | Each parameter of a function or method has a matching @param. |
| [`require-returns`](#require-returns) | A function or method that returns a value has @returns, unless its return type is void. |
| [`require-template`](#require-template) | Each type parameter of a declaration has a @template or @typeParam. |
| [`require-throws`](#require-throws) | An exported function or method that throws has @throws. |
| [`require-yields`](#require-yields) | A generator that yields a value has @yields. |
| [`summary-sentence`](#summary-sentence) | A description's first sentence is short and ends with a period. |
| [`tsdoc-code`](#tsdoc-code) | Each TSDoc code span and code fence is closed and well formed. |
| [`tsdoc-escapes`](#tsdoc-escapes) | TSDoc text escapes a {, } or > that isn't markup and an @ that isn't a tag, and a backslash escapes only punctuation. |
| [`tsdoc-html`](#tsdoc-html) | Each HTML tag in a TSDoc block is well formed, and an element the file's tsdoc.json supports. |
| [`tsdoc-inline-tags`](#tsdoc-inline-tags) | Each TSDoc tag has a valid name with a space after it, and each inline tag is closed with no unescaped { inside. |
| [`tsdoc-links`](#tsdoc-links) | Each TSDoc {@link} has a URL or a declaration reference in TSDoc's grammar, and text only after a |. |
| [`tsdoc-params`](#tsdoc-params) | Each TSDoc @param and @typeParam has a valid name and a hyphen before its description, without a {type}. |
| [`tsdoc-tags`](#tsdoc-tags) | Each TSDoc tag is defined and supported by the file's tsdoc.json, used as its kind, and repeated only with allowMultiple. |
| [`valid-link`](#valid-link) | Each {@link} is well formed, and a target that's a name is declared or imported in its file. |

## `doc-markdown`

A description's code is in backticks, and its Markdown lists have no lazy continuation lines, as clippy's doc_markdown and doc_lazy_continuation have it. A word outside backticks, inline tags and code blocks that looks like code (`snake_case`, `a::b`, `camelCase` or `PascalCase`) is a finding unless it's in the allowlist, which adds to a default one of words like `JavaScript` and `GitHub`.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowlist` | array of string | `[]` | Words that look like code and aren't, on top of the default ones. |

Fails:

```ts
/** Fetches `url`, trying again up to maxRetries times when it fails. */
export async function fetchWithRetry(url: string, maxRetries = 3): Promise<Response> {
  for (let attempt = 0; ; attempt++) {
    try {
      return await fetch(url);
    } catch (error) {
      if (attempt >= maxRetries) throw error;
    }
  }
}
```

```text
docov: example.ts:1:39: doc-markdown: `maxRetries` is code, so put it in backticks
docov: the doc checks found 1 error
```

Passes:

```ts
/** Fetches `url`, trying again up to `maxRetries` times when it fails. */
export async function fetchWithRetry(url: string, maxRetries = 3): Promise<Response> {
  for (let attempt = 0; ; attempt++) {
    try {
      return await fetch(url);
    } catch (error) {
      if (attempt >= maxRetries) throw error;
    }
  }
}
```

## `empty-tag-description`

Each `@param`, `@typeParam`, `@returns`, `@throws` and `@yields` tag (or `@arg`, `@argument`, `@return`, `@exception` or `@yield`) says something past its type and name, and past the `-` TSDoc puts before a description, so `@param x -` says nothing. A `@returns` of a function that returns nothing (by its return type or the tag's: `void`, `undefined` or `Promise<void>`) may say nothing.

Fails:

```ts
/**
 * Formats a price for display.
 *
 * @param cents - The price in cents.
 * @param currency
 * @returns The formatted price, like `$4.20`.
 */
export function formatPrice(cents: number, currency: string): string {
  return new Intl.NumberFormat("en-US", { style: "currency", currency }).format(cents / 100);
}
```

```text
docov: example.ts:5:4: empty-tag-description: @param `currency` has no description
docov: the doc checks found 1 error
```

Passes:

```ts
/**
 * Formats a price for display.
 *
 * @param cents - The price in cents.
 * @param currency - An ISO 4217 currency code, like `USD`.
 * @returns The formatted price, like `$4.20`.
 */
export function formatPrice(cents: number, currency: string): string {
  return new Intl.NumberFormat("en-US", { style: "currency", currency }).format(cents / 100);
}
```

## `informative-docs`

A description says more than its item's name, as eslint-plugin-jsdoc's informative-docs has it: `/** The user name. */` on `userName` restates it. Words are compared without case, split at camelCase and separators, and past filler words.

Fails:

```ts
/** Gets the display name. */
export function getDisplayName(user: { name?: string; email: string }): string {
  return user.name ?? user.email;
}
```

```text
docov: example.ts:1:5: informative-docs: the description only restates the name `getDisplayName`
docov: the doc checks found 1 error
```

Passes:

```ts
/** The name to show for a user: theirs, or their email when they haven't set one. */
export function getDisplayName(user: { name?: string; email: string }): string {
  return user.name ?? user.email;
}
```

## `param-names`

Each `@param` names one of its function's parameters, so none is left over from a rename. A destructured parameter takes the name of the `@param` at its place, and a dotted name (`@param options.a`) belongs to its root.

Fails:

```ts
/**
 * Posts a message to a chat channel.
 *
 * @param channelId - The channel to post in.
 * @param text - What the message says.
 */
export async function post(channel: string, text: string): Promise<void> {
  await fetch(`/api/channels/${channel}/messages`, { method: "POST", body: text });
}
```

```text
docov: example.ts:4:11: param-names: @param `channelId` doesn't match a parameter
docov: the doc checks found 1 error
```

Passes:

```ts
/**
 * Posts a message to a chat channel.
 *
 * @param channel - The channel to post in.
 * @param text - What the message says.
 */
export async function post(channel: string, text: string): Promise<void> {
  await fetch(`/api/channels/${channel}/messages`, { method: "POST", body: text });
}
```

## `require-param`

Each of a function's or method's parameters has a matching `@param`, by name, or for a destructured one, by place, as `paramsDocumented` counts them.

Fails:

```ts
/**
 * Clamps a number to a range.
 *
 * @param value - The number to clamp.
 * @param min - The lowest it may be.
 * @returns `value`, or the bound it's past.
 */
export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}
```

```text
docov: example.ts:1:1: require-param: missing @param for `max`
docov: the doc checks found 1 error
```

Passes:

```ts
/**
 * Clamps a number to a range.
 *
 * @param value - The number to clamp.
 * @param min - The lowest it may be.
 * @param max - The highest it may be.
 * @returns `value`, or the bound it's past.
 */
export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}
```

## `require-returns`

A function or method whose own body returns a value (`return x;`, or an arrow function's expression body) has `@returns` (or `@return`). One whose return type is `void`, `undefined` or `Promise<void>` needs none.

Fails:

```js
/**
 * Splits a full name into its parts.
 *
 * @param {string} name The full name.
 */
export function splitName(name) {
  return name.trim().split(/\s+/);
}
```

```text
docov: example.js:1:1: require-returns: missing @returns
docov: the doc checks found 1 error
```

Passes:

```js
/**
 * Splits a full name into its parts.
 *
 * @param {string} name The full name.
 * @returns {string[]} Each word of the name, in order.
 */
export function splitName(name) {
  return name.trim().split(/\s+/);
}
```

## `require-template`

A declaration with type parameters has a `@template`, or TSDoc's `@typeParam`, for each, as in `@template T`, `@template T, U`, `@template {string} K` or `@typeParam T - The key.` A finding names the tag the config's `syntax` writes.

Fails:

```ts
/**
 * Gets the first item of a list.
 *
 * @param items - The list.
 * @returns Its first item, or `undefined` when it's empty.
 */
export function first<T>(items: T[]): T | undefined {
  return items[0];
}
```

```text
docov: example.ts:1:1: require-template: missing @template for `T`
docov: the doc checks found 1 error
```

Passes:

```ts
/**
 * Gets the first item of a list.
 *
 * @template T The type of the list's items.
 * @param items - The list.
 * @returns Its first item, or `undefined` when it's empty.
 */
export function first<T>(items: T[]): T | undefined {
  return items[0];
}
```

## `require-throws`

An exported function or method whose own body has a `throw` (outside a `try` with a `catch`, and not in a function inside it) has `@throws` (or `@exception`).

Fails:

```ts
/**
 * Parses a port number.
 *
 * @param value - The port, as text.
 * @returns The port.
 */
export function parsePort(value: string): number {
  const port = Number(value);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new RangeError(`not a port: ${value}`);
  }
  return port;
}
```

```text
docov: example.ts:1:1: require-throws: missing @throws
docov: the doc checks found 1 error
```

Passes:

```ts
/**
 * Parses a port number.
 *
 * @param value - The port, as text.
 * @returns The port.
 * @throws {RangeError} When it isn't a whole number from 1 to 65535.
 */
export function parsePort(value: string): number {
  const port = Number(value);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new RangeError(`not a port: ${value}`);
  }
  return port;
}
```

## `require-yields`

A generator whose own body yields a value has `@yields` (or `@yield`).

Fails:

```ts
/**
 * Reads a text one line at a time.
 *
 * @param text - The text to read.
 */
export function* lines(text: string): Generator<string> {
  for (const line of text.split("\n")) {
    yield line;
  }
}
```

```text
docov: example.ts:1:1: require-yields: missing @yields
docov: the doc checks found 1 error
```

Passes:

```ts
/**
 * Reads a text one line at a time.
 *
 * @param text - The text to read.
 * @yields Each line, without its line break.
 */
export function* lines(text: string): Generator<string> {
  for (const line of text.split("\n")) {
    yield line;
  }
}
```

## `summary-sentence`

A description's first sentence, its summary, is at most `maxLength` characters, and with `period` on, ends with `.`, `!` or `?`. The summary ends at the first of those followed by a space, or else with the first paragraph. A block that inherits its docs (`{@inheritDoc Base.m}`) takes its summary from where it inherits, so it's left be.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `maxLength` | number | `120` | The most characters the summary may have. |
| `period` | boolean | `true` | Whether the summary ends with `.`, `!` or `?`. |

Fails:

```ts
/**
 * Converts a temperature from Celsius to Fahrenheit
 *
 * @param celsius - The temperature in degrees Celsius.
 * @returns The temperature in degrees Fahrenheit.
 */
export function toFahrenheit(celsius: number): number {
  return (celsius * 9) / 5 + 32;
}
```

```text
docov: example.ts:2:4: summary-sentence: the summary doesn't end with a period
docov: the doc checks found 1 error
```

Passes:

```ts
/**
 * Converts a temperature from Celsius to Fahrenheit.
 *
 * @param celsius - The temperature in degrees Celsius.
 * @returns The temperature in degrees Fahrenheit.
 */
export function toFahrenheit(celsius: number): number {
  return (celsius * 9) / 5 + 32;
}
```

## `tsdoc-code`

Code spans and code fences are closed and well formed, as `@microsoft/tsdoc` reads them: a code span has something between its backticks and ends on its line, and a code fence's backticks start their lines, unindented, with no backtick in its language and nothing after the closing ones.

Fails:

````ts
/**
 * Runs `npm test.
 *
 * @example
 *   ```sh
 *   npm test
 *   ```
 */
export function runTests(): void {}
````

```text
docov: example.ts:2:9: tsdoc-code: tsdoc-code-span-missing-delimiter: The code span is missing its closing backtick
docov: example.ts:5:6: tsdoc-code: tsdoc-code-fence-opening-indent: The opening backtick for a code fence must appear at the start of the line
docov: example.ts:7:6: tsdoc-code: tsdoc-code-fence-opening-indent: The opening backtick for a code fence must appear at the start of the line
docov: the doc checks found 3 errors
```

Passes:

````ts
/**
 * Runs `npm test`.
 *
 * @example
 * ```sh
 * npm test
 * ```
 */
export function runTests(): void {}
````

## `tsdoc-escapes`

Text escapes what TSDoc would read as markup, as `@microsoft/tsdoc` reads it: a `{` that doesn't start an inline tag, a `}` and a `>` outside one, and an `@` in a word or without a tag name. A backslash escapes only punctuation.

Fails:

```ts
/**
 * Formats a user as {name} <email>, or name@host.
 */
export function formatUser(name: string, email: string): string {
  return `${name} <${email}>`;
}
```

```text
docov: example.ts:2:22: tsdoc-escapes: tsdoc-malformed-inline-tag: Expecting a TSDoc tag starting with "{@"
docov: example.ts:2:27: tsdoc-escapes: tsdoc-escape-right-brace: The "}" character should be escaped using a backslash to avoid confusion with a TSDoc inline tag
docov: example.ts:2:45: tsdoc-escapes: tsdoc-at-sign-in-word: The "@" character looks like part of a TSDoc tag; use a backslash to escape it
docov: the doc checks found 3 errors
```

Passes:

```ts
/**
 * Formats a user as \{name\} \<email\>, or name\@host.
 */
export function formatUser(name: string, email: string): string {
  return `${name} <${email}>`;
}
```

## `tsdoc-html`

HTML tags are well formed, as `@microsoft/tsdoc` reads them, and when the file's tsdoc.json has `supportedHtmlElements` (or `reportUnsupportedHtmlElements`), name only the elements it lists. Like TSDoc, it holds attribute names to that list too.

Fails:

```ts
/**
 * Renders the banner, <b class=bold>in bold</b>.
 */
export function renderBanner(): string {
  return "<b>banner</b>";
}
```

```text
docov: example.ts:2:24: tsdoc-html: tsdoc-html-tag-missing-string: The HTML element has an invalid attribute: Expecting an HTML string starting with a single-quote or double-quote character
docov: the doc checks found 1 error
```

Passes:

```ts
/**
 * Renders the banner, <b class="bold">in bold</b>.
 */
export function renderBanner(): string {
  return "<b>banner</b>";
}
```

## `tsdoc-inline-tags`

Tags are well formed, as `@microsoft/tsdoc` reads them. A tag's name is a letter then letters and digits, with a space after a block tag's and an inline tag's, and an inline tag `{@name …}` is closed, with any `{` inside escaped. `{@inheritDoc}` has nothing past its declaration reference.

Fails:

```ts
/**
 * Parses a date, as {@link Date.parse does.
 *
 * @returns The time.
 */
export function parseDate(text: string): number {
  return Date.parse(text);
}
```

```text
docov: example.ts:2:22: tsdoc-inline-tags: tsdoc-inline-tag-missing-right-brace: The TSDoc inline tag name is missing its closing "}"
docov: the doc checks found 1 error
```

Passes:

```ts
/**
 * Parses a date, as {@link Date.parse} does.
 *
 * @returns The time.
 */
export function parseDate(text: string): number {
  return Date.parse(text);
}
```

## `tsdoc-links`

Each `{@link}` has a URL with a scheme (`https://…`) or a declaration reference in TSDoc's grammar (`pkg#Class.(member:static)`), then nothing but its text after a `|`, and each `{@inheritDoc}` reference follows that grammar too, as `@microsoft/tsdoc` reads them. `valid-link` checks JSDoc's links, and looks up their names.

Fails:

```ts
/**
 * Retries a request, as {@link Client.send the client} does. See {@link www.example.com}.
 */
export function retry(attempts: number): number {
  return attempts;
}
```

```text
docov: example.ts:2:27: tsdoc-links: tsdoc-reference-missing-dot: Expecting a period before the next component of a declaration reference
docov: the doc checks found 1 error
```

Passes:

```ts
/**
 * Retries a request, as {@link Client.send | the client} does. See {@link https://example.com}.
 */
export function retry(attempts: number): number {
  return attempts;
}
```

## `tsdoc-params`

Each `@param` and `@typeParam` names a parameter with a valid identifier and puts ` - ` before its description (`tsdoc-param-tag-missing-hyphen`), with no JSDoc-style `{type}` or `[optional]` name, as `@microsoft/tsdoc` reads them.

Fails:

```ts
/**
 * Joins two paths.
 *
 * @param {string} base - The folder.
 * @param rest The path inside it.
 */
export function join(base: string, rest: string): string {
  return `${base}/${rest}`;
}
```

```text
docov: example.ts:4:11: tsdoc-params: tsdoc-param-tag-with-invalid-type: The @param block should not include a JSDoc-style '{type}'
docov: example.ts:5:4: tsdoc-params: tsdoc-param-tag-missing-hyphen: The @param block should be followed by a parameter name and then a hyphen
docov: the doc checks found 2 errors
```

Passes:

```ts
/**
 * Joins two paths.
 *
 * @param base - The folder.
 * @param rest - The path inside it.
 */
export function join(base: string, rest: string): string {
  return `${base}/${rest}`;
}
```

## `tsdoc-tags`

Each tag is one the file's tsdoc.json defines (`tsdoc-undefined-tag`) and supports, when its `supportForTags` says which it does (`tsdoc-unsupported-tag`), and is used as the kind it's defined as: an inline tag in braces, a block or modifier tag not (`tsdoc-tag-should-not-have-braces`, `tsdoc-inline-tag-missing-braces`). A tag without `allowMultiple` is used once (`docov-repeated-tag`), and a modifier tag has nothing after it on its line (`docov-modifier-tag-text`), which TSDoc would put in the section before it; TSDoc itself checks neither, so their ids are docov's. It also has TSDoc's checks of particular tags: `@deprecated` needs a message, and `{@inheritDoc}` comes once, with no summary or `@remarks`.

Fails:

```ts
/**
 * Reads the config file.
 *
 * @remarks Reads it once.
 * @remarks And caches it.
 * @internal Only the CLI calls it.
 * @since 2.0
 */
export function readConfig(path: string): string {
  return path;
}
```

```text
docov: example.ts:5:4: tsdoc-tags: docov-repeated-tag: The TSDoc tag "@remarks" may only appear once in a doc comment
docov: example.ts:6:4: tsdoc-tags: docov-modifier-tag-text: The TSDoc tag "@internal" is a modifier tag; text after it on its line belongs to the section before it
docov: example.ts:7:4: tsdoc-tags: tsdoc-undefined-tag: The TSDoc tag "@since" is not defined in this configuration
docov: the doc checks found 3 errors
```

Passes:

```ts
/**
 * Reads the config file.
 *
 * @remarks Reads it once, and caches it.
 * @internal
 */
export function readConfig(path: string): string {
  return path;
}
```

## `valid-link`

Each `{@link}`, `{@linkcode}` and `{@linkplain}` is closed, has no unescaped `{` inside, and has a target, and each `@see` has text. A target that's a plain name, or a namepath (`A#b`, `A.b`, `A~b`) by its first part, names something the file declares or imports, a type a block there declares, or a JavaScript, browser or Node.js global. URLs, `module:` paths, TSDoc's declaration references (`(A:constructor)`, `@scope/pkg#A`) and other targets aren't looked up, nor are names in other files. With `"syntax": "tsdoc"`, the `A` in `A#b` is a package (`pkg#Foo.bar`), so it isn't either.

Fails:

```ts
/** Opens a socket to `url`. Close it with {@link disconnect} when you're done. */
export function openSocket(url: string): WebSocket {
  return new WebSocket(url);
}

/** Closes a socket that {@link openSocket} opened. */
export function closeSocket(socket: WebSocket): void {
  socket.close();
}
```

```text
docov: example.ts:1:44: valid-link: `disconnect` isn't declared or imported in this file
docov: the doc checks found 1 error
```

Passes:

```ts
/** Opens a socket to `url`. Close it with {@link closeSocket} when you're done. */
export function openSocket(url: string): WebSocket {
  return new WebSocket(url);
}

/** Closes a socket that {@link openSocket} opened. */
export function closeSocket(socket: WebSocket): void {
  socket.close();
}
```
