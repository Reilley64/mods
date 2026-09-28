import { gh, publicAssets, stableRelease } from "./stable-release";

export function manifestPath(version: string) {
  stableRelease(`v${version}`);
  return `manifests/r/Reilley64/Mods/${version}/`;
}

export function containsVersion(files: { path?: string; filename?: string }[], version: string) {
  return files.some(file => (file.path ?? file.filename ?? "").startsWith(manifestPath(version)));
}

if (import.meta.main) {
  const release = await publicAssets(Bun.argv[2] ?? "");
  const path = manifestPath(release.version);
  const merged = await fetch(`https://api.github.com/repos/microsoft/winget-pkgs/contents/${path}`);
  if (merged.ok) {
    console.log(`Already merged: https://github.com/microsoft/winget-pkgs/tree/master/${path}`);
  } else {
    if (merged.status !== 404) throw new Error(`Winget lookup failed: ${merged.status}`);
    // Search broadly; confirm exact version by changed paths, not a title substring.
    const search = JSON.parse(await gh("api", "--method", "GET", "search/issues",
      "-f", "q=repo:microsoft/winget-pkgs is:pr is:open Reilley64.Mods in:title", "-f", "per_page=100"));
    if (search.incomplete_results || search.total_count > 100) throw new Error("Incomplete duplicate lookup; retry after inspection");
    let existing: string | undefined;
    for (const pr of search.items) {
      const pages = JSON.parse(await gh("api", "--paginate", "--slurp", `repos/microsoft/winget-pkgs/pulls/${pr.number}/files`));
      if (containsVersion(pages.flat(), release.version)) { existing = pr.html_url; break; }
    }
    if (existing) console.log(`Already open: ${existing}`);
    else {
      const bootstrap = await fetch("https://api.github.com/repos/microsoft/winget-pkgs/contents/manifests/r/Reilley64/Mods/0.1.0");
      if (!bootstrap.ok || release.version === "0.1.0") throw new Error("Human-controlled v0.1.0 bootstrap must be merged first");
      if (!process.env.WINGET_CREATE_GITHUB_TOKEN) throw new Error("Winget environment token is required");
      const child = Bun.spawn(["./wingetcreate.exe", "update", "Reilley64.Mods", "--urls", release.url,
        "--version", release.version, "--submit", "--no-open"], { stdout: "inherit", stderr: "inherit" });
      if (await child.exited !== 0) throw new Error("Winget submission failed; preserve release and retry this job");
    }
  }
}
