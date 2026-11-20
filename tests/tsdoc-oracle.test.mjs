// docov's tsdoc-* findings, and the text tsdocJsdocTypes has TSDoc read, checked against
// @microsoft/tsdoc, the parser tsdocrs ports. tests/props/tsdoc.rs writes each block docov checks
// into tests/fixtures/tsdoc/jsdoc-types-corpus.json, with that text and docov's findings with the
// setting off and on.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { TSDocConfiguration, TSDocParser } from "@microsoft/tsdoc";
import { TSDocConfigFile } from "@microsoft/tsdoc-config";

const corpus = JSON.parse(
	readFileSync(new URL("fixtures/tsdoc/jsdoc-types-corpus.json", import.meta.url), "utf8"),
);

// What @microsoft/tsdoc logs for `text`, read with the tsdoc.json keys `config`.
function messages(text, config) {
	const configuration = new TSDocConfiguration();
	if (config) {
		const file = TSDocConfigFile.loadFromObject({
			$schema: "https://developer.microsoft.com/json-schemas/tsdoc/v0/tsdoc.schema.json",
			...config,
		});
		assert.ok(!file.hasErrors, file.getErrorSummary());
		file.configureParser(configuration);
	}
	return new TSDocParser(configuration).parseString(text).log.messages;
}

// The 1-based line and column, in characters, of UTF-16 offset `at` in `text`, as docov counts them.
function position(text, at) {
	const lines = text.slice(0, at).split("\n");
	return [lines.length, [...lines.at(-1)].length + 1];
}

// [line, column, message id] for each of `found`, in order.
function placed(text, found) {
	return found.map((m) => [...position(text, m.textRange.pos), m.messageId]).toSorted(byPlace);
}

function byPlace(a, b) {
	return a[0] - b[0] || a[1] - b[1] || a[2].localeCompare(b[2]);
}

// Each entry `check` finds something wrong with, and what.
function failures(check) {
	return corpus.flatMap((entry) => {
		const wrong = check(entry);
		return wrong ? [{ comment: entry.comment, ...wrong }] : [];
	});
}

// docov's findings in `text` unless they're @microsoft/tsdoc's.
function mismatch(text, config, docov) {
	const microsoft = placed(text, messages(text, config));
	docov = docov.toSorted(byPlace);
	return JSON.stringify(microsoft) !== JSON.stringify(docov) && { text, microsoft, docov };
}

test("without tsdocJsdocTypes, docov reports what @microsoft/tsdoc does", () => {
	assert.deepEqual(
		failures(({ comment, config, off }) => mismatch(comment, config, off)),
		[],
	);
});

test("with it, docov reports what @microsoft/tsdoc does in the text it has TSDoc read", () => {
	assert.deepEqual(
		failures(({ blanked, config, on }) => mismatch(blanked, config, on)),
		[],
	);
});

test("@microsoft/tsdoc reads a generated block's text as it reads the block without JSDoc's syntax", () => {
	assert.deepEqual(
		failures(({ blanked, plain }) => {
			if (plain === undefined) return false;
			const [read, wanted] = [blanked, plain].map((text) => placed(text, messages(text)));
			return JSON.stringify(read) !== JSON.stringify(wanted) && { blanked, plain, read, wanted };
		}),
		[],
	);
});

test("@microsoft/tsdoc reports nothing where that text blanked JSDoc's syntax", () => {
	assert.deepEqual(
		failures(({ comment, blanked, config }) => {
			const changed = messages(blanked, config).filter(
				(m) => comment[m.textRange.pos] !== blanked[m.textRange.pos],
			);
			return changed.length > 0 && { blanked, at: placed(blanked, changed) };
		}),
		[],
	);
});

// A block it blanks nothing in keeps what isn't JSDoc's syntax, like a {type} after a @param's
// name, and TSDoc reports it as before.
test("where it blanks a block, it leaves @microsoft/tsdoc no JSDoc type or optional name to report", () => {
	const jsdoc = ["tsdoc-param-tag-with-invalid-type", "tsdoc-param-tag-with-invalid-optional-name"];
	assert.deepEqual(
		failures(({ comment, blanked, config }) => {
			if (blanked === comment) return false;
			const left = messages(blanked, config).filter((m) => jsdoc.includes(m.messageId));
			return left.length > 0 && { blanked, left: placed(blanked, left) };
		}),
		[],
	);
});
