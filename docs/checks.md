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
