/*
 * Kronello design system preview runtime.
 *
 * Query parameters:
 *   theme=dark|light   theme to render (default: dark, never follows the OS)
 *   state=<name>       keep [data-when~=name] and drop [data-unless~=name]
 *   only=<id>          components page: render one section only
 *   capture            screenshot mode (no animation, no page chrome)
 *
 * Press "t" to toggle the theme. Works from file:// without a server.
 */
(function () {
  "use strict";

  var params = new URLSearchParams(location.search);
  var root = document.documentElement;
  var STORAGE_KEY = "kronello-preview-theme";

  function storedTheme() {
    try { return localStorage.getItem(STORAGE_KEY); } catch (e) { return null; }
  }
  function storeTheme(theme) {
    try { localStorage.setItem(STORAGE_KEY, theme); } catch (e) { /* storage unavailable */ }
  }

  var theme = params.get("theme") || storedTheme() || "dark";
  root.dataset.theme = theme === "light" ? "light" : "dark";
  var state = params.get("state") || "";
  if (state) root.dataset.state = state;
  if (params.has("capture")) root.dataset.capture = "";

  function setTheme(next) {
    var changed = root.dataset.theme !== next;
    root.dataset.theme = next;
    storeTheme(next);
    if (changed) document.dispatchEvent(new CustomEvent("kronello-theme", { detail: next }));
    document.querySelectorAll("[data-theme-label]").forEach(function (el) {
      el.textContent = next === "light" ? "Light" : "Dark";
    });
  }

  function renderIcons(scope) {
    var icons = window.KRONELLO_ICONS || {};
    scope.querySelectorAll("i[data-icon]").forEach(function (el) {
      var name = el.getAttribute("data-icon");
      var markup = icons[name];
      if (!markup) {
        console.error("Unknown icon: " + name);
        el.textContent = "?";
        el.className += " danger";
        return;
      }
      var holder = document.createElement("span");
      holder.innerHTML = markup;
      var svg = holder.firstElementChild;
      svg.setAttribute("class", ("ic " + el.className).trim() + " lucide lucide-" + name);
      svg.setAttribute("aria-hidden", "true");
      svg.removeAttribute("width");
      svg.removeAttribute("height");
      el.replaceWith(svg);
    });
  }

  function applyState() {
    var active = state.split(",").filter(Boolean);
    function matches(el, attr) {
      return el.getAttribute(attr).split(/\s+/).some(function (name) { return active.indexOf(name) >= 0; });
    }
    document.querySelectorAll("[data-when]").forEach(function (el) {
      if (!matches(el, "data-when")) el.remove();
    });
    document.querySelectorAll("[data-unless]").forEach(function (el) {
      if (matches(el, "data-unless")) el.remove();
    });
  }

  function applyOnly() {
    var only = params.get("only");
    if (!only) return;
    var target = document.getElementById(only);
    if (!target) { console.error("Unknown section: " + only); return; }
    root.dataset.only = only;
    document.querySelectorAll("[data-section]").forEach(function (el) {
      if (el !== target) el.remove();
    });
  }

  // Deterministic pseudo waveform so screenshots are reproducible.
  function renderWaves() {
    document.querySelectorAll(".clip-wave").forEach(function (el, index) {
      var width = Math.max(10, Math.floor(el.getBoundingClientRect().width));
      var seed = 7 + index * 131;
      function next() { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648; }
      var bars = [];
      for (var x = 0; x < width; x += 3) {
        var envelope = 0.45 + 0.55 * Math.abs(Math.sin(x / 37 + index));
        var h = Math.max(1, Math.round(9 * envelope * (0.35 + 0.65 * next())));
        bars.push('<rect x="' + x + '" y="' + ((9 - h) / 2) + '" width="1.5" height="' + h + '"/>');
      }
      el.innerHTML = '<svg width="' + width + '" height="9" viewBox="0 0 ' + width + ' 9" aria-hidden="true">' + bars.join("") + "</svg>";
    });
  }

  // Ruler ticks: data-frames (span), data-rate (fps), data-minor / data-major (frames between ticks / labels),
  // data-start (first frame), data-inset (px kept free at both ends).
  function renderRulers() {
    document.querySelectorAll(".ruler[data-frames]").forEach(function (el) {
      var frames = Number(el.dataset.frames);
      var rate = Number(el.dataset.rate || 24);
      var minor = Number(el.dataset.minor || 6);
      var major = Number(el.dataset.major || rate);
      var start = Number(el.dataset.start || 0);
      var inset = Number(el.dataset.inset || 0);
      var out = [];
      for (var f = 0; f <= frames; f += minor) {
        var left = inset
          ? "calc(" + inset + "px + (100% - " + 2 * inset + "px) * " + (f / frames) + ")"
          : (f / frames) * 100 + "%";
        var isMajor = (f + start) % major === 0;
        out.push('<span class="ruler-tick' + (isMajor ? " major" : "") + '" style="left:' + left + '"></span>');
        if (isMajor) out.push('<span class="ruler-label" style="left:' + left + '">' + formatFrames(f + start, rate) + "</span>");
      }
      el.insertAdjacentHTML("beforeend", out.join(""));
    });
  }

  function formatFrames(frames, rate) {
    var s = Math.floor(frames / rate);
    var f = frames % rate;
    if (s === 0) return f + "f";
    return f === 0 ? s + "s" : s + "s" + f + "f";
  }

  function report() {
    var target = root.dataset.only ? document.getElementById(root.dataset.only) : document.documentElement;
    var rect = target.getBoundingClientRect();
    root.dataset.width = String(Math.ceil(root.dataset.only ? rect.width : document.documentElement.scrollWidth));
    root.dataset.height = String(Math.ceil(root.dataset.only ? rect.height : document.documentElement.scrollHeight));
    root.dataset.ready = "1";
  }

  document.addEventListener("DOMContentLoaded", function () {
    applyOnly();
    applyState();
    renderIcons(document);
    renderRulers();
    renderWaves();
    setTheme(root.dataset.theme);
    document.querySelectorAll("[data-theme-toggle]").forEach(function (button) {
      button.addEventListener("click", function () { setTheme(root.dataset.theme === "light" ? "dark" : "light"); });
    });
    document.querySelectorAll("a[data-keep-theme]").forEach(function (link) {
      link.addEventListener("click", function () { storeTheme(root.dataset.theme); });
    });
    (document.fonts ? document.fonts.ready : Promise.resolve()).then(report);
  });

  document.addEventListener("keydown", function (event) {
    if (event.key === "t" && !event.metaKey && !event.ctrlKey && !event.altKey && !/INPUT|TEXTAREA/.test(event.target.tagName)) {
      setTheme(root.dataset.theme === "light" ? "dark" : "light");
    }
  });

  window.KronelloPreview = { renderIcons: renderIcons, formatFrames: formatFrames };
})();
