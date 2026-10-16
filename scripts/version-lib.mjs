// 版本一致性檢查的純函式（不碰檔案系統，方便測試）。
//
// 這個專案的版本號分散在五個檔案裡，必須一致：
//   package.json、package-lock.json、src-tauri/Cargo.toml、src-tauri/Cargo.lock、src-tauri/tauri.conf.json
// 不一致的後果很隱蔽：打包出來的檔名與程式內部版本不同，自動更新的版本比對會錯亂，
// 已安裝的使用者可能被判定「已是最新」而永遠收不到更新。

export const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

/** Cargo.toml 的 [package] 區段中的 version。 */
export function cargoTomlVersion(text) {
  const start = text.search(/^\[package\]\s*$/m);
  if (start < 0) return null;
  const rest = text.slice(start).replace(/^\[package\]\s*\r?\n/, "");
  const next = rest.search(/^\[/m);
  const section = next < 0 ? rest : rest.slice(0, next);
  const m = section.match(/^version\s*=\s*"([^"]+)"/m);
  return m ? m[1] : null;
}

/** Cargo.lock 中指定 crate 自己的 version。 */
export function cargoLockVersion(text, crate) {
  const re = new RegExp(`\\[\\[package\\]\\]\\s*\\r?\\nname = "${crate}"\\s*\\r?\\nversion = "([^"]+)"`);
  const m = text.match(re);
  return m ? m[1] : null;
}

export function jsonVersion(text) {
  return JSON.parse(text).version ?? null;
}

/** package-lock.json 有兩處：頂層 version 與 packages[""].version。 */
export function packageLockVersions(text) {
  const j = JSON.parse(text);
  return { top: j.version ?? null, root: j.packages?.[""]?.version ?? null };
}

/**
 * 驗證版本。
 * @param {{file: string, version: string|null}[]} entries 各檔案讀到的版本
 * @param {string|undefined} tag 發佈用的標籤（可省略；省略時只檢查檔案彼此一致）
 * @returns {{ok: boolean, version: string|null, problems: string[]}}
 */
export function verify(entries, tag) {
  const problems = [];

  for (const e of entries) {
    if (!e.version) problems.push(`${e.file}：找不到版本號`);
    else if (!SEMVER.test(e.version)) problems.push(`${e.file}：版本號 "${e.version}" 不是有效的語意化版本`);
  }

  const versions = [...new Set(entries.map((e) => e.version).filter(Boolean))];
  const version = versions[0] ?? null;
  if (versions.length > 1) {
    problems.push(
      "各檔案的版本號不一致：" + entries.map((e) => `${e.file}=${e.version ?? "(無)"}`).join("、"),
    );
  }

  if (tag !== undefined) {
    const t = tag.replace(/^v/, "");
    if (!SEMVER.test(t)) {
      problems.push(`標籤 "${tag}" 不是 vX.Y.Z（或 vX.Y.Z-rc.N）的格式`);
    } else if (version && versions.length === 1) {
      // 預發行標籤（v1.2.3-rc.1）沿用同一個基礎版本號：安裝檔（尤其是 Windows 的 MSI）不接受
      // 非數字的預發行識別碼，所以檔案裡維持 1.2.3，只有標籤帶 -rc.N。
      const core = t.split(/[-+]/)[0];
      if (t !== version && core !== version) {
        problems.push(`標籤 "${tag}" 與檔案中的版本 ${version} 不符（請先更新五個檔案的版本號再打標籤）`);
      }
    }
  }

  return { ok: problems.length === 0, version, problems };
}
