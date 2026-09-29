import type { StyleFinding, StyleReviewReport } from "./reviewer";

export interface GateReviewReport extends StyleReviewReport {
	/** Findings cleared by a documented file, rule, and reason. `findings` holds only unaccepted findings. */
	acceptedFindings?: StyleFinding[];
	/** Dispositions-file errors. An invalid file or entry accepts no findings. */
	dispositionProblems?: string[];
}

export function formatReviewFailure(message: string): string {
	return `${message}
Before handing back, resolve the review failure and rerun the gate, or explicitly report that review is blocked and why. A failed review has no findings to accept; do not claim that the code passed.`;
}

function plural(count: number, singular: string, pluralForm = `${singular}s`): string {
	return count === 1 ? singular : pluralForm;
}

export function formatReview(report: GateReviewReport, dispositionsFile: string): string {
	const accepted = report.acceptedFindings?.length ?? 0;
	const problems = (report.dispositionProblems ?? []).map((problem) => `coding-style-gate dispositions problem: ${problem}`);
	const acceptedLine = `${accepted} ${plural(accepted, "finding was", "findings were")} accepted by documented dispositions in ${dispositionsFile}. Accepted findings are not a clean review.`;
	if (report.findings.length === 0) {
		if (accepted === 0) {
			return `coding-style-gate found no likely violations in ${report.filesReviewed} Rust ${plural(report.filesReviewed, "file")} (${report.model}).`;
		}
		return [
			`coding-style-gate found no unaccepted likely violations in ${report.filesReviewed} Rust ${plural(report.filesReviewed, "file")} (${report.model}).`,
			acceptedLine,
			...problems,
		].join("\n");
	}

	const lines = report.findings.map(
		(finding) =>
			`- ${finding.file} | ${finding.rule.section} / ${finding.rule.title} (${finding.probability.toFixed(2)}): ${finding.rule.violation.replace(/\s+/g, " ")}`,
	);
	return [
		`coding-style-gate found ${report.findings.length} likely ${plural(report.findings.length, "violation")} in ${report.filesReviewed} Rust ${plural(report.filesReviewed, "file")} (${report.model}).`,
		...lines,
		...(accepted > 0 ? [acceptedLine] : []),
		...problems,
		`Before handing back, address every finding: either fix the code and recheck it, or explicitly accept the finding by recording its file, rule, and reason in ${dispositionsFile} (including why a suspected false positive does not apply).`,
		"Do not silently ignore findings or describe accepted findings as a clean review. In enforce mode, a disposition recorded in the dispositions file clears the block for that finding; acceptance only in your handoff does not clear the block. The existing /coding-style-gate override <reason> mechanism still applies.",
	].join("\n");
}
