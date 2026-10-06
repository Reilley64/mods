import { createTwoFilesPatch } from "diff";
import { ruleAppliesTo, type StyleRule } from "../rules";
import type { RustChange } from "../snapshot";

interface CalibrationSample {
	name: string;
	path?: string;
	before: string;
	after: string;
	referencingFiles?: readonly string[];
}

export function calibrationInput(sample: CalibrationSample) {
	const path = sample.path ?? `calibration/${sample.name}.rs`;
	const change: RustChange = {
		path,
		before: sample.before || undefined,
		after: sample.after || undefined,
		patch: createTwoFilesPatch(
			sample.before ? `a/${path}` : "/dev/null",
			sample.after ? `b/${path}` : "/dev/null",
			sample.before, sample.after, undefined, undefined, { context: 20 },
		),
	};
	return {
		change,
		moduleReferences: new Map([[path, sample.after ? sample.referencingFiles ?? [] : []]]),
	};
}

export function outOfScopeFixtures(
	rules: readonly StyleRule[],
	samples: readonly (CalibrationSample & { ruleId: string })[],
): string[] {
	return samples.flatMap((sample) => {
		const rule = rules.find((candidate) => candidate.id === sample.ruleId);
		const path = calibrationInput(sample).change.path;
		return rule === undefined || ruleAppliesTo(rule, path) ? [] : [`${sample.name} (${path} is outside ${sample.ruleId} Applies to)`];
	});
}
