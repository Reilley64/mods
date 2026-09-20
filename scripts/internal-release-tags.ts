const internalComponents = {
  domain: "domain",
  application: "application",
  "infrastructure/environment": "environment",
  "infrastructure/settings": "settings",
  "infrastructure/game_platform": "game-platform",
  "infrastructure/archive": "archive",
  "infrastructure/execution": "execution",
  "infrastructure/dependencies": "infrastructure",
  "presentation/cli": "mods-cli",
  "presentation/mcp": "mods-mcp",
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
