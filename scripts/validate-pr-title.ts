export function validatePullRequestTitle(title: string): string[] {
  const result = Bun.spawnSync(["bunx", "--no-install", "commitlint"], {
    cwd: import.meta.dir + "/..",
    stdin: new TextEncoder().encode(title),
    stdout: "pipe",
    stderr: "pipe",
  });
  return result.exitCode === 0 ? [] : [new TextDecoder().decode(result.stderr) || new TextDecoder().decode(result.stdout)];
}

if (import.meta.main) {
  const title = process.env.PR_TITLE;
  if (!title) {
    console.error("PR_TITLE is required");
    process.exit(1);
  }
  const errors = validatePullRequestTitle(title);
  if (errors.length) {
    console.error(errors.join("\n"));
    process.exit(1);
  }
}
