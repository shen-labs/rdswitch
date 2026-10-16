// 檢查五個檔案的版本號是否一致；給標籤時，另外檢查標籤是否與版本相符。
//
//   node scripts/check-version.mjs            # 平常（CI）：只檢查檔案彼此一致
//   node scripts/check-version.mjs v0.1.1     # 發佈：標籤也要與檔案版本相符
//
// 純函式在 version-lib.mjs，測試在 version-lib.test.mjs。

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { cargoLockVersion, cargoTomlVersion, jsonVersion, packageLockVersions, verify } from "./version-lib.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (rel) => readFileSync(join(root, rel), "utf8");

const lock = packageLockVersions(read("package-lock.json"));
const entries = [
  { file: "package.json", version: jsonVersion(read("package.json")) },
  { file: "package-lock.json (version)", version: lock.top },
  { file: 'package-lock.json (packages[""])', version: lock.root },
  { file: "src-tauri/Cargo.toml", version: cargoTomlVersion(read("src-tauri/Cargo.toml")) },
  { file: "src-tauri/Cargo.lock (rdswitch)", version: cargoLockVersion(read("src-tauri/Cargo.lock"), "rdswitch") },
  { file: "src-tauri/tauri.conf.json", version: jsonVersion(read("src-tauri/tauri.conf.json")) },
];

const tag = process.argv[2];
const result = verify(entries, tag);

for (const e of entries) console.log(`  ${e.version ?? "(無)"}\t${e.file}`);
if (tag !== undefined) console.log(`  標籤\t${tag}`);

if (!result.ok) {
  console.error("\n" + result.problems.map((p) => `  ✘ ${p}`).join("\n"));
  console.error("\n版本檢查失敗");
  process.exit(1);
}
console.log(`\n版本檢查通過：${result.version}${tag !== undefined ? `（標籤 ${tag}）` : ""}`);
