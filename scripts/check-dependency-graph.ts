const packageNames = [
  "mods",
  "domain",
  "application",
  "environment",
  "settings",
  "game-platform",
  "archive",
  "execution",
  "infrastructure",
  "cli",
  "mcp",
] as const;

type PackageName = (typeof packageNames)[number];
type CargoMetadata = {
  packages: Array<{ name: string; dependencies: Array<{ name: string }> }>;
};

const expectedProjectDependencies: Record<PackageName, readonly PackageName[]> = {
  mods: ["cli", "mcp", "infrastructure"],
  domain: [],
  application: ["domain"],
  environment: ["application", "domain"],
  settings: ["application", "domain"],
  "game-platform": ["application", "domain"],
  archive: ["application", "domain"],
  execution: ["application", "domain"],
  infrastructure: ["environment", "settings", "game-platform", "archive", "execution", "application", "domain"],
  cli: ["application", "domain"],
  mcp: ["application", "domain"],
};

export function validateProjectGraph(metadata: CargoMetadata): string[] {
  const packages = new Map(metadata.packages.map((item) => [item.name, item]));
  const errors: string[] = [];

  for (const packageMetadata of metadata.packages) {
    if (!packageNames.includes(packageMetadata.name as PackageName)) {
      errors.push(`workspace must not contain package ${packageMetadata.name}`);
    }
  }

  for (const name of packageNames) {
    const packageMetadata = packages.get(name);
    if (!packageMetadata) {
      errors.push(`workspace is missing package ${name}`);
      continue;
    }
    const actual = packageMetadata.dependencies
      .map((dependency) => dependency.name)
      .filter((dependency): dependency is PackageName => packageNames.includes(dependency as PackageName))
      .sort();
    const expected = [...expectedProjectDependencies[name]].sort();
    if (actual.join(",") !== expected.join(",")) {
      errors.push(`${name} dependencies must be ${expected.join(", ") || "none"}`);
    }
  }

  return errors;
}

export async function checkDependencyGraphAsync(root = "."): Promise<string[]> {
  const result = Bun.spawnSync(["cargo", "metadata", "--format-version=1", "--no-deps"], {
    cwd: root,
    stdout: "pipe",
    stderr: "pipe",
  });
  if (result.exitCode !== 0) {
    return [new TextDecoder().decode(result.stderr)];
  }
  return validateProjectGraph(JSON.parse(new TextDecoder().decode(result.stdout)) as CargoMetadata);
}

if (import.meta.main) {
  const errors = await checkDependencyGraphAsync();
  if (errors.length) {
    console.error(errors.join("\n"));
    process.exit(1);
  }
}
