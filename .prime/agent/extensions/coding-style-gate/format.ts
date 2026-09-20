import type { StyleReviewReport } from "./reviewer";

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
		"Inspect each finding and fix the code or explain why the rule does not apply before completing the task.",
	].join("\n");
}
