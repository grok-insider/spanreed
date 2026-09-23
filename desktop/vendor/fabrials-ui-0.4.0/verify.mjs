import { verifyDistribution } from "./integrity.mjs";

if (process.argv.length < 3)
  throw new Error("Pass one or more generated distribution directories");
for (const directory of process.argv.slice(2)) {
  const manifest = await verifyDistribution(directory);
  console.log(`Verified ${manifest.name}@${manifest.version}`);
}
