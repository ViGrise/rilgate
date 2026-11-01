(function () {
  if (window.RILGATE_PAGE_INITIALIZED) return;
  window.RILGATE_PAGE_INITIALIZED = true;
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
      if (form && !form.rilgateClosesDialog) {
        form.rilgateClosesDialog = true;
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
