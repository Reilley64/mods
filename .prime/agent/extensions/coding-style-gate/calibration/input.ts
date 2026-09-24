import { createTwoFilesPatch } from "diff";
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
