/*
  Splash 页脚本：本地化文案 + 启动进度 + 启动失败提示。

  独立成 .js 文件与 update-check.js 同理：CSP 的 script-src 只有 'self'，
  内联脚本不会执行。

  外壳在等待后端就绪期间持续发出 `backend-status`（含 elapsed/port），
  启动失败时发出 `backend-error`。两个事件都会在真实 UI 替换本页之前
  到达，因此 splash 可以显示真实进度——以前是一个静态的"请稍候"，
  首次启动最长会干转 60 秒。
*/
(function () {
  var zh = (navigator.language || "").toLowerCase().indexOf("zh") === 0;
  var TEXT = zh
    ? {
        starting: "正在启动服务，首次启动需要初始化，请稍候…",
        waiting: "已等待 {s} 秒（端口 {port}）…",
        failed: "启动失败：{msg}",
      }
    : {
        starting: "Starting the service; first launch may take longer…",
        waiting: "Waiting for {s}s (port {port})…",
        failed: "Startup failed: {msg}",
      };

  var hint = document.getElementById("hint");
  var startedAt = Date.now();
  var knownPort = null;
  var failed = false;

  function render() {
    if (!hint || failed) return;
    var secs = Math.floor((Date.now() - startedAt) / 1000);
    hint.textContent =
      secs === 0 && knownPort == null
        ? TEXT.starting
        : TEXT.waiting.replace("{s}", secs).replace(
            "{port}",
            knownPort == null ? "-" : knownPort
          );
  }

  render();
  setInterval(render, 1000);

  var tauri = window.__TAURI__;
  function listen(name, handler) {
    try {
      if (tauri && tauri.event && typeof tauri.event.listen === "function") {
        tauri.event.listen(name, handler);
      }
    } catch (e) {
      // The page may also be opened outside the shell (e.g. in a plain
      // browser during development); progress display is best-effort.
    }
  }

  // Authoritative clock from the shell's health-check thread.
  listen("backend-status", function (ev) {
    var payload = (ev && ev.payload) || {};
    if (typeof payload.elapsed === "number") {
      startedAt = Date.now() - payload.elapsed * 1000;
    }
    if (typeof payload.port === "number") {
      knownPort = payload.port;
    }
    render();
  });

  // Startup failure: stop the spinner and say so, instead of spinning
  // forever while the native error dialog sits underneath.
  listen("backend-error", function (ev) {
    failed = true;
    var msg = ev && ev.payload && ev.payload.message;
    var spinner = document.querySelector(".spinner");
    if (spinner) spinner.style.display = "none";
    if (hint && msg) {
      hint.textContent = TEXT.failed.replace("{msg}", msg);
      hint.style.color = "#c0392b";
    }
  });
})();
