import assert from "node:assert/strict";
import { test } from "node:test";
import { cargoLockVersion, cargoTomlVersion, jsonVersion, packageLockVersions, verify } from "./version-lib.mjs";

const same = (v) => ["a", "b", "c"].map((file) => ({ file, version: v }));

test("檔案版本一致、沒有標籤：通過", () => {
  const r = verify(same("0.1.0"));
  assert.equal(r.ok, true);
  assert.equal(r.version, "0.1.0");
});

test("任何一個檔案版本不同：失敗，並指出是哪些檔案", () => {
  const r = verify([
    { file: "package.json", version: "0.2.0" },
    { file: "Cargo.toml", version: "0.1.0" },
  ]);
  assert.equal(r.ok, false);
  assert.match(r.problems.join("\n"), /package\.json=0\.2\.0/);
  assert.match(r.problems.join("\n"), /Cargo\.toml=0\.1\.0/);
});

test("找不到版本號、或不是語意化版本：失敗", () => {
  assert.equal(verify([{ file: "x", version: null }]).ok, false);
  assert.equal(verify(same("1.0")).ok, false);
  assert.equal(verify(same("latest")).ok, false);
});

test("標籤與檔案版本相符（有沒有 v 都可以）：通過", () => {
  assert.equal(verify(same("0.1.1"), "v0.1.1").ok, true);
  assert.equal(verify(same("0.1.1"), "0.1.1").ok, true);
});

test("標籤與檔案版本不符：失敗（忘了改版本號就打標籤）", () => {
  const r = verify(same("0.1.0"), "v0.1.1");
  assert.equal(r.ok, false);
  assert.match(r.problems.join("\n"), /v0\.1\.1/);
});

test("預發行標籤沿用同一個基礎版本：通過；基礎版本不同：失敗", () => {
  assert.equal(verify(same("0.1.0"), "v0.1.0-rc.1").ok, true);
  assert.equal(verify(same("0.1.0"), "v0.1.0-rc.2").ok, true);
  assert.equal(verify(same("0.1.0"), "v0.2.0-rc.1").ok, false);
});

test("檔案版本本身就帶預發行識別碼時，標籤要完全相同", () => {
  assert.equal(verify(same("0.2.0-beta.1"), "v0.2.0-beta.1").ok, true);
  assert.equal(verify(same("0.2.0-beta.1"), "v0.2.0-beta.2").ok, false);
});

test("標籤格式不對：失敗", () => {
  assert.equal(verify(same("0.1.0"), "release-1").ok, false);
  assert.equal(verify(same("0.1.0"), "v0.1").ok, false);
  assert.equal(verify(same("0.1.0"), "main").ok, false);
});

test("檔案本身不一致時，不會再拿標籤去比對（避免產生誤導的訊息）", () => {
  const r = verify([{ file: "a", version: "0.1.0" }, { file: "b", version: "0.2.0" }], "v0.1.0");
  assert.equal(r.ok, false);
  assert.equal(r.problems.filter((p) => p.includes("與檔案中的版本")).length, 0);
});

test("Cargo.toml：只讀 [package] 的 version，不會誤抓相依套件的 version", () => {
  const toml = [
    "[package]",
    'name = "x"',
    'version = "1.2.3"',
    'edition = "2021"',
    "",
    "[dependencies]",
    'serde = { version = "1" }',
    "",
    "[profile.release]",
    'version = "999"',
  ].join("\n");
  assert.equal(cargoTomlVersion(toml), "1.2.3");
});

test("Cargo.toml：[package] 沒有 version、或沒有 [package]：回傳 null", () => {
  assert.equal(cargoTomlVersion('[package]\nname = "x"\n\n[dependencies]\nfoo = { version = "9" }\n'), null);
  assert.equal(cargoTomlVersion('[workspace]\nmembers = []\n'), null);
});

test("Cargo.lock：取出指定 crate 自己的版本，不會取到其他 crate", () => {
  const lock = [
    "[[package]]",
    'name = "aho-corasick"',
    'version = "1.1.5"',
    "",
    "[[package]]",
    'name = "rdswitch"',
    'version = "0.1.0"',
    "",
    "[[package]]",
    'name = "rdswitch_lib"',
    'version = "7.7.7"',
  ].join("\n");
  assert.equal(cargoLockVersion(lock, "rdswitch"), "0.1.0");
  assert.equal(cargoLockVersion(lock, "nope"), null);
});

test("JSON 與 package-lock.json 的版本讀取", () => {
  assert.equal(jsonVersion('{"version":"3.2.1"}'), "3.2.1");
  assert.equal(jsonVersion("{}"), null);
  assert.deepEqual(packageLockVersions('{"version":"1.0.0","packages":{"":{"version":"1.0.1"}}}'), { top: "1.0.0", root: "1.0.1" });
  assert.deepEqual(packageLockVersions("{}"), { top: null, root: null });
});
