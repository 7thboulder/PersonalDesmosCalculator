// Daily problems: the archive of dailyintegral.com's problems of the day.
// The engine fetches them (it loads the site's own pages in a hidden panel)
// and keeps them in daily.json; this page lists them by day, hides each
// problem's hints and working until revealed, and turns problems into
// document lines: a whole day as a new document, or chosen problems at the
// bottom of one that exists.
(function () {
  'use strict';

  var MQ = MathQuill.getInterface(2);
  var panel = window.panel || { send: function () {} };
  var send = panel.send;

  var root = document.getElementById('daily');
  var daysEl = document.getElementById('daily-days');
  var statusEl = document.getElementById('daily-status');
  var fetchBtn = document.getElementById('daily-fetch');
  var autoBox = document.getElementById('daily-auto');
  var addSelBtn = document.getElementById('daily-add-selected');
  var pickBox = document.getElementById('daily-pick');
  var pickList = pickBox.querySelector('.daily-pick-list');
  var pickSearch = pickBox.querySelector('.daily-pick-search');
  var pickWhat = pickBox.querySelector('.daily-pick-what');

  var archive = { fetchOnOpen: false, days: [] };
  var busy = false;
  var rendered = false;           // the list is drawn and up to date
  var revealed = {};              // problem key -> { hints: [bool, bool], solution: bool }
  var selected = {};              // problem key -> true
  var solutions = {};             // problem key -> { token, reply } once asked
  var solveToken = 0;
  var picking = null;             // the problems the document picker adds

  var KIND = { integral: 'Integral', derivative: 'Derivative', differential: 'Differential Equation' };
  var MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August',
    'September', 'October', 'November', 'December'];
  var WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];

  function showing() { return document.body.classList.contains('daily-view'); }

  function level(l) { return String(l || '').charAt(0) + String(l || '').slice(1).toLowerCase(); }
  function kindName(p) { return KIND[p.kind] || p.kind; }
  function keyOf(day, p) { return day.day + '/' + p.kind + '/' + p.level; }

  function dateParts(date) {
    var m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date || '');
    if (!m) return null;
    return { y: +m[1], m: +m[2], d: +m[3] };
  }

  // "Wednesday 25 November 2026", as the site heads the day.
  function longDate(date) {
    var p = dateParts(date);
    if (!p) return date || '';
    var wd = new Date(Date.UTC(p.y, p.m - 1, p.d)).getUTCDay();
    return WEEKDAYS[wd] + ' ' + p.d + ' ' + MONTHS[p.m - 1] + ' ' + p.y;
  }

  // The document title for a day: "Daily Integrals 11-25" (month-day).
  function docTitle(date) {
    var p = dateParts(date);
    return 'Daily Integrals ' + (p ? p.m + '-' + p.d : date);
  }

  // ---- The problem's LaTeX as lines ---------------------------------------
  //
  // The site writes a problem as LaTeX lines split by \\ (or a line of
  // backslashes), with prose in \text{...} and a {\small \text{...}} credit
  // at the end. Each line becomes a math line, or a text line with the math
  // between $ signs, as documents store it.

  // The group that starts at `open` (a "{"), and where it ends.
  function group(s, open) {
    var depth = 0;
    for (var i = open; i < s.length; i++) {
      var c = s[i];
      if (c === '\\') { i++; continue; }
      if (c === '{') depth++;
      else if (c === '}' && --depth === 0) return { body: s.slice(open + 1, i), end: i + 1 };
    }
    return { body: s.slice(open + 1), end: s.length };
  }

  // Splits on \\ outside braces.
  function splitRows(s) {
    var out = [];
    var depth = 0;
    var start = 0;
    for (var i = 0; i < s.length; i++) {
      var c = s[i];
      if (c === '{') depth++;
      else if (c === '}') depth--;
      else if (c === '\\') {
        if (s[i + 1] === '\\' && depth === 0) {
          out.push(s.slice(start, i));
          start = i + 2;
        }
        i++;
      }
    }
    out.push(s.slice(start));
    return out;
  }

  // A line as runs of prose and math. Only \text at the top is prose; one
  // inside math, as in \frac{\text{mod}(a,b)}{2}, stays in the math.
  function runs(s) {
    var out = [];
    var math = '';
    var depth = 0;
    var i = 0;
    while (i < s.length) {
      var t = depth === 0 && /^\\text(?:rm|bf|it)?\s*\{/.exec(s.slice(i));
      if (t) {
        if (math) out.push({ math: math });
        math = '';
        var g = group(s, i + t[0].length - 1);
        out.push({ text: g.body.replace(/\\([{}$%&#_ ])/g, '$1').replace(/~/g, ' ') });
        i = g.end;
      } else if (s[i] === '\\') {
        math += s.slice(i, i + 2);
        i += 2;
      } else {
        if (s[i] === '{') depth++;
        else if (s[i] === '}') depth = Math.max(0, depth - 1);
        math += s[i];
        i++;
      }
    }
    if (math) out.push({ math: math });
    return out;
  }

  // Math as MathQuill types it. \quad stays until lists are split at it.
  function tidyMath(latex) {
    return latex
      .replace(/\\dots\b/g, '\\ldots')
      .replace(/\\displaystyle\s*/g, '')
      .replace(/\\[,;:!](?![a-zA-Z])/g, ' ')
      .replace(/\s+/g, ' ')
      .trim();
  }
  function noQuads(latex) { return latex.replace(/\\q?quad\b\s*/g, '\\ ').trim(); }

  // Punctuation that ends a piece of math belongs to the sentence.
  function splitPunct(latex) {
    var m = /^([\s\S]*?)\s*([.,;:]?)\s*$/.exec(latex);
    return { math: m[1], punct: m[2] };
  }

  function escText(t) { return t.replace(/\\/g, '\\\\').replace(/\$/g, '\\$'); }

  // Lists of equations, as in P_0(x) = 1, \quad P_1(x) = x.
  function chunks(latex) {
    return latex.split(/,\s*\\q?quad\b|\\q?quad\b/).map(function (c) { return c.trim(); })
      .filter(function (c) { return c && !/^[\s.,;:]*$/.test(c); });
  }

  // { lines: [{ type: 'math', latex } | { type: 'text', text }], credit,
  //   pieces: [{ latex, asks }] } where pieces are the math in order, each
  // with how many times "find" (or the like) came before it.
  function statement(raw) {
    var credit = [];
    var body = String(raw || '').replace(/\{\s*\\(?:small|footnotesize|scriptsize|tiny)\s*([\s\S]*?)\}(?=\s*(?:\\\\|\n|$))/g, function (all, inner) {
      runs(inner).forEach(function (r) { if (r.text) credit.push(r.text.trim()); });
      return '';
    });
    var rows = [];
    // A line that is only backslashes, or blank, ends a row.
    var block = [];
    body.split('\n').forEach(function (line) {
      if (/^\s*\\*\s*$/.test(line)) {
        if (block.length) rows.push(block.join(' '));
        block = [];
      } else block.push(line);
    });
    if (block.length) rows.push(block.join(' '));
    var lines = [];
    var pieces = [];
    var asked = 0;
    var ASK = /\b(find|evaluate|compute|determine|calculate|what is|show that)\b/i;
    rows.forEach(function (row) {
      splitRows(row).forEach(function (part) {
        part = part.trim();
        if (!part) return;
        var rs = runs(part);
        var hasText = rs.some(function (r) { return r.text != null && r.text.trim(); });
        if (!hasText) {
          var m = splitPunct(tidyMath(part));
          if (!m.math) return;
          lines.push({ type: 'math', latex: noQuads(m.math) });
          chunks(m.math).forEach(function (c) { pieces.push({ latex: noQuads(splitPunct(c).math), asks: asked }); });
          return;
        }
        var text = '';
        rs.forEach(function (r) {
          if (r.text != null) {
            text += escText(r.text);
            if (ASK.test(r.text)) asked++;
            return;
          }
          var latex = tidyMath(r.math);
          if (/^[\s.,;:]*$/.test(latex)) { text += escText(r.math.replace(/\s+/g, ' ')); return; }
          var lead = /^\s/.test(r.math) && text && !/\s$/.test(text) ? ' ' : '';
          var cs = chunks(latex);
          var bits = cs.map(function (c) {
            var m = splitPunct(c);
            pieces.push({ latex: noQuads(m.math), asks: asked });
            return '$' + noQuads(m.math) + '$' + m.punct;
          });
          var end = splitPunct(latex).punct;
          var joined = bits.join(', ');
          if (end && !/[.,;:]$/.test(joined)) joined += end;
          text += lead + joined + (/\s$/.test(r.math) ? ' ' : '');
        });
        text = text.replace(/\s+/g, ' ').trim();
        if (!text) return;
        // The site breaks long sentences over lines; a sentence that runs
        // on stays one line here.
        var prev = lines[lines.length - 1];
        if (prev && prev.type === 'text' && !/[.:?!]\$?$/.test(prev.text)) prev.text += ' ' + text;
        else lines.push({ type: 'text', text: text });
      });
    });
    var who = credit.join(' ').replace(/\s+/g, ' ').trim()
      .replace(/^This problem was generously submitted\s*/i, 'Submitted ');
    return { lines: lines, credit: who, pieces: pieces };
  }

  var RELATION = /(^|[^<>!\\])=/;
  function isNumber(latex) { return /^[-+]?\s*[\d.]+$/.test(latex.trim()); }
  function isDiffEq(latex) {
    return RELATION.test(latex) && /'|\\frac\{d|\\frac\{\\partial|\\partial|\\dot/.test(latex);
  }

  // What Solve works on, with the equations given as the facts it may use.
  // A differential equation is solved itself, its conditions alongside;
  // otherwise it is the math the problem asks to find.
  function solveRequest(p) {
    var st = statement(p.latex);
    var pieces = st.pieces.filter(function (x) { return x.latex; });
    if (!pieces.length) return null;
    var target = null;
    if (p.kind === 'differential') target = pieces.filter(function (x) { return isDiffEq(x.latex); })[0] || null;
    // The first math after the last "find".
    var most = Math.max.apply(null, pieces.map(function (x) { return x.asks; }));
    if (!target && most > 0) {
      target = pieces.filter(function (x) { return x.asks === most && !isNumber(x.latex); })[0] || null;
    }
    if (!target) {
      var plain = pieces.filter(function (x) { return !RELATION.test(x.latex) && !isNumber(x.latex); });
      target = plain.length ? plain[plain.length - 1] : pieces[pieces.length - 1];
    }
    var others = pieces.filter(function (x) { return x !== target && RELATION.test(x.latex); })
      .map(function (x) { return x.latex; });
    return { latex: target.latex, lines: others };
  }

  // A problem's lines for a document, under a heading.
  function problemLines(p, heading) {
    var st = statement(p.latex);
    var out = [{ type: 'heading', level: 2, text: heading }];
    if (p.title) out.push({ type: 'text', text: escText(p.title) });
    st.lines.forEach(function (l) { out.push(l); });
    return out;
  }

  function dayDocument(day) {
    var lines = [];
    var n = 0;
    day.problems.forEach(function (p) {
      if (!p.latex) return;
      n++;
      lines = lines.concat(problemLines(p, 'Problem ' + n + ': ' + kindName(p) + ' (' + level(p.level) + ')'));
    });
    lines.push({ type: 'math', latex: '' });
    return { format: 'graphcalc-document', version: 1, title: docTitle(day.date), lines: lines };
  }

  // ---- Drawing -------------------------------------------------------------

  function staticMath(el, latex) {
    try { MQ.StaticMath(el).latex(latex); } catch (e) { el.textContent = latex; }
  }

  // Text with math between $ signs, as documents store it.
  function richText(el, text) {
    var run = '';
    var flush = function () {
      if (run) el.appendChild(document.createTextNode(run));
      run = '';
    };
    for (var i = 0; i < text.length; i++) {
      var c = text[i];
      if (c === '\\' && (text[i + 1] === '$' || text[i + 1] === '\\')) { run += text[i + 1]; i++; continue; }
      if (c === '$') {
        var end = text.indexOf('$', i + 1);
        if (end < 0) { run += text.slice(i); break; }
        flush();
        var span = document.createElement('span');
        span.className = 'daily-inline';
        el.appendChild(span);
        staticMath(span, text.slice(i + 1, end));
        i = end;
        continue;
      }
      run += c;
    }
    flush();
  }

  function button(text, cls, title, onClick) {
    var b = document.createElement('button');
    b.textContent = text;
    if (cls) b.className = cls;
    if (title) b.title = title;
    b.addEventListener('click', onClick);
    return b;
  }

  function render() {
    autoBox.checked = !!archive.fetchOnOpen;
    fetchBtn.disabled = busy;
    if (!showing()) { rendered = false; return; }
    rendered = true;
    daysEl.textContent = '';
    var days = (archive.days || []).filter(function (d) {
      return (d.problems || []).some(function (p) { return p.latex; });
    });
    if (!days.length) {
      var empty = document.createElement('div');
      empty.className = 'daily-empty';
      empty.textContent = busy ? 'Fetching today\'s problems…' :
        'No problems saved yet. Press "Fetch today\'s problems" to save today\'s.';
      daysEl.appendChild(empty);
    }
    days.forEach(function (day) { daysEl.appendChild(renderDay(day)); });
    showSelected();
  }

  function renderDay(day) {
    var box = document.createElement('section');
    box.className = 'daily-day';
    var head = document.createElement('div');
    head.className = 'daily-day-head';
    var h = document.createElement('h2');
    h.textContent = longDate(day.date);
    head.appendChild(h);
    var count = day.problems.filter(function (p) { return p.latex; }).length;
    var n = document.createElement('span');
    n.className = 'daily-count';
    n.textContent = count + (count === 1 ? ' problem' : ' problems');
    head.appendChild(n);
    var grow = document.createElement('span');
    grow.className = 'grow';
    head.appendChild(grow);
    head.appendChild(button('Open day as a document', 'primary',
      'Make a new document, "' + docTitle(day.date) + '", with all of this day\'s problems',
      function () { openDay(day); }));
    box.appendChild(head);
    day.problems.forEach(function (p) {
      if (p.latex) box.appendChild(renderProblem(day, p));
    });
    return box;
  }

  function renderProblem(day, p) {
    var key = keyOf(day, p);
    var state = revealed[key] || (revealed[key] = { hints: [false, false], solution: false });
    var card = document.createElement('article');
    card.className = 'daily-problem';
    card.dataset.key = key;

    var top = document.createElement('div');
    top.className = 'daily-problem-top';
    var tick = document.createElement('input');
    tick.type = 'checkbox';
    tick.title = 'Choose this problem to add to a document';
    tick.checked = !!selected[key];
    tick.addEventListener('change', function () {
      if (tick.checked) selected[key] = true; else delete selected[key];
      showSelected();
    });
    top.appendChild(tick);
    var badge = document.createElement('span');
    badge.className = 'daily-badge ' + p.kind + ' ' + String(p.level).toLowerCase();
    badge.textContent = kindName(p) + ' · ' + level(p.level);
    top.appendChild(badge);
    var title = document.createElement('span');
    title.className = 'daily-title';
    title.textContent = p.title || '';
    top.appendChild(title);
    top.appendChild(button('Add to a document…', 'daily-add',
      'Add this problem at the bottom of a document', function () { pick([{ day: day, p: p }]); }));
    card.appendChild(top);

    var st = statement(p.latex);
    var body = document.createElement('div');
    body.className = 'daily-statement';
    st.lines.forEach(function (l) {
      var div = document.createElement('div');
      if (l.type === 'math') {
        div.className = 'daily-math';
        var span = document.createElement('span');
        div.appendChild(span);
        staticMath(span, l.latex);
      } else {
        div.className = 'daily-text';
        richText(div, l.text);
      }
      body.appendChild(div);
    });
    if (st.credit) {
      var cr = document.createElement('div');
      cr.className = 'daily-credit';
      cr.textContent = st.credit;
      body.appendChild(cr);
    }
    card.appendChild(body);

    var reveal = document.createElement('div');
    reveal.className = 'daily-reveal';
    var hints = (p.hints || []).filter(function (h) { return h; });
    (p.hints || []).forEach(function (hint, i) {
      if (!hint) return;
      var row = document.createElement('div');
      row.className = 'daily-hint';
      if (state.hints[i]) {
        var label = document.createElement('b');
        label.textContent = 'Hint ' + (i + 1) + ': ';
        row.appendChild(label);
        var t = document.createElement('span');
        richText(t, hint.replace(/\\\(([\s\S]*?)\\\)/g, function (a, m) { return '$' + m + '$'; }));
        row.appendChild(t);
        row.appendChild(button('Hide', 'daily-link', 'Hide this hint', function () {
          state.hints[i] = false;
          redraw(card, day, p);
        }));
      } else {
        row.appendChild(button('Reveal hint ' + (i + 1), 'daily-reveal-btn', 'Show hint ' + (i + 1), function () {
          state.hints[i] = true;
          redraw(card, day, p);
        }));
      }
      reveal.appendChild(row);
    });
    if (!hints.length) {
      var none = document.createElement('div');
      none.className = 'daily-none';
      none.textContent = 'The site gives no hints for this one.';
      reveal.appendChild(none);
    }
    var sol = document.createElement('div');
    sol.className = 'daily-solution';
    if (state.solution) {
      sol.appendChild(renderSolution(key, p));
      sol.appendChild(button('Hide solution', 'daily-link', 'Hide the working', function () {
        state.solution = false;
        redraw(card, day, p);
      }));
    } else {
      sol.appendChild(button('Reveal solution', 'daily-reveal-btn',
        'Work it out with Solve, step by step', function () {
          state.solution = true;
          if (!solutions[key]) solve(key, p);
          redraw(card, day, p);
        }));
    }
    reveal.appendChild(sol);
    card.appendChild(reveal);
    return card;
  }

  function redraw(card, day, p) {
    if (card.parentNode) card.parentNode.replaceChild(renderProblem(day, p), card);
  }

  function findCard(key) {
    return daysEl.querySelector('.daily-problem[data-key="' + key + '"]');
  }

  // ---- Solutions -----------------------------------------------------------

  function solve(key, p) {
    var req = solveRequest(p);
    if (!req) {
      solutions[key] = { reply: { error: 'There is no math in this problem to work on.' } };
      return;
    }
    var job = { token: ++solveToken, request: req, started: Date.now(), reply: null };
    solutions[key] = job;
    send({ type: 'dailySolve', token: job.token, latex: req.latex, op: null, lines: req.lines });
    job.timer = setInterval(function () {
      var w = document.querySelector('.daily-problem[data-key="' + key + '"] .dsolve-wait');
      if (!w || job.reply) { if (job.reply || !showing()) clearInterval(job.timer); return; }
      var secs = Math.floor((Date.now() - job.started) / 1000);
      w.textContent = 'Solving' + (secs >= 1 ? ' (' + secs + ' s)' : '') + '…';
    }, 500);
  }

  function solved(msg) {
    var key = null;
    Object.keys(solutions).forEach(function (k) { if (solutions[k].token === msg.token) key = k; });
    if (!key) return;
    var job = solutions[key];
    clearInterval(job.timer);
    job.reply = msg.reply || {};
    var card = findCard(key);
    if (card && showing()) {
      var day = null, p = null;
      (archive.days || []).forEach(function (d) {
        (d.problems || []).forEach(function (q) { if (keyOf(d, q) === key) { day = d; p = q; } });
      });
      if (day) redraw(card, day, p);
    }
  }

  function renderSolution(key, p) {
    var job = solutions[key];
    var box = document.createElement('div');
    box.className = 'dsolve daily-dsolve';
    if (job && job.request) {
      var what = document.createElement('div');
      what.className = 'daily-solving';
      what.appendChild(document.createTextNode('Solve works on '));
      var m = document.createElement('span');
      m.className = 'daily-inline';
      box.appendChild(what);
      what.appendChild(m);
      staticMath(m, job.request.latex);
      if (job.request.lines.length) {
        what.appendChild(document.createTextNode(' using '));
        job.request.lines.forEach(function (l, i) {
          if (i) what.appendChild(document.createTextNode(', '));
          var s = document.createElement('span');
          s.className = 'daily-inline';
          what.appendChild(s);
          staticMath(s, l);
        });
      }
    }
    var r = job && job.reply;
    if (!r) {
      var wait = document.createElement('div');
      wait.className = 'dsolve-wait';
      wait.textContent = 'Solving…';
      box.appendChild(wait);
      return box;
    }
    var list = document.createElement('ol');
    list.className = 'dsolve-steps';
    (r.steps || []).forEach(function (st) {
      var li = document.createElement('li');
      if (st.note) {
        var note = document.createElement('div');
        note.className = 'note';
        richText(note, st.note);
        li.appendChild(note);
      }
      if (st.latex) {
        var d = document.createElement('div');
        d.className = 'math';
        var s = document.createElement('span');
        d.appendChild(s);
        staticMath(s, st.latex);
        li.appendChild(d);
      }
      list.appendChild(li);
    });
    if (list.firstChild) box.appendChild(list);
    if (r.result) {
      var res = document.createElement('div');
      res.className = 'dsolve-result';
      var rs = document.createElement('span');
      res.appendChild(rs);
      staticMath(rs, r.result);
      box.appendChild(res);
    }
    if (r.error || (!r.result && !(r.steps || []).length)) {
      var err = document.createElement('div');
      err.className = 'dsolve-error';
      richText(err, r.error ? 'Solve couldn\'t finish this one: ' + r.error :
        'Solve couldn\'t work this one out yet.');
      box.appendChild(err);
    }
    return box;
  }

  // ---- Into documents ------------------------------------------------------

  function openDay(day) {
    send({ type: 'dailyNewDoc', doc: dayDocument(day) });
    setStatus('Making "' + docTitle(day.date) + '"…');
  }

  function selectedProblems() {
    var out = [];
    (archive.days || []).forEach(function (d) {
      (d.problems || []).forEach(function (p) { if (selected[keyOf(d, p)] && p.latex) out.push({ day: d, p: p }); });
    });
    return out;
  }

  function showSelected() {
    var n = selectedProblems().length;
    addSelBtn.disabled = !n;
    addSelBtn.textContent = n ? 'Add ' + n + ' selected to a document…' : 'Add selected to a document…';
  }

  function addedLines(items) {
    var lines = [];
    items.forEach(function (it) {
      lines = lines.concat(problemLines(it.p, docTitle(it.day.date) + ': ' + kindName(it.p) +
        ' (' + level(it.p.level) + ')'));
    });
    lines.push({ type: 'math', latex: '' });
    return lines;
  }

  function pick(items) {
    picking = items;
    pickWhat.textContent = items.length === 1
      ? kindName(items[0].p) + ' (' + level(items[0].p.level) + ') goes at the bottom of the document you choose.'
      : items.length + ' problems go at the bottom of the document you choose.';
    pickSearch.value = '';
    renderPick();
    pickBox.classList.add('open');
    pickSearch.focus();
  }

  function renderPick() {
    var q = pickSearch.value.trim().toLowerCase();
    var items = window.doc && window.doc.libraryItems ? window.doc.libraryItems() : [];
    var current = window.doc && window.doc.currentPath ? window.doc.currentPath() : null;
    pickList.textContent = '';
    items.filter(function (it) {
      return !q || it.name.toLowerCase().indexOf(q) >= 0 || (it.group || '').toLowerCase().indexOf(q) >= 0;
    }).forEach(function (it) {
      var b = document.createElement('button');
      b.className = 'daily-pick-item';
      var name = document.createElement('span');
      name.textContent = it.name;
      b.appendChild(name);
      var info = document.createElement('span');
      info.className = 'where';
      info.textContent = (it.group ? it.group : '') + (current && samePath(current, it.path) ? (it.group ? ' · ' : '') + 'open now' : '');
      b.appendChild(info);
      b.addEventListener('click', function () { addTo(it.path); });
      pickList.appendChild(b);
    });
    if (!pickList.firstChild) {
      var none = document.createElement('div');
      none.className = 'daily-none';
      none.textContent = q ? 'No documents match.' : 'There are no documents yet. Use "Open day as a document" instead.';
      pickList.appendChild(none);
    }
  }

  function samePath(a, b) { return String(a).toLowerCase() === String(b).toLowerCase(); }

  function addTo(path) {
    var items = picking || [];
    closePick();
    if (!items.length) return;
    var lines = addedLines(items);
    var current = window.doc && window.doc.currentPath ? window.doc.currentPath() : null;
    if (current && samePath(current, path)) {
      // The open document takes them itself, so nothing typed is lost.
      window.doc.appendLines(lines);
      window.doc.show(true);
    } else {
      if (window.doc && window.doc.flush) window.doc.flush();
      send({ type: 'dailyAppend', path: path, lines: lines });
    }
    selected = {};
    showSelected();
  }

  function closePick() {
    pickBox.classList.remove('open');
    picking = null;
  }

  pickSearch.addEventListener('input', renderPick);
  pickBox.querySelector('.cancel').addEventListener('click', closePick);
  pickBox.addEventListener('mousedown', function (e) { if (e.target === pickBox) closePick(); });
  pickBox.addEventListener('keydown', function (e) {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closePick(); }
  });
  addSelBtn.addEventListener('click', function () {
    var items = selectedProblems();
    if (items.length) pick(items);
  });

  // ---- Fetching and settings ----------------------------------------------

  function setStatus(text, error) {
    statusEl.textContent = text || '';
    statusEl.classList.toggle('error', !!error);
  }

  fetchBtn.addEventListener('click', function () {
    if (busy) return;
    busy = true;
    fetchBtn.disabled = true;
    setStatus('Opening dailyintegral.com…');
    send({ type: 'dailyFetch' });
  });

  autoBox.addEventListener('change', function () {
    archive.fetchOnOpen = autoBox.checked;
    send({ type: 'dailySetting', fetchOnOpen: autoBox.checked });
  });

  // ---- Showing the page ----------------------------------------------------

  function setViewButtons(view) {
    document.querySelectorAll('#views button').forEach(function (b) {
      b.classList.toggle('on', b.dataset.view === view);
    });
  }

  function toggle(on) {
    var was = showing();
    document.body.classList.toggle('daily-view', on);
    setViewButtons(on ? 'daily' : 'doc');
    if (!on) closePick();
    if (on && (!was || !rendered)) render();
  }

  function open() {
    if (window.doc) window.doc.show(true);
    toggle(true);
    root.scrollTop = 0;
  }

  // ---- Called by Rust through evaluate_script ------------------------------

  window.daily = {
    open: open,
    toggle: toggle,
    init: function (info) {
      archive = info.archive || archive;
      busy = !!info.busy;
      if (busy) setStatus('Fetching today\'s problems…');
      rendered = false;
      render();
    },
    archive: function (a) {
      archive = a;
      rendered = false;
      render();
    },
    progress: function (p) {
      busy = true;
      fetchBtn.disabled = true;
      setStatus('Fetching ' + (p.at + 1) + ' of ' + p.total + ': ' + (KIND[p.kind] || p.kind) + ' (' + level(p.level) + ')…');
    },
    fetched: function (r) {
      busy = false;
      if (r.archive) archive = r.archive;
      rendered = false;
      render();
      if (r.error) { setStatus(r.error, true); return; }
      var msg = 'Saved ' + r.saved + (r.saved === 1 ? ' problem' : ' problems') + '.';
      if ((r.failed || []).length) {
        msg += ' These didn\'t load: ' + r.failed.map(function (f) {
          var bits = f.split(' ');
          return (KIND[bits[0]] || bits[0]) + ' (' + level(bits[1]) + ')';
        }).join(', ') + '.';
      }
      if (!r.saved) msg = 'Nothing came from dailyintegral.com. Check the internet connection and try again.';
      setStatus(msg, !r.saved);
    },
    status: function (s) { setStatus(s.error || s.text || '', !!s.error); },
    solved: solved,
    opened: function () {
      setStatus('');
      if (window.doc) window.doc.show(true);
    },
    // For tests.
    statement: statement,
    solveRequest: solveRequest,
    dayDocument: dayDocument,
    addedLines: addedLines
  };

  send({ type: 'dailyInit' });
})();
