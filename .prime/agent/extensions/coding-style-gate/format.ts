import type { StyleReviewReport } from "./reviewer";

export function formatReviewFailure(message: string): string {
	return `${message}
Before handing back, resolve the review failure and rerun the gate, or explicitly report that review is blocked and why. A failed review has no findings to accept; do not claim that the code passed.`;
}

function plural(count: number, singular: string, pluralForm = `${singular}s`): string {
	return count === 1 ? singular : pluralForm;
}

export function formatReview(report: StyleReviewReport): string {
	if (report.findings.length === 0) {
		return `coding-style-gate found no likely violations in ${report.filesReviewed} Rust ${plural(report.filesReviewed, "file")} (${report.model}).`;
	}

	const lines = report.findings.map(
		(finding) =>
			`- ${finding.file} | ${finding.rule.section} / ${finding.rule.title} (${finding.probability.toFixed(2)}): ${finding.rule.violation.replace(/\s+/g, " ")}`,
	);
	return [
		`coding-style-gate found ${report.findings.length} likely ${plural(report.findings.length, "violation")} in ${report.filesReviewed} Rust ${plural(report.filesReviewed, "file")} (${report.model}).`,
		...lines,
		"Before handing back, address every finding: either fix the code and recheck it, or explicitly accept the finding in your handoff with its file, rule, and reason (including why a suspected false positive does not apply).",
		"Do not silently ignore findings or describe accepted findings as a clean review. In enforce mode, acceptance in your handoff does not clear the block; the existing /coding-style-gate override <reason> mechanism is still required.",
	].join("\n");
}
