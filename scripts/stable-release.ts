import { createHash } from "node:crypto";

export function stableRelease(tag: string) {
  if (!/^v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)$/.test(tag) || tag === "v0.0.0") {
    throw new Error("Expected a nonzero aggregate stable tag");
  }
  const archive = `mods-${tag}-x86_64-pc-windows-msvc.zip`;
  return { tag, version: tag.slice(1), archive,
    url: `https://github.com/Reilley64/mods/releases/download/${tag}/${archive}` };
}

export function verifyChecksum(archive: string, bytes: Uint8Array, sums: string) {
  const hash = createHash("sha256").update(bytes).digest("hex");
  if (sums.trim() !== `${hash}  ${archive}`) throw new Error("Public ZIP checksum mismatch");
  return hash;
}

export async function publicAssets(tag: string) {
  const release = stableRelease(tag);
  // Deliberately anonymous: an authenticated API download is not public evidence.
  const [zip, sums] = await Promise.all([fetch(release.url), fetch(new URL("SHA256SUMS", release.url))]);
  if (!zip.ok || !sums.ok) throw new Error("Stable assets are not publicly available");
  const hash = verifyChecksum(release.archive, new Uint8Array(await zip.arrayBuffer()), await sums.text());
  return { ...release, hash };
}

export async function gh(...args: string[]) {
  const result = Bun.spawn(["gh", ...args], { stdout: "pipe", stderr: "inherit" });
  const output = await new Response(result.stdout).text();
  if (await result.exited !== 0) throw new Error(`gh ${args[0]} failed`);
  return output;
}

if (import.meta.main) {
  const [mode, tag] = Bun.argv.slice(2);
  const release = stableRelease(tag ?? "");
  const metadata = JSON.parse(await gh("api", `repos/Reilley64/mods/releases/tags/${release.tag}`));
  if (metadata.draft || metadata.prerelease || metadata.tag_name !== release.tag) throw new Error("Release must be published and stable");
  if (mode === "publish") {
    // Never clobber public bytes or delete the release. Recovery adds only missing assets.
    let uploadedHash: string | undefined;
    const names = new Set(metadata.assets.map((asset: { name: string }) => asset.name));
    if (!names.has(release.archive)) {
      if (names.has("SHA256SUMS")) throw new Error("Checksum exists without ZIP; investigate before recovery");
      uploadedHash = verifyChecksum(release.archive, new Uint8Array(await Bun.file(`dist/${release.archive}`).arrayBuffer()), await Bun.file("dist/SHA256SUMS").text());
      await gh("release", "upload", release.tag, `dist/${release.archive}`, "--repo", "Reilley64/mods");
    }
    if (!names.has("SHA256SUMS")) {
      const zip = await fetch(release.url);
      if (!zip.ok) throw new Error("Uploaded ZIP is not public; retry recovery later");
      const hash = createHash("sha256").update(new Uint8Array(await zip.arrayBuffer())).digest("hex");
      if (uploadedHash && hash !== uploadedHash) throw new Error("Public ZIP differs from packaged ZIP");
      await Bun.write("dist/SHA256SUMS", `${hash}  ${release.archive}\n`);
      await gh("release", "upload", release.tag, "dist/SHA256SUMS", "--repo", "Reilley64/mods");
    }
  } else if (mode !== "verify") throw new Error("Expected publish or verify");
  console.log(JSON.stringify(await publicAssets(release.tag), null, 2));
}
