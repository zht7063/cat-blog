// 主题切换按钮（D31）。<head> 里的内联脚本已经按 localStorage 设好了 data-theme；
// 这里只负责显示按钮并处理点击。禁用 JS 时按钮保持隐藏，页面跟随系统。
(function () {
  var button = document.querySelector(".theme-toggle");
  if (!button) return;
  var root = document.documentElement;
  var media = window.matchMedia("(prefers-color-scheme: dark)");
  function current() {
    return root.dataset.theme || (media.matches ? "dark" : "light");
  }
  button.hidden = false;
  button.addEventListener("click", function () {
    var next = current() === "dark" ? "light" : "dark";
    root.dataset.theme = next;
    try { localStorage.setItem("theme", next); } catch (e) {}
  });
})();
