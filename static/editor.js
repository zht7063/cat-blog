// 编辑页（D26、docs/v1.0-spec.md §9.2）：Ctrl/⌘+S 保存；文本框有未保存的改动时，离开页面前提醒。
(function () {
  var form = document.querySelector("form[data-editor]");
  if (!form) return;
  var text = form.querySelector("textarea");
  var save = form.querySelector("[data-save]");
  // 保存失败时编辑框里的原文还没存进库，一开始就算有改动。
  var saved = form.hasAttribute("data-unsaved") ? null : text.value;
  var leaving = false;
  form.addEventListener("submit", function (e) {
    if (e.submitter === save) leaving = true;
  });
  document.addEventListener("keydown", function (e) {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      form.requestSubmit(save);
    }
  });
  window.addEventListener("beforeunload", function (e) {
    if (!leaving && text.value !== saved) {
      e.preventDefault();
      e.returnValue = "";
    }
  });
})();
