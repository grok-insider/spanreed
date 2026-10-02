import { createHash } from "node:crypto";
import { readdir, readFile, lstat } from "node:fs/promises";
import { join } from "node:path";

export async function fileHashes(directory, prefix = "") {
  const result = {};
  for (const name of (await readdir(join(directory, prefix))).sort()) {
    const relative = prefix ? `${prefix}/${name}` : name;
    if (relative === "fabrials-manifest.json") continue;
    const absolute = join(directory, relative);
    const stat = await lstat(absolute);
    if (stat.isSymbolicLink())
      throw new Error(`Symlink not allowed in distribution: ${relative}`);
    if (stat.isDirectory())
      Object.assign(result, await fileHashes(directory, relative));
    else
      result[relative] = createHash("sha256")
        .update(await readFile(absolute))
        .digest("hex");
  }
  return result;
}

export async function verifyDistribution(directory) {
  const manifest = JSON.parse(
    await readFile(join(directory, "fabrials-manifest.json"), "utf8"),
  );
  const actual = await fileHashes(directory);
  if (
    manifest.schema !== 1 ||
    JSON.stringify(actual) !== JSON.stringify(manifest.files)
  ) {
    throw new Error(`Distribution modified or incomplete: ${directory}`);
  }
  const pkg = JSON.parse(
    await readFile(join(directory, "package.json"), "utf8"),
  );
  if (pkg.name !== manifest.name || pkg.version !== manifest.version)
    throw new Error(`Package identity mismatch: ${directory}`);
  return manifest;
}
