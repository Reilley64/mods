export interface StyleRule {
	id: string;
	section: string;
	title: string;
	text: string;
	violation: string;
	compliant: string;
	badExamples: string[];
	goodExamples: string[];
}

type FieldName = "Rule" | "Violation" | "Compliant" | "Bad example" | "Good example";

interface RubricItem {
	section: string;
	title: string;
	fields: Partial<Record<FieldName, string[]>>;
}

const SECTION = /^##\s+(.+?)\s*$/;
const ITEM = /^###\s+(.+?)\s*$/;
const FIELD = /^####\s+(Rule|Violation|Compliant|Bad example|Good example)\s*$/;
const ANY_FIELD = /^####\s+(.+?)\s*$/;
const REQUIRED_FIELDS: FieldName[] = ["Rule", "Violation", "Compliant", "Bad example", "Good example"];

function slug(value: string): string {
	return value
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, "-")
		.replace(/^-|-$/g, "");
}

function prose(lines: readonly string[]): string {
	return lines
		.join("\n")
		.trim()
		.split(/\n\s*\n/)
		.map((paragraph) => paragraph.replace(/\s*\n\s*/g, " ").trim())
		.filter(Boolean)
		.join("\n\n");
}

function examples(lines: readonly string[]): string[] {
	const body = lines.join("\n").trim();
	const fenced = [...body.matchAll(/```[^\n]*\n([\s\S]*?)```/g)].map((match) => match[1]!.trim());
	return fenced.length > 0 ? fenced : body ? [body] : [];
}

function required(item: RubricItem, field: FieldName): string[] {
	const value = item.fields[field];
	if (value === undefined || value.every((line) => !line.trim())) {
		throw new Error(`coding-style-gate: rubric item "${item.section} / ${item.title}" is missing ${field}`);
	}
	return value;
}

export function extractStyleRules(markdown: string): StyleRule[] {
	const items: RubricItem[] = [];
	let section: string | undefined;
	let item: RubricItem | undefined;
	let field: FieldName | undefined;
	let inFence = false;

	for (const line of markdown.split(/\r?\n/)) {
		if (/^\s*```/.test(line)) {
			if (item !== undefined && field !== undefined) {
				item.fields[field]!.push(line);
			}
			inFence = !inFence;
			continue;
		}
		if (inFence) {
			if (item !== undefined && field !== undefined) {
				item.fields[field]!.push(line);
			}
			continue;
		}
		const sectionHeading = SECTION.exec(line);
		if (sectionHeading) {
			section = sectionHeading[1]!.trim();
			item = undefined;
			field = undefined;
			continue;
		}
		const itemHeading = ITEM.exec(line);
		if (itemHeading) {
			if (section === undefined) {
				throw new Error("coding-style-gate: rubric item appears before a section");
			}
			item = { section, title: itemHeading[1]!.trim(), fields: {} };
			items.push(item);
			field = undefined;
			continue;
		}
		const fieldHeading = FIELD.exec(line);
		if (fieldHeading) {
			if (item === undefined) {
				throw new Error(`coding-style-gate: rubric field ${fieldHeading[1]} appears before an item`);
			}
			field = fieldHeading[1] as FieldName;
			if (item.fields[field] !== undefined) {
				throw new Error(`coding-style-gate: duplicate rubric field ${field} in "${item.section} / ${item.title}"`);
			}
			item.fields[field] = [];
			continue;
		}
		const unknownField = ANY_FIELD.exec(line);
		if (unknownField) {
			throw new Error(`coding-style-gate: unknown rubric field ${unknownField[1]}`);
		}
		if (item !== undefined && field !== undefined) {
			item.fields[field]!.push(line);
		}
	}

	const ids = new Set<string>();
	return items.flatMap((rubricItem) => {
		for (const requiredField of REQUIRED_FIELDS) {
			required(rubricItem, requiredField);
		}
		const id = `${slug(rubricItem.section)}-${slug(rubricItem.title)}`;
		if (ids.has(id)) {
			throw new Error(`coding-style-gate: duplicate rubric rule id ${id}`);
		}
		ids.add(id);
		return [
			{
				id,
				section: rubricItem.section,
				title: rubricItem.title,
				text: prose(required(rubricItem, "Rule")),
				violation: prose(required(rubricItem, "Violation")),
				compliant: prose(required(rubricItem, "Compliant")),
				badExamples: examples(required(rubricItem, "Bad example")),
				goodExamples: examples(required(rubricItem, "Good example")),
			},
		];
	});
}
