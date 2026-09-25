const internalComponents = {
  "src/domain": "domain",
  "src/application": "application",
  "src/infrastructure/environment": "infrastructure-environment",
  "src/infrastructure/settings": "infrastructure-settings",
  "src/infrastructure/game_platform": "infrastructure-game-platform",
  "src/infrastructure/archive": "infrastructure-archive",
  "src/infrastructure/execution": "infrastructure-execution",
  "src/infrastructure/dependencies": "infrastructure-dependencies",
  "src/presentation/cli": "mods-cli",
  "src/presentation/mcp": "mods-mcp",
} as const;

type Manifest = Record<string, string>;

export function selectInternalTags(before: Manifest, after: Manifest): string[] {
  return Object.entries(internalComponents)
    .flatMap(([path, component]) => {
      const version = after[path];
      return version && version !== "0.0.0" && version !== before[path] ? [`${component}-v${version}`] : [];
    })
    .sort();
}

function git(args: string[]): string {
  const result = Bun.spawnSync(["git", ...args], { stdout: "pipe", stderr: "pipe" });
  if (result.exitCode !== 0) throw new Error(new TextDecoder().decode(result.stderr));
  return new TextDecoder().decode(result.stdout);
}

if (import.meta.main) {
  const [beforeSha, afterSha] = process.argv.slice(2);
  if (!beforeSha || !afterSha) throw new Error("expected before and after SHA arguments");
  git(["rev-parse", "--verify", `${beforeSha}^{commit}`]);
  const priorManifest = Bun.spawnSync(["git", "show", `${beforeSha}:.release-please-manifest.json`], {
    stdout: "pipe",
    stderr: "pipe",
  });
  const before = priorManifest.exitCode === 0
    ? (JSON.parse(new TextDecoder().decode(priorManifest.stdout)) as Manifest)
    : {};
  const after = JSON.parse(git(["show", `${afterSha}:.release-please-manifest.json`])) as Manifest;
  for (const tag of selectInternalTags(before, after)) console.log(tag);
}
