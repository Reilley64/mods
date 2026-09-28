import { expect, test } from "bun:test";

const path = "skills/mods-cli/SKILL.md";
const config = await Bun.file("release-please-config.json").json();
const version = (await Bun.file("version.txt").text()).trim();
const skill = await Bun.file(path).text();
const frontmatter = Bun.YAML.parse(skill.split("---")[1]!) as {
  compatibility: string;
  metadata: { "reference-version": string };
};

// These checks protect our configuration and annotations, not Release Please's implementation.
test("the aggregate release owns the skill version update", () => {
  expect(config.packages["."]["version-file"]).toBe("version.txt");
  expect(config.packages["."]["extra-files"]).toContainEqual({ type: "generic", path });
  for (const [name, settings] of Object.entries(config.packages)) {
    if (name === ".") continue;
    expect(JSON.stringify(settings)).not.toContain(path);
  }
});

test("all current skill references match the aggregate version and are annotated", () => {
  expect(frontmatter.metadata["reference-version"]).toBe(`v${version}`);
  expect(frontmatter.compatibility).toContain(`Reference v${version};`);
  const lines = skill.split("\n");
  const current = [
    lines.find(line => line.startsWith("compatibility:")),
    lines.find(line => line.startsWith("  reference-version:")),
    lines.find(line => line.startsWith("Reference: published")),
  ];
  expect(current.every(line => line !== undefined)).toBe(true);
  expect(lines.filter(line => line.includes("x-release-please-version"))).toEqual(current);
  for (const line of current) {
    expect(line).toContain("x-release-please-version");
    expect(line!.match(/\d+\.\d+\.\d+/)?.[0]).toBe(version);
  }
  expect(current[2]).toContain(`**v${version}**`);
});

test("historical validation notes are not release-managed", async () => {
  const validation = await Bun.file("skills/mods-cli/references/validation.md").text();
  expect(validation).not.toContain("x-release-please-");
  expect(JSON.stringify(config)).not.toContain("skills/mods-cli/references/validation.md");
});
