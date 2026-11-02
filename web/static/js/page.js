(function () {
  if (document.documentElement.hasAttribute("data-page-initialized")) return;
  document.documentElement.setAttribute("data-page-initialized", "");
  var handledForms = new WeakSet();
  var buttons = document.querySelectorAll("[data-dialog]");
  var i;
  var j;
  for (i = 0; i < buttons.length; i++) {
    buttons[i].addEventListener("click", function () {
      var dialog = document.getElementById(this.getAttribute("data-dialog"));
      if (!dialog) return;
      if (typeof dialog.showModal === "function") {
        dialog.showModal();
        return;
      }
      dialog.setAttribute("open", "");
      var form = dialog.querySelector("form");
      if (form && !handledForms.has(form)) {
        handledForms.add(form);
        form.addEventListener("submit", function (event) {
          event.preventDefault();
          dialog.removeAttribute("open");
        });
      }
    });
  }
  var reloads = document.querySelectorAll('[data-action="reload"]');
  for (j = 0; j < reloads.length; j++) {
    reloads[j].addEventListener("click", function () {
      location.reload();
    });
  }
})();
