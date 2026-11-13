#!/usr/bin/env node
// Drops repeated entries from each open release PR's changelog (release/v* in this repository).
// uppt/pr lists every commit since the last tag and has no setting to leave any out, so a commit
// that reached main under two hashes is listed twice. A stack of pull requests does that: GitHub
// rebases the next one as its base merges, while the ones above it keep the old commits. uppt/pr
// rewrites the changelog on every run, so release.yml runs this after it, and uppt/release copies
// the PR's body to the GitHub release.
//
// Needs GH_TOKEN, allowed to read the repository and write pull requests.
import { execFileSync } from "node:child_process";
import { pathToFileURL } from "node:url";

// What uppt puts after an entry that has no pull request to link to.
const commitLink = / \(\[`[0-9a-f]+`\]\(\S+\/commit\/[0-9a-f]+\)\)$/;

/** The body without the commit entries that repeat one above them in the same section. */
export function dropRepeatedEntries(body) {
	const seen = new Set();
	const kept = [];
	for (const line of body.split("\n")) {
		if (line.startsWith("### ")) seen.clear();
		const entry = line.replace(commitLink, "");
		const isCommitEntry = entry !== line;
		if (isCommitEntry && seen.has(entry)) continue;
		if (isCommitEntry) seen.add(entry);
		kept.push(line);
	}
	return kept.join("\n");
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
	const listArgs = [
		"pr",
		"list",
		"--state",
		"open",
		"--json",
		"number,headRefName,isCrossRepository,body",
	];
	const prs = JSON.parse(execFileSync("gh", listArgs, { encoding: "utf8" }));
	for (const { number, headRefName, isCrossRepository, body } of prs) {
		if (isCrossRepository || !headRefName.startsWith("release/v")) continue;
		const deduped = dropRepeatedEntries(body);
		if (deduped === body) {
			console.log(`#${number}: no repeated entries`);
			continue;
		}
		const patchArgs = [
			"api",
			"--method",
			"PATCH",
			`repos/{owner}/{repo}/pulls/${number}`,
			"--input",
			"-",
			"--silent",
		];
		execFileSync("gh", patchArgs, { input: JSON.stringify({ body: deduped }) });
		console.log(
			`#${number}: dropped ${body.split("\n").length - deduped.split("\n").length} repeated entries`,
		);
	}
}
