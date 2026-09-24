export interface ScoredCase {
	ruleId: string;
	split: "train" | "validation";
	expectedViolation: boolean;
	probability: number;
}

export function measure(rows: readonly ScoredCase[], threshold: number) {
	return {
		truePositive: rows.filter(row => row.expectedViolation && row.probability >= threshold).length,
		falseNegative: rows.filter(row => row.expectedViolation && row.probability < threshold).length,
		trueNegative: rows.filter(row => !row.expectedViolation && row.probability < threshold).length,
		falsePositive: rows.filter(row => !row.expectedViolation && row.probability >= threshold).length,
	};
}

export function fitRuleThresholds(ruleIds: readonly string[], rows: readonly ScoredCase[]) {
	const fitted: Record<string, { threshold: number; separable: boolean; training: ReturnType<typeof measure> }> = {};
	for (const row of rows) {
		if (!Number.isFinite(row.probability) || row.probability < 0 || row.probability > 1) {
			throw new Error(`Invalid calibration probability for ${row.ruleId}`);
		}
	}
	for (const ruleId of ruleIds) {
		const training = rows.filter(row => row.ruleId === ruleId && row.split === "train");
		const positive = training.filter(row => row.expectedViolation).map(row => row.probability);
		const negative = training.filter(row => !row.expectedViolation).map(row => row.probability);
		if (!positive.length || !negative.length) {
			throw new Error(`Calibration requires both labels for ${ruleId}`);
		}
		const scores = [...new Set(training.map(row => row.probability))].sort((a, b) => a - b);
		const candidates = [0, 1, ...scores.slice(1).map((score, index) => (score + scores[index]!) / 2)];
		const ranked = candidates.map(threshold => {
			const counts = measure(training, threshold);
			return {
				threshold,
				counts,
				loss: 2 * counts.falsePositive + counts.falseNegative,
				margin: Math.min(...scores.map(score => Math.abs(score - threshold))),
			};
		}).sort((a, b) => a.loss - b.loss || a.counts.falsePositive - b.counts.falsePositive || b.margin - a.margin || b.threshold - a.threshold);
		const chosen = ranked[0]!;
		fitted[ruleId] = {
			threshold: chosen.threshold,
			separable: Math.max(...negative) < Math.min(...positive),
			training: chosen.counts,
		};
	}
	return fitted;
}
