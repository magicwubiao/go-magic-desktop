/*
  版本比较工具（从 update-check.js 抽出，便于单元测试）。

  双格式加载：浏览器里挂到 window.VersionCompare（供 update-check.js 使用），
  Node 里走 module.exports（供 tests/update-check.test.mjs 使用）。
  保持 ES5 语法，与 splash 页其余脚本一致。
*/
(function (root, factory) {
  if (typeof module === "object" && module.exports) {
    module.exports = factory();
  } else if (root) {
    root.VersionCompare = factory();
  }
})(typeof self !== "undefined" ? self : this, function () {
  // "1.2.3" / "v1.2.3" -> [1, 2, 3]；非数字段按 0 处理。
  function parseVersion(v) {
    return String(v || "")
      .trim()
      .replace(/^v/i, "")
      .split(".")
      .map(function (n) { return parseInt(n, 10) || 0; });
  }

  // 返回 >0 表示 a 比 b 新，0 表示相同，<0 表示更旧。
  function compareVersions(a, b) {
    var x = parseVersion(a);
    var y = parseVersion(b);
    var len = Math.max(x.length, y.length);
    for (var i = 0; i < len; i++) {
      var diff = (x[i] || 0) - (y[i] || 0);
      if (diff !== 0) return diff;
    }
    return 0;
  }

  function isNewer(latest, current) {
    return compareVersions(latest, current) > 0;
  }

  return {
    parseVersion: parseVersion,
    compareVersions: compareVersions,
    isNewer: isNewer,
  };
});
