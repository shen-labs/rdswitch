// 前端翻譯字典的一致性檢查（CI 與本機都用同一個）。
//
//   node scripts/check-i18n.mjs
//
// 檢查：
//   1. 每個語言都有繁中字典的所有鍵，且沒有多餘的鍵
//   2. 每個鍵在各語言的 {變數} 都相同
//   3. index.html 與 app.js 用到的鍵都有定義
//   4. 沒有定義了卻從未使用的鍵（避免字典越積越多）
//
// 後端字典（src-tauri/src/i18n.rs）有自己的 Rust 測試，這裡只檢查前端。

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (rel) => readFileSync(join(root, rel), "utf8");

// i18n.js 是給瀏覽器用的一般 script（沒有 export），在隔離環境執行後取出字典
const { DICT, LANGS } = vm.runInNewContext(`${read("src/i18n.js")}\n({ DICT, LANGS })`, {
  document: { documentElement: {}, querySelectorAll: () => [] },
});

const problems = [];
const base = Object.keys(DICT["zh-TW"]);
const placeholders = (s) =>
  [...s.matchAll(/\{(\w+)\}/g)]
    .map((m) => m[1])
    .sort()
    .join(",");

for (const lang of LANGS) {
  if (!DICT[lang]) {
    problems.push(`語言 ${lang} 在 LANGS 中，但 DICT 沒有它`);
    continue;
  }
  for (const key of base) {
    const value = DICT[lang][key];
    if (!value || !value.trim()) problems.push(`缺少翻譯：${lang} / ${key}`);
    else if (placeholders(value) !== placeholders(DICT["zh-TW"][key]))
      problems.push(`變數與繁中不一致：${lang} / ${key}`);
  }
  for (const key of Object.keys(DICT[lang])) {
    if (!base.includes(key)) problems.push(`多餘的鍵（繁中沒有）：${lang} / ${key}`);
  }
}

// 收集 index.html 與 app.js 實際用到的鍵
const used = new Set();
const html = read("src/index.html");
const app = read("src/app.js");
for (const m of html.matchAll(/data-i18n="([\w.]+)"/g)) used.add(m[1]);
for (const m of html.matchAll(/data-i18n-attr="([^"]+)"/g)) {
  for (const pair of m[1].split(";")) used.add(pair.split(":")[1].trim());
}
for (const m of app.matchAll(/\bt\("([\w.]+)"/g)) used.add(m[1]);

// 動態組出來的鍵（t("code." + …)）：只要是 code.* 就視為有被使用
const isDynamic = (key) => key.endsWith(".") || key.startsWith("code.");

for (const key of used) {
  if (!isDynamic(key) && !base.includes(key)) problems.push(`用到了沒有定義的鍵：${key}`);
}
for (const key of base) {
  if (!isDynamic(key) && !used.has(key)) problems.push(`定義了卻沒有使用的鍵：${key}`);
}

if (problems.length) {
  console.error(problems.map((p) => `  ✘ ${p}`).join("\n"));
  console.error(`\n翻譯檢查失敗：${problems.length} 個問題`);
  process.exit(1);
}
console.log(`翻譯檢查通過：${LANGS.length} 種語言 × ${base.length} 個鍵，一致且沒有多餘或未使用的鍵`);
