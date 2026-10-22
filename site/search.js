// Поиск по документации: ищет по заголовкам и тексту разделов, без внешних сервисов.
(function () {
  var box = document.querySelector(".side-box");
  if (box && window.matchMedia("(max-width: 52rem)").matches) box.open = false;
  var input = document.getElementById("q");
  var list = document.getElementById("hits");
  if (!input || !list) return;
  var base = document.currentScript.src.replace(/search\.js.*$/, "");
  var index = null;

  function load() {
    if (index) return Promise.resolve(index);
    return fetch(base + "search-index.json")
      .then(function (r) { return r.json(); })
      .then(function (data) { index = data; return data; });
  }

  function score(entry, words) {
    var title = entry.title.toLowerCase();
    var text = entry.text.toLowerCase();
    var total = 0;
    for (var i = 0; i < words.length; i++) {
      var inTitle = title.indexOf(words[i]) !== -1;
      var inText = text.indexOf(words[i]) !== -1;
      if (!inTitle && !inText) return 0;
      total += inTitle ? 3 : 1;
    }
    return total;
  }

  input.addEventListener("input", function () {
    var q = input.value.trim().toLowerCase();
    if (q.length < 2) { list.hidden = true; list.textContent = ""; return; }
    load().then(function (data) {
      var words = q.split(/\s+/);
      var hits = data
        .map(function (e) { return { e: e, s: score(e, words) }; })
        .filter(function (h) { return h.s > 0; })
        .sort(function (a, b) { return b.s - a.s; })
        .slice(0, 8);
      list.textContent = "";
      if (!hits.length) {
        var none = document.createElement("li");
        none.textContent = "Ничего не найдено";
        list.appendChild(none);
      }
      hits.forEach(function (h) {
        var li = document.createElement("li");
        var a = document.createElement("a");
        a.href = base + "docs/" + h.e.url.replace(/^docs\//, "");
        a.textContent = h.e.title;
        var small = document.createElement("span");
        small.textContent = h.e.page;
        li.appendChild(a);
        li.appendChild(small);
        list.appendChild(li);
      });
      list.hidden = false;
    });
  });
})();
