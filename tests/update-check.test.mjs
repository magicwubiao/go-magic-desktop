// ============================================================================
// update-check.js 版本比较逻辑的单元测试（node --test）。
//
// 版本比对是更新检查的裁决点：比较出错会直接导致所有客户端「永远提示更新」
// 或「永远不提示」（jsDelivr 返回 0.0.0 的事故即由此类 bug 引发），因此把
// parseVersion / compareVersions / isNewer 抽成独立模块并在此全面覆盖。
//
// 运行：npm run test:web
// ============================================================================

import { readFileSync } from "node:fs";
import { test } from "node:test";
import assert from "node:assert/strict";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// version-compare.js 是浏览器/Node 双格式的 UMD 经典脚本（splash 页以
// <script src> 直接加载，package.json 的 "type": "module" 会让 Node 把 .js
// 当 ESM 处理，所以这里按经典脚本语义手工执行，命中 module.exports 分支）。
const scriptPath = join(
  dirname(fileURLToPath(import.meta.url)),
  "../src-tauri/splash/version-compare.js"
);
const moduleShim = { exports: {} };
new Function("module", "exports", readFileSync(scriptPath, "utf8"))(
  moduleShim,
  moduleShim.exports
);

const {
  parseVersion,
  compareVersions,
  isNewer,
} = moduleShim.exports;

test("parseVersion splits numeric segments", () => {
  assert.deepEqual(parseVersion("1.2.3"), [1, 2, 3]);
  assert.deepEqual(parseVersion("0.6.25"), [0, 6, 25]);
});

test("parseVersion tolerates a v prefix and whitespace", () => {
  assert.deepEqual(parseVersion("v1.2.3"), [1, 2, 3]);
  assert.deepEqual(parseVersion(" V10.0.1 "), [10, 0, 1]);
});

test("parseVersion treats non-numeric segments as zero", () => {
  assert.deepEqual(parseVersion("1.abc.3"), [1, 0, 3]);
  assert.deepEqual(parseVersion(""), [0]);
  assert.deepEqual(parseVersion(null), [0]);
  assert.deepEqual(parseVersion(undefined), [0]);
});

test("compareVersions compares segment by segment", () => {
  assert.equal(compareVersions("1.2.3", "1.2.3"), 0);
  assert.ok(compareVersions("1.2.4", "1.2.3") > 0);
  assert.ok(compareVersions("1.2.3", "1.2.4") < 0);
  assert.ok(compareVersions("2.0.0", "1.9.9") > 0);
  assert.ok(compareVersions("1.10.0", "1.9.0") > 0, "numeric, not lexicographic");
});

test("compareVersions handles differing segment counts", () => {
  assert.ok(compareVersions("1.2", "1.2.0") === 0);
  assert.ok(compareVersions("1.2", "1.2.1") < 0);
  assert.ok(compareVersions("1.2.0.1", "1.2") > 0);
});

test("isNewer is strictly greater-than", () => {
  assert.equal(isNewer("1.3.0", "1.2.9"), true);
  assert.equal(isNewer("1.3.0", "1.3.0"), false);
  assert.equal(isNewer("1.2.9", "1.3.0"), false);
});

test("isNewer ignores the v prefix on both sides", () => {
  assert.equal(isNewer("v1.3.0", "1.2.9"), true);
  assert.equal(isNewer("1.3.0", "v1.3.0"), false);
});

test("isNewer is not fooled by stale or poisoned manifests", () => {
  // jsDelivr 缓存曾长期返回 0.0.0 —— 任何真实版本都不能被判为「更旧」。
  assert.equal(isNewer("0.0.0", "0.6.25"), false);
  assert.equal(isNewer("0.6.25", "0.0.0"), true);
});
