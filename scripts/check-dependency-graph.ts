const packageNames = [
  "mods",
  "mods-mcp",
  "domain",
  "application",
  "environment",
  "settings",
  "game-platform",
  "archive",
  "execution",
  "infrastructure",
] as const;

type PackageName = (typeof packageNames)[number];
type CargoMetadata = {
  packages: Array<{
    name: string;
    dependencies: Array<{ name: string }>;
    manifest_path?: string;
    targets?: Array<{ name: string; kind: string[] }>;
  }>;
};

const expectedProjectDependencies: Record<PackageName, readonly PackageName[]> = {
  mods: ["application", "domain", "infrastructure"],
  "mods-mcp": ["application", "domain", "infrastructure"],
  domain: [],
  application: ["domain"],
  environment: ["application", "domain"],
  settings: ["application", "domain"],
  "game-platform": ["application", "domain"],
  archive: ["application", "domain"],
  execution: ["application", "domain"],
  infrastructure: ["environment", "settings", "game-platform", "archive", "execution", "application", "domain"],
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

  for (const [name, expectedPath] of [
    ["mods", "presentation/cli/Cargo.toml"],
    ["mods-mcp", "presentation/mcp/Cargo.toml"],
  ] as const) {
    const packageMetadata = packages.get(name);
    if (!packageMetadata) continue;
    const targets = packageMetadata.targets ?? [];
    if (targets.some((target) => target.kind.includes("lib"))) {
      errors.push(`${name} must be binary-only`);
    }
    if (!targets.some((target) => target.name === name && target.kind.includes("bin"))) {
      errors.push(`${name} must provide the ${name} binary`);
    }
    const manifestPath = packageMetadata.manifest_path?.replaceAll("\\", "/");
    if (manifestPath && !manifestPath.endsWith(expectedPath)) {
      errors.push(`${name} must be owned by ${expectedPath}`);
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
