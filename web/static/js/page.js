(function () {
  if (window.RILGATE_PAGE_INITIALIZED) return;
  window.RILGATE_PAGE_INITIALIZED = true;
  var buttons = document.querySelectorAll("[data-dialog]");
  var i;
  var j;
  for (i = 0; i < buttons.length; i++) {
    buttons[i].addEventListener("click", function () {
      var dialog = document.getElementById(this.getAttribute("data-dialog"));
      if (dialog) dialog.showModal();
    });
  }
  var reloads = document.querySelectorAll('[data-action="reload"]');
  for (j = 0; j < reloads.length; j++) {
    reloads[j].addEventListener("click", function () {
      location.reload();
    });
  }
})();
