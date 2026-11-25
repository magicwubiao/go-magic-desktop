/*
  更新检查脚本（方案 C：版本检查 + 跳转下载页）。

  独立成 .js 文件是刻意的：这样 CSP 的 script-src 可以收紧为 'self'，
  不再需要 'unsafe-inline'（见 tauri.conf.json）。之所以不用 splash 页
  做这件事：后端热启动时端口几乎立即绑定，splash 页存活时间极短，fetch
  大概率被随后的导航打断。这个脚本由 Rust 侧在 setup 阶段以隐藏窗口
  （visible: false）加载，完全独立于主窗口的生命周期，每次启动都能可靠
  跑完检查。

  流程：并发拉取多个更新清单源 → 取其中最高的版本与 get_app_info 的
  当前版本比对 → 有新版才调用 notify_update_available（原生弹窗 +
  打开系统浏览器下载页）。无论结果如何最后调用 update_check_finished
  让 Rust 侧关闭本隐藏窗口（另有超时兜底）。

  外壳刻意不引入 HTTP 客户端与 TLS crate（见 README 安全模型），
  远程请求全部由 webview 网络栈完成；任何失败都静默跳过。
*/
(function () {
  // 更新清单来源。发版时 CI 会把 latest.json 的 version 提到 master
  // （见 .github/workflows/build.yml 的 publish-manifest 步骤），
  // 并 purge jsDelivr 缓存。
  //
  // 为什么是「多源取最高」而不是「按顺序取第一个成功的」：jsDelivr 与
  // raw 都是 CDN / 边缘缓存，发版后可能仍返回旧内容（曾经长期返回
  // 0.0.0 导致所有人永远显示「已是最新版本」）。GitHub Releases API
  // 没有缓存，是权威源，但国内网络未必可达；两者并发拉、取最高版本，
  // 任一源说了有新版就算有新版，同时保住国内的可达性。
  var MANIFEST_SOURCES = [
    // jsDelivr 对国内网络友好：{ url, timeoutMs }
    {
      url: "https://cdn.jsdelivr.net/gh/magicwubiao/go-magic-desktop@master/latest.json",
      timeoutMs: 5000,
    },
    {
      url: "https://fastly.jsdelivr.net/gh/magicwubiao/go-magic-desktop@master/latest.json",
      timeoutMs: 5000,
    },
    {
      url: "https://raw.githubusercontent.com/magicwubiao/go-magic-desktop/master/latest.json",
      timeoutMs: 5000,
    },
    // 权威源：无缓存，返回 Release 对象（tag_name / html_url / body）。
    // 没有独立 CDN，超时压短一些，避免拖慢「已是最新」的手动检查。
    {
      url: "https://api.github.com/repos/magicwubiao/go-magic-desktop/releases/latest",
      timeoutMs: 3500,
    },
  ];
  // 兜底关闭由 Rust 侧的 UPDATE_CHECK_MAX_LIFETIME 控制。
  var DOWNLOAD_PAGE =
    "https://github.com/magicwubiao/go-magic-desktop/releases/latest";

  var tauri = window.__TAURI__;
  var canInvoke = tauri && tauri.core && typeof tauri.core.invoke === "function";

  function finish(status) {
    if (canInvoke) {
      tauri.core
        .invoke("update_check_finished", { status: status })
        .catch(function () {});
    }
  }

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

  // 把两种清单格式归一化成 { version, downloadUrl, notes }：
  //   - latest.json: { version, downloadUrl, notes }
  //   - GitHub API:  { tag_name, html_url, body }
  // 解析不出合法版本时返回 null（当作该源失败）。
  //
  // downloadUrl 只是「建议」：Rust 侧会把它钳制到项目官方 releases
  // 路径内（见 main.rs 的 official_download_url），被投毒的清单无法把
  // 用户引到第三方页面。
  function normalizeManifest(raw) {
    if (!raw || typeof raw !== "object") return null;

    var rawVersion =
      typeof raw.version === "string"
        ? raw.version
        : typeof raw.tag_name === "string"
          ? raw.tag_name
          : "";
    var version = rawVersion.trim().replace(/^v/i, "");
    if (!/^\d+\.\d+/.test(version)) return null;

    var downloadUrl =
      typeof raw.downloadUrl === "string" && raw.downloadUrl
        ? raw.downloadUrl
        : typeof raw.html_url === "string" && raw.html_url
          ? raw.html_url
          : DOWNLOAD_PAGE;

    var notes = "";
    if (typeof raw.notes === "string" && raw.notes.trim()) {
      notes = raw.notes;
    } else if (typeof raw.body === "string") {
      notes = raw.body;
    }

    return { version: version, downloadUrl: downloadUrl, notes: notes };
  }

  function fetchSource(source) {
    var url = source.url + (source.url.indexOf("?") < 0 ? "?t=" + Date.now() : "");
    return fetch(url, {
      signal: AbortSignal.timeout(source.timeoutMs),
      cache: "no-store",
    })
      .then(function (res) {
        if (!res.ok) throw new Error("manifest HTTP " + res.status);
        return res.json();
      })
      .then(function (raw) {
        return normalizeManifest(raw);
      });
  }

  // 并发拉取全部源，取其中最高的版本。
  //
  // 一旦某个源报出的版本已经比当前版本新，就立刻兑现（用户能最快看到
  // 更新弹窗）；否则等所有源都尘埃落定，用最高版本得出「已是最新」。
  function resolveLatest(currentVersion) {
    return new Promise(function (resolve, reject) {
      var best = null;
      var pending = MANIFEST_SOURCES.length;
      var settled = false;

      function done() {
        if (settled) return;
        settled = true;
        resolve(best);
      }

      function onResult(manifest) {
        pending -= 1;
        if (manifest && (!best || isNewer(manifest.version, best.version))) {
          best = manifest;
        }
        if (best && isNewer(best.version, currentVersion)) {
          done();
        } else if (pending === 0) {
          done();
        }
      }

      MANIFEST_SOURCES.forEach(function (source) {
        fetchSource(source).then(onResult, function () { onResult(null); });
      });

      // 双保险：所有源都比 Rust 侧的生命周期上限先给出结果。
      setTimeout(function () {
        if (settled) return;
        if (best) { done(); } else { settled = true; reject(new Error("timeout")); }
      }, 9000);
    });
  }

  function check() {
    if (!canInvoke) return;

    tauri.core
      .invoke("get_app_info")
      .then(function (info) {
        if (!info || !info.version) throw new Error("no app version");
        return resolveLatest(info.version).then(function (manifest) {
          if (!manifest) throw new Error("bad manifest");
          if (!isNewer(manifest.version, info.version)) {
            return "up-to-date";
          }
          return tauri.core
            .invoke("notify_update_available", {
              notice: {
                currentVersion: info.version,
                latestVersion: manifest.version,
                downloadUrl: manifest.downloadUrl,
                notes: manifest.notes,
              },
            })
            .then(function () { return "updated"; });
        });
      })
      .then(finish, function () {
        // 网络不通、清单缺失等——手动检查时原生侧会提示失败。
        finish("error");
      });
  }

  check();
})();
