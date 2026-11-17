// Compares the checkpoints of a Web session with a native `--checkpoints` run, line by line:
//   node web/compare-checkpoints.mjs native.jsonl web.jsonl
// Prints the counts, the first differing checkpoint (with the differing fields) and, when the
// runs agree up to the shorter one, says so. Exit 0 when the files are identical.
import fs from "node:fs";

const [nativePath, webPath] = process.argv.slice(2);
if (!nativePath || !webPath) {
  console.error("usage: node web/compare-checkpoints.mjs native.jsonl web.jsonl");
  process.exit(2);
}
const read = (file) => fs.readFileSync(file, "utf8").split("\n").filter(Boolean);
const native = read(nativePath);
const web = read(webPath);
console.log(`native ${native.length} checkpoints, web ${web.length}`);
const common = Math.min(native.length, web.length);
for (let i = 0; i < common; i++) {
  if (native[i] === web[i]) continue;
  const a = JSON.parse(native[i]);
  const b = JSON.parse(web[i]);
  console.log(`first difference at checkpoint ${i} (native tick ${a.tick}, web tick ${b.tick}, kind ${a.kind}/${b.kind})`);
  for (const key of new Set([...Object.keys(a), ...Object.keys(b)])) {
    if (JSON.stringify(a[key]) !== JSON.stringify(b[key])) {
      console.log(`  ${key}: native ${JSON.stringify(a[key]).slice(0, 200)} | web ${JSON.stringify(b[key]).slice(0, 200)}`);
    }
  }
  process.exit(1);
}
if (native.length !== web.length) {
  console.log(`identical for the first ${common} checkpoints; one run is longer`);
  process.exit(1);
}
console.log("identical");
