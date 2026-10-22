/**
 * PuLinTo 2-line signature demo:
 * Line 1: types mistyped word + space -> strikes out with a line
 * Line 2: simultaneously types the corrected word below
 * Strictly alternating between EN -> RU and RU -> EN (IT terms, commands, keywords).
 */
(function() {
  const PAIRS = [
    // 1. EN -> RU (Daily)
    { wrong: "ghbdtn", right: "привет" },
    // 2. RU -> EN (IT command)
    { wrong: "пше", right: "git" },
    // 3. EN -> RU (Daily)
    { wrong: "ctqxfc", right: "сейчас" },
    // 4. RU -> EN (Daily)
    { wrong: "руддщ", right: "hello" },
    // 5. EN -> RU (Daily)
    { wrong: "djj,ot", right: "вообще" },
    // 6. RU -> EN (IT command)
    { wrong: "сщььше", right: "commit" },
    // 7. EN -> RU (Daily)
    { wrong: "hfcrdflrf", right: "раскладка" },
    // 8. RU -> EN (IT command)
    { wrong: "ыыр", right: "ssh" },
    // 9. EN -> RU (Daily)
    { wrong: "ujnjdj", right: "готово" },
    // 10. RU -> EN (IT)
    { wrong: "икфтср", right: "branch" },
    // 11. EN -> RU (Daily)
    { wrong: "ghbrjp", right: "прикол" },
    // 12. RU -> EN (IT)
    { wrong: "вуздщн", right: "deploy" },
    // 13. EN -> RU (Daily)
    { wrong: "ybrjulf", right: "никогда" },
    // 14. RU -> EN (IT)
    { wrong: "вщслкер", right: "docker" },
    // 15. EN -> RU (Daily)
    { wrong: "cnfhn", right: "старт" },
    // 16. RU -> EN (Abbr)
    { wrong: "шз", right: "ip" },
    // 17. EN -> RU (Daily)
    { wrong: "ntcn", right: "тест" },
    // 18. RU -> EN (IT)
    { wrong: "ыйд", right: "sql" },
    // 19. EN -> RU (Daily)
    { wrong: "htkbpt", right: "релизе" },
    // 20. RU -> EN (Abbr)
    { wrong: "ФЗШ", right: "API" },
    // 21. EN -> RU (Daily)
    { wrong: "jndtn", right: "ответ" },
    // 22. RU -> EN (IT)
    { wrong: "кузщ", right: "repo" },
    // 23. EN -> RU (Daily)
    { wrong: "ghjuhfvvf", right: "программа" },
    // 24. RU -> EN (IT keyword)
    { wrong: "сщтые", right: "const" },
    // 25. EN -> RU (Tricky with ;)
    { wrong: "rhby;", right: "кринж" },
    // 26. RU -> EN (IT keyword)
    { wrong: "фгтсе", right: "async" },
    // 27. EN -> RU (Full phrase)
    { wrong: "ghbdtn vbh", right: "привет мир" },
    // 28. RU -> EN (IT keyword)
    { wrong: "фцфше", right: "await" },
    // 29. EN -> RU (Daily)
    { wrong: "рщц", right: "how" },
    // 30. RU -> EN (IT keyword)
    { wrong: "ыекгсе", right: "struct" },
    // 31. RU -> EN (Daily)
    { wrong: "цщкдв", right: "world" },
    // 32. RU -> EN (IT keyword)
    { wrong: "шьзщке", right: "import" },
    // 33. RU -> EN (Article)
    { wrong: "еру", right: "the" },
    // 34. RU -> EN (IT keyword)
    { wrong: "куегкт", right: "return" },
    // 35. RU -> EN (Compound)
    { wrong: "ыыв-диск", right: "SSD-диск" },
    // 36. RU -> EN (IT)
    { wrong: "ыукмук", right: "server" },
    // 37. RU -> EN (Compound)
    { wrong: "шз-адрес", right: "IP-адрес" },
    // 38. RU -> EN (IT)
    { wrong: "сдшуте", right: "client" },
    // 39. RU -> EN (Abbr)
    { wrong: "афй", right: "faq" },
    // 40. RU -> EN (IT)
    { wrong: "куйгуые", right: "request" },
    // 41. RU -> EN (Abbr)
    { wrong: "зс", right: "pc" },
    // 42. RU -> EN (Abbr)
    { wrong: "гш", right: "ui" }
  ];

  const wrongEl = document.getElementById('demo-wrong');
  const rightEl = document.getElementById('demo-right');
  const wrongCaret = document.getElementById('demo-wrong-caret');
  const rightCaret = document.getElementById('demo-right-caret');
  const screenEl = document.getElementById('demo-screen');

  if (!wrongEl || !rightEl) return;

  let index = 0;

  function sleep(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
  }

  async function loop() {
    while (true) {
      const pair = PAIRS[index];
      index = (index + 1) % PAIRS.length;

      // Reset
      screenEl.style.opacity = '1';
      wrongEl.textContent = '';
      rightEl.textContent = '';
      wrongEl.style.setProperty('--strike-width', '0%');
      if (wrongCaret) wrongCaret.style.display = 'inline-block';
      if (rightCaret) rightCaret.style.display = 'none';

      // 1. Type wrong word
      for (let i = 0; i < pair.wrong.length; i++) {
        wrongEl.textContent += pair.wrong[i];
        await sleep(50 + Math.random() * 40);
      }

      // Space trigger
      await sleep(100);
      wrongEl.textContent += ' ';
      await sleep(180);

      // 2. Animate strikethrough line
      wrongEl.style.setProperty('--strike-width', '100%');
      if (wrongCaret) wrongCaret.style.display = 'none';
      if (rightCaret) rightCaret.style.display = 'inline-block';

      // 3. Type right word on line 2 in sync
      await sleep(60);
      for (let i = 0; i < pair.right.length; i++) {
        rightEl.textContent += pair.right[i];
        await sleep(40 + Math.random() * 30);
      }

      // 4. Pause to read both lines
      await sleep(2300);

      // 5. Fade out before next word
      screenEl.style.transition = 'opacity 0.22s ease';
      screenEl.style.opacity = '0';
      await sleep(240);
      screenEl.style.transition = '';
    }
  }

  loop();
})();
