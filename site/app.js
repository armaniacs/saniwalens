// saniwalens docs site — no dependencies. Deferred, so DOM is ready.
(function () {
  "use strict";

  /* Polyfill popover only where it is missing (progressive enhancement). */
  if (!Object.hasOwn(HTMLElement.prototype, "popover")) {
    import("https://unpkg.com/@oddbird/popover-polyfill@latest/dist/popover.min.js").catch(function () {
      /* Without popover the menu list stays visible as a nested list. */
    });
  }

  /* ---- Responsive header: inline when the row fits, popover when not ----
   * No hard-coded breakpoint: measure the row and toggle `is-wide`.
   * Without script the header stays collapsed (menu button + native popover).
   */
  var header = document.querySelector(".site-header");
  var inner = document.querySelector(".header-inner");
  var navList = document.getElementById("site-menu");

  function syncLayout() {
    if (!header || !inner || !navList) return;
    var wasOpen = false;
    try {
      wasOpen = navList.matches(":popover-open");
    } catch (e) {
      wasOpen = false;
    }

    header.classList.add("is-wide", "is-measuring");
    var wide = inner.scrollWidth <= inner.clientWidth + 1;
    header.classList.remove("is-measuring");
    header.classList.toggle("is-wide", wide);

    // The popover lives only in the collapsed layout; inline is plain flow.
    if (wide) {
      navList.removeAttribute("popover");
    } else {
      if (!navList.hasAttribute("popover")) navList.setAttribute("popover", "auto");
      if (wasOpen) {
        try {
          navList.showPopover();
        } catch (e) {}
      }
    }
  }

  if (header && inner && typeof ResizeObserver !== "undefined") {
    var lastInlineWidth = 0;
    var ro = new ResizeObserver(function () {
      var isWide = header.classList.contains("is-wide");
      // While collapsed, only remeasure when the header grew — measuring on
      // every shrink would thrash between layouts.
      if (!isWide && inner.clientWidth <= lastInlineWidth) return;
      syncLayout();
      lastInlineWidth = inner.clientWidth;
    });
    ro.observe(inner);
    if (navList) ro.observe(navList);
    syncLayout();
  }

  /* ---- Theme toggle: two states (system + opposite of rendered) ----
   * Developer docs justify a persistent control; intent is a temporary comfort
   * adjustment, so toggling back to the system value removes the override
   * instead of pinning it.
   */
  var meta = document.querySelector('meta[name="color-scheme"]');
  var toggle = document.getElementById("theme-toggle");
  var label = document.getElementById("theme-toggle-label");
  var mq = window.matchMedia("(prefers-color-scheme: dark)");

  function systemScheme() {
    return mq.matches ? "dark" : "light";
  }

  function renderedScheme() {
    var stored = null;
    try {
      stored = localStorage.getItem("color-scheme");
    } catch (e) {}
    if (stored === "light" || stored === "dark") return stored;
    return systemScheme();
  }

  function paintLabel() {
    if (!label || !meta) return;
    label.textContent =
      renderedScheme() === "dark" ? "Switch to light theme" : "Switch to dark theme";
  }

  function applyScheme(stored) {
    if (!meta) return;
    meta.content = stored || "light dark";
    paintLabel();
  }

  if (toggle && meta) {
    paintLabel();
    toggle.addEventListener("click", function () {
      var target = renderedScheme() === "dark" ? "light" : "dark";
      try {
        if (target === systemScheme()) {
          localStorage.removeItem("color-scheme");
          applyScheme(null);
        } else {
          localStorage.setItem("color-scheme", target);
          applyScheme(target);
        }
      } catch (e) {
        applyScheme(target);
      }
    });

    window.addEventListener("storage", function (ev) {
      if (ev.key !== "color-scheme") return;
      applyScheme(ev.newValue);
    });

    if (typeof mq.addEventListener === "function") {
      mq.addEventListener("change", paintLabel);
    }
  }
})();
