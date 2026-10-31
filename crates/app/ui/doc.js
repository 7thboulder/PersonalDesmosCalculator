// Math document view: a page of lines. Every line is math typed as in the
// Desmos expression list, unless it is made a text line (" on an empty
// line) or a heading (#). Text lines hold math between $ signs. The engine
// side reads and writes the .gcdoc files; this page only asks.
(function () {
  'use strict';

  var MQ = MathQuill.getInterface(2);
  var panel = window.panel || { mathOptions: {}, send: function () {} };
  var send = panel.send;

  var root = document.getElementById('doc');
  var linesEl = document.getElementById('doc-lines');
  var titleEl = document.getElementById('doc-title');
  var statusEl = root.querySelector('.doc-status');
  var listEl = document.getElementById('doc-list');
  var searchEl = document.getElementById('doc-search');
  var helpBox = document.getElementById('doc-help-box');

  var lines = [];
  var loading = 0;          // building lines: edits are not changes
  var path = null;          // the document's file, once it has one
  var saveTimer = null;
  var saving = false;       // a save is waiting for the engine's answer
  var saveAgain = false;    // and the document changed meanwhile
  var dirty = false;
  var library = { folder: '', items: [] };
  var lastFocus = 0;
  var SAVE_MS = 400;
  var MERGE_MS = 700;
  var ZW = '​';

  function visible() { return document.body.classList.contains('doc-view'); }

  // ---- Lines ---------------------------------------------------------------

  function indexOf(line) { return lines.indexOf(line); }

  function mathOptions(handlers) {
    return Object.assign({}, panel.mathOptions, { handlers: handlers });
  }

  function lineShell(kind) {
    var el = document.createElement('div');
    el.className = 'dline ' + kind;
    var gut = document.createElement('div');
    gut.className = 'gut';
    el.appendChild(gut);
    return el;
  }

  function insertLine(line, at) {
    var next = lines[at];
    if (next) linesEl.insertBefore(line.el, next.el); else linesEl.appendChild(line.el);
    lines.splice(at, 0, line);
    line.el.addEventListener('focusin', function () {
      lastFocus = indexOf(line);
      lines.forEach(function (l) { l.el.classList.toggle('focused', l === line); });
    });
    line.el.addEventListener('focusout', function () { line.el.classList.remove('focused'); });
    return line;
  }

  function makeMath(latex, at) {
    var el = lineShell('math');
    var box = document.createElement('div');
    box.className = 'dmath';
    var span = document.createElement('span');
    box.appendChild(span);
    el.appendChild(box);
    var line = { kind: 'math', el: el };
    line.field = MQ.MathField(span, mathOptions({
      edit: function () { changed(); },
      enter: function () { focusLine(indexOf(line) + 1, 'start', newMath('', indexOf(line) + 1)); },
      upOutOf: function () { focusLine(indexOf(line) - 1, 'end'); },
      downOutOf: function () { focusLine(indexOf(line) + 1, 'down'); },
      moveOutOf: function (dir) {
        if (dir === MQ.L) focusLine(indexOf(line) - 1, 'end');
        else focusLine(indexOf(line) + 1, 'start');
      },
      deleteOutOf: function (dir) {
        if (dir !== MQ.L) return;
        var i = indexOf(line);
        if (line.field.latex() === '') {
          if (lines.length === 1) return;
          removeLine(line);
          if (i > 0) focusLine(i - 1, 'end'); else focusLine(0, 'start');
          changed();
        } else if (i > 0 && isEmpty(lines[i - 1])) {
          removeLine(lines[i - 1]);
          changed();
        }
      }
    }));
    if (latex) {
      loading++;
      line.field.latex(latex);
      loading--;
    }
    // " or # on an empty line makes it text or a heading, the way " makes
    // a note in Desmos.
    el.addEventListener('keydown', function (e) {
      if (e.ctrlKey || e.altKey || e.metaKey || line.field.latex() !== '') return;
      if (e.key === '"' || e.key === '#') {
        e.preventDefault();
        e.stopPropagation();
        var i = indexOf(line);
        removeLine(line);
        var made = e.key === '"' ? newText('', 'text', 1, i) : newText('', 'heading', 1, i);
        focusLine(i, 'start', made);
        changed();
      }
    }, true);
    return insertLine(line, at);
  }

  function newMath(latex, at) { return makeMath(latex, at); }

  // A text line or heading: editable text with math islands between $s.
  function newText(text, kind, level, at) {
    var el = lineShell(kind === 'heading' ? 'heading h' + level : 'text');
    var t = document.createElement('div');
    t.className = 'dtext';
    t.contentEditable = 'true';
    t.spellcheck = true;
    t.dataset.hint = kind === 'heading' ? 'Heading' : 'Text (type $ for math)';
    el.appendChild(t);
    var line = { kind: kind, level: level || 1, el: el, text: t };
    loading++;
    fillText(line, text || '');
    loading--;
    t.addEventListener('keydown', function (e) { textKey(line, e); });
    t.addEventListener('input', function () { textInput(line); });
    t.addEventListener('paste', function (e) {
      e.preventDefault();
      var plain = e.clipboardData ? e.clipboardData.getData('text/plain') : '';
      document.execCommand('insertText', false, plain);
    });
    t.addEventListener('drop', function (e) { e.preventDefault(); });
    return insertLine(line, at);
  }

  function removeLine(line) {
    var i = indexOf(line);
    if (i < 0) return;
    lines.splice(i, 1);
    line.el.remove();
  }

  function isEmpty(line) {
    if (!line) return false;
    if (line.kind === 'math') return line.field.latex() === '';
    return lineText(line) === '';
  }

  function focusLine(i, where, line) {
    line = line || lines[i];
    if (!line) return;
    // Moving down lands at the end of a math line, as between Desmos rows,
    // and at the start of text.
    if (where === 'down') where = line.kind === 'math' ? 'end' : 'start';
    if (line.kind === 'math') {
      line.field.focus();
      if (where === 'start') line.field.moveToLeftEnd(); else line.field.moveToRightEnd();
    } else {
      line.text.focus();
      caretAtEdge(line.text, where);
    }
    line.el.scrollIntoView({ block: 'nearest' });
  }

  // ---- Text lines ----------------------------------------------------------

  // Text is stored with math between $ signs; \$ and \\ are a literal $ and \.
  function serializeNodes(nodes) {
    var out = '';
    Array.prototype.forEach.call(nodes, function (n) {
      if (n.nodeType === 3) {
        out += n.nodeValue.replace(/​/g, '').replace(/\\/g, '\\\\').replace(/\$/g, '\\$');
      } else if (n.nodeType === 1) {
        if (n.classList.contains('dinl')) out += '$' + (n.dataset.latex || '') + '$';
        else if (n.tagName === 'BR') out += '\n';
        else {
          var inner = serializeNodes(n.childNodes);
          out += (out && n.tagName === 'DIV' ? '\n' : '') + inner;
        }
      }
    });
    return out;
  }

  // A line's stored text. Chromium leaves a <br> behind when the last
  // character is deleted; that is not a line break anyone typed.
  function lineText(line) {
    var t = line.text;
    var text = serializeNodes(t.childNodes);
    if (t.lastChild && t.lastChild.nodeName === 'BR') text = text.replace(/\n$/, '');
    return text;
  }

  // Splits stored text into plain runs and math.
  function parseText(text) {
    var parts = [];
    var run = '';
    var i = 0;
    while (i < text.length) {
      var c = text[i];
      if (c === '\\' && i + 1 < text.length && (text[i + 1] === '$' || text[i + 1] === '\\')) {
        run += text[i + 1];
        i += 2;
      } else if (c === '$') {
        var end = text.indexOf('$', i + 1);
        if (end < 0) { run += text.slice(i); break; }
        if (run) parts.push({ text: run });
        run = '';
        parts.push({ math: text.slice(i + 1, end) });
        i = end + 1;
      } else {
        run += c;
        i++;
      }
    }
    if (run) parts.push({ text: run });
    return parts;
  }

  function fillText(line, text) {
    var t = line.text;
    t.textContent = '';
    parseText(text).forEach(function (p) {
      if (p.math != null) t.appendChild(makeIsland(p.math, line));
      else {
        p.text.split('\n').forEach(function (piece, k) {
          if (k > 0) t.appendChild(document.createElement('br'));
          if (piece) t.appendChild(document.createTextNode(piece));
        });
      }
    });
  }

  function makeIsland(latex, line) {
    var span = document.createElement('span');
    span.className = 'dinl';
    span.contentEditable = 'false';
    var inner = document.createElement('span');
    span.appendChild(inner);
    span.dataset.latex = latex || '';
    var mf = MQ.MathField(inner, mathOptions({
      edit: function () {
        span.dataset.latex = mf.latex();
        changed();
      },
      enter: function () { caretBeside(span, 1); },
      upOutOf: function () { caretBeside(span, 1); },
      downOutOf: function () { caretBeside(span, 1); },
      moveOutOf: function (dir) { caretBeside(span, dir === MQ.L ? -1 : 1); },
      deleteOutOf: function (dir) {
        if (mf.latex() === '') {
          var after = caretBeside(span, -1);
          span.remove();
          if (after) after();
          changed();
        } else if (dir === MQ.L) {
          caretBeside(span, -1);
        }
      }
    }));
    if (latex) {
      loading++;
      mf.latex(latex);
      loading--;
    }
    span.mq = mf;
    // $ leaves the math again, as it would in LaTeX.
    span.addEventListener('keydown', function (e) {
      if (e.key === '$' && !e.ctrlKey && !e.altKey) {
        e.preventDefault();
        e.stopPropagation();
        caretBeside(span, 1);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        caretBeside(span, 1);
      }
    }, true);
    span.addEventListener('mousedown', function (e) { e.stopPropagation(); });
    return span;
  }

  // Puts the text caret just before (dir -1) or after (dir 1) a math island.
  // A zero-width space gives the caret somewhere to sit next to it.
  function caretBeside(span, dir) {
    var t = span.parentNode;
    if (!t) return null;
    var sib = dir < 0 ? span.previousSibling : span.nextSibling;
    if (!sib || sib.nodeType !== 3) {
      sib = document.createTextNode(ZW);
      t.insertBefore(sib, dir < 0 ? span : span.nextSibling);
    }
    var node = sib;
    var offset = dir < 0 ? node.nodeValue.length : 0;
    if (dir > 0 && node.nodeValue.charAt(0) === ZW) offset = 1;
    var place = function () {
      t.focus();
      var r = document.createRange();
      r.setStart(node, Math.min(offset, node.nodeValue.length));
      r.collapse(true);
      var s = window.getSelection();
      s.removeAllRanges();
      s.addRange(r);
    };
    place();
    return place;
  }

  function caretAtEdge(t, where) {
    var r = document.createRange();
    r.selectNodeContents(t);
    r.collapse(where === 'start');
    var s = window.getSelection();
    s.removeAllRanges();
    s.addRange(r);
  }

  function caretRange(t) {
    var s = window.getSelection();
    if (!s.rangeCount) return null;
    var r = s.getRangeAt(0);
    if (!t.contains(r.startContainer)) return null;
    return r;
  }

  // The text before or after the caret, as stored (math included).
  function textAround(t, r, dir) {
    var span = document.createRange();
    span.selectNodeContents(t);
    if (dir < 0) span.setEnd(r.startContainer, r.startOffset);
    else span.setStart(r.endContainer, r.endOffset);
    return serializeNodes(span.cloneContents().childNodes);
  }

  // The math island right beside a collapsed caret, if there is one.
  function islandBeside(t, r, dir) {
    var node = r.startContainer;
    var offset = r.startOffset;
    var candidate = null;
    if (node.nodeType === 3) {
      var text = node.nodeValue;
      var rest = dir < 0 ? text.slice(0, offset) : text.slice(offset);
      if (rest.replace(/​/g, '') !== '') return null;
      candidate = dir < 0 ? node.previousSibling : node.nextSibling;
    } else {
      candidate = dir < 0 ? node.childNodes[offset - 1] : node.childNodes[offset];
    }
    return candidate && candidate.nodeType === 1 && candidate.classList.contains('dinl') ? candidate : null;
  }

  function onFirstRow(t, r, last) {
    var rects = r.getClientRects();
    var box = t.getBoundingClientRect();
    var lh = parseFloat(getComputedStyle(t).lineHeight) || 24;
    if (!rects.length) return last ? textAround(t, r, 1) === '' : textAround(t, r, -1) === '';
    var c = rects[0];
    var pad = parseFloat(getComputedStyle(t).paddingTop) || 0;
    var padB = parseFloat(getComputedStyle(t).paddingBottom) || 0;
    return last ? c.bottom > box.bottom - padB - lh * 0.8 : c.top < box.top + pad + lh * 0.8;
  }

  function textKey(line, e) {
    var t = line.text;
    var r = caretRange(t);
    if (!r || e.ctrlKey || e.metaKey || e.altKey) return;
    var i = indexOf(line);
    var collapsed = r.collapsed;
    var island;
    if (e.key === '$') {
      e.preventDefault();
      r.deleteContents();
      var span = makeIsland('', line);
      r.insertNode(span);
      if (!span.nextSibling || span.nextSibling.nodeType !== 3) {
        t.insertBefore(document.createTextNode(ZW), span.nextSibling);
      }
      span.mq.focus();
      changed();
    } else if (e.key === 'Enter' && e.shiftKey) {
      e.preventDefault();
      document.execCommand('insertLineBreak');
    } else if (e.key === 'Enter') {
      e.preventDefault();
      // Enter splits the text at the caret; with nothing after it, the next
      // line is math.
      r.deleteContents();
      var before = textAround(t, r, -1);
      var after = textAround(t, r, 1);
      loading++;
      fillText(line, before);
      loading--;
      var made = after.trim() === '' ? newMath('', i + 1) : newText(after, 'text', 1, i + 1);
      focusLine(i + 1, 'start', made);
      changed();
    } else if (e.key === 'Backspace' && collapsed) {
      island = islandBeside(t, r, -1);
      if (island) {
        e.preventDefault();
        island.mq.focus();
        island.mq.moveToRightEnd();
        return;
      }
      if (textAround(t, r, -1) !== '') return;
      e.preventDefault();
      var rest = lineText(line);
      if (rest === '') {
        // An empty heading becomes text, empty text becomes math.
        removeLine(line);
        var back = line.kind === 'heading' ? newText('', 'text', 1, i) : newMath('', i);
        focusLine(i, 'start', back);
        changed();
        return;
      }
      var prev = lines[i - 1];
      if (!prev) return;
      if (isEmpty(prev)) {
        removeLine(prev);
        changed();
      } else if (prev.kind !== 'math') {
        // Join this text onto the line above.
        var joinAt = lineText(prev);
        removeLine(line);
        loading++;
        fillText(prev, joinAt + rest);
        loading--;
        prev.text.focus();
        placeAtStoredOffset(prev, joinAt);
        changed();
      } else {
        focusLine(i - 1, 'end');
      }
    } else if (e.key === 'Delete' && collapsed) {
      island = islandBeside(t, r, 1);
      if (island) {
        e.preventDefault();
        island.mq.focus();
        island.mq.moveToLeftEnd();
      }
    } else if (e.key === 'ArrowLeft' && collapsed && !e.shiftKey) {
      island = islandBeside(t, r, -1);
      if (island) {
        e.preventDefault();
        island.mq.focus();
        island.mq.moveToRightEnd();
      } else if (textAround(t, r, -1) === '' && i > 0) {
        e.preventDefault();
        focusLine(i - 1, 'end');
      }
    } else if (e.key === 'ArrowRight' && collapsed && !e.shiftKey) {
      island = islandBeside(t, r, 1);
      if (island) {
        e.preventDefault();
        island.mq.focus();
        island.mq.moveToLeftEnd();
      } else if (textAround(t, r, 1) === '' && i < lines.length - 1) {
        e.preventDefault();
        focusLine(i + 1, 'start');
      }
    } else if (e.key === 'ArrowUp' && !e.shiftKey && i > 0 && onFirstRow(t, r, false)) {
      e.preventDefault();
      focusLine(i - 1, 'end');
    } else if (e.key === 'ArrowDown' && !e.shiftKey && i < lines.length - 1 && onFirstRow(t, r, true)) {
      e.preventDefault();
      focusLine(i + 1, 'down');
    }
  }

  // Puts the caret after the first `prefix.length` stored characters, which
  // is where two joined lines meet.
  function placeAtStoredOffset(line, prefix) {
    var t = line.text;
    var want = prefix;
    var nodes = t.childNodes;
    for (var k = 0; k <= nodes.length; k++) {
      var r = document.createRange();
      r.selectNodeContents(t);
      r.setEnd(t, k);
      var got = serializeNodes(r.cloneContents().childNodes);
      if (got === want) {
        r.collapse(false);
        var s = window.getSelection();
        s.removeAllRanges();
        s.addRange(r);
        return;
      }
      if (got.length > want.length && nodes[k - 1] && nodes[k - 1].nodeType === 3) {
        // Inside a text node.
        var node = nodes[k - 1];
        var before = serializeNodes(Array.prototype.slice.call(nodes, 0, k - 1));
        var offset = want.length - before.length;
        var r2 = document.createRange();
        r2.setStart(node, Math.max(0, Math.min(offset, node.nodeValue.length)));
        r2.collapse(true);
        var s2 = window.getSelection();
        s2.removeAllRanges();
        s2.addRange(r2);
        return;
      }
    }
    caretAtEdge(t, 'end');
  }

  function textInput(line) {
    if (line.kind === 'text') {
      // "# " or "## " at the start makes a heading, as in Markdown.
      var text = lineText(line);
      var m = /^(#{1,3})[  ]/.exec(text);
      if (m) {
        var i = indexOf(line);
        removeLine(line);
        var h = newText(text.slice(m[0].length), 'heading', m[1].length, i);
        focusLine(i, 'start', h);
      }
    }
    changed();
  }

  // ---- The whole document --------------------------------------------------

  function serialize() {
    return {
      format: 'graphcalc-document',
      version: 1,
      title: titleEl.value,
      lines: lines.map(function (l) {
        if (l.kind === 'math') return { type: 'math', latex: l.field.latex() };
        var text = lineText(l);
        if (l.kind === 'heading') return { type: 'heading', level: l.level, text: text };
        return { type: 'text', text: text };
      })
    };
  }

  function build(doc) {
    loading++;
    lines.slice().forEach(removeLine);
    titleEl.value = doc.title || '';
    (doc.lines || []).forEach(function (l, i) {
      if (l.type === 'text') newText(l.text, 'text', 1, i);
      else if (l.type === 'heading') newText(l.text, 'heading', l.level || 1, i);
      else newMath(l.latex || '', i);
    });
    if (!lines.length) newMath('', 0);
    loading--;
  }

  function isBlank(doc) {
    return !doc.title && doc.lines.every(function (l) { return !(l.latex || l.text); });
  }

  function changed() {
    if (loading) return;
    dirty = true;
    snapshot();
    showStatus('Edited');
    clearTimeout(saveTimer);
    saveTimer = setTimeout(save, SAVE_MS);
  }

  function save() {
    clearTimeout(saveTimer);
    saveTimer = null;
    if (!dirty) return;
    var doc = serialize();
    // A new document gets its file once it has something in it.
    if (!path && isBlank(doc)) { dirty = false; return; }
    if (saving) { saveAgain = true; return; }
    saving = true;
    dirty = false;
    send({ type: 'docSave', path: path, doc: doc });
  }

  function flush() {
    if (saveTimer || dirty) save();
  }

  function showStatus(text, error) {
    statusEl.textContent = text;
    statusEl.classList.toggle('error', !!error);
  }

  function fileName(p) {
    return String(p || '').split(/[\\/]/).pop().replace(/\.gcdoc$/i, '');
  }

  function newDocument() {
    flush();
    path = null;
    dirty = false;
    build({ title: '', lines: [] });
    forgetHistory();
    send({ type: 'docNew' });
    showStatus('');
    renderLibrary();
    focusLine(0, 'start');
  }

  // ---- Undo ----------------------------------------------------------------

  var history = [];
  var historyAt = -1;
  var lastChange = 0;
  var undoButton = document.getElementById('doc-undo');
  var redoButton = document.getElementById('doc-redo');

  function snapshot() {
    var doc = JSON.stringify(serialize());
    var state = JSON.stringify({ doc: JSON.parse(doc), focus: lastFocus });
    var now = Date.now();
    if (history[historyAt] && JSON.stringify(JSON.parse(history[historyAt]).doc) === doc) return;
    history.length = historyAt + 1;
    // Typing in one line merges into one step; a new or removed line, or a
    // different line, starts the next step.
    var prev = history[historyAt] ? JSON.parse(history[historyAt]) : null;
    var sameLine = prev && prev.focus === lastFocus && prev.doc.lines.length === lines.length;
    if (historyAt > 0 && sameLine && now - lastChange < MERGE_MS) history[historyAt] = state;
    else history.push(state);
    historyAt = history.length - 1;
    lastChange = now;
    showUndo();
  }

  function forgetHistory() {
    history = [JSON.stringify({ doc: serialize(), focus: 0 })];
    historyAt = 0;
    lastChange = 0;
    showUndo();
  }

  function undoTo(at) {
    if (at < 0 || at >= history.length || at === historyAt) return;
    // Undo returns to the line that changed; redo goes to the redone one.
    var focus = JSON.parse(history[Math.max(at, historyAt)]).focus;
    historyAt = at;
    var state = JSON.parse(history[at]);
    build(state.doc);
    lastChange = 0;
    dirty = true;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(save, SAVE_MS);
    showUndo();
    focusLine(Math.min(focus, lines.length - 1), 'end');
  }

  function showUndo() {
    undoButton.disabled = historyAt <= 0;
    redoButton.disabled = historyAt >= history.length - 1;
  }
  undoButton.addEventListener('click', function () { undoTo(historyAt - 1); });
  redoButton.addEventListener('click', function () { undoTo(historyAt + 1); });

  // ---- Library sidebar -----------------------------------------------------

  function when(ms) {
    if (!ms) return '';
    var d = new Date(ms);
    var now = new Date();
    if (d.toDateString() === now.toDateString()) {
      return 'Today ' + d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
    }
    return d.toLocaleDateString([], { month: 'short', day: 'numeric', year: d.getFullYear() === now.getFullYear() ? undefined : 'numeric' });
  }

  function samePath(a, b) {
    return !!a && !!b && String(a).toLowerCase() === String(b).toLowerCase();
  }

  function renderLibrary() {
    var q = searchEl.value.trim().toLowerCase();
    listEl.textContent = '';
    var items = library.items.filter(function (it) {
      return !q || it.name.toLowerCase().indexOf(q) >= 0 || (it.group || '').toLowerCase().indexOf(q) >= 0;
    });
    if (!items.length) {
      var empty = document.createElement('div');
      empty.className = 'empty';
      empty.textContent = q ? 'No documents match.' : 'No documents yet. Start typing and this one is saved here.';
      listEl.appendChild(empty);
      return;
    }
    var groups = {};
    var order = [];
    items.forEach(function (it) {
      var g = it.group || '';
      if (!groups[g]) { groups[g] = []; order.push(g); }
      groups[g].push(it);
    });
    order.sort(function (a, b) { return a === '' ? -1 : b === '' ? 1 : a.localeCompare(b); });
    order.forEach(function (g) {
      if (g) {
        var head = document.createElement('div');
        head.className = 'group';
        head.textContent = g;
        listEl.appendChild(head);
      }
      groups[g].forEach(function (it) {
        var el = document.createElement('div');
        el.className = 'item' + (samePath(it.path, path) ? ' on' : '');
        el.title = it.path;
        var name = document.createElement('span');
        name.className = 'name';
        name.textContent = it.name;
        var w = document.createElement('span');
        w.className = 'when';
        w.textContent = when(it.modified);
        el.appendChild(name);
        el.appendChild(w);
        el.addEventListener('click', function () {
          if (samePath(it.path, path)) return;
          flush();
          send({ type: 'docOpen', path: it.path });
        });
        listEl.appendChild(el);
      });
    });
  }
  searchEl.addEventListener('input', renderLibrary);

  document.getElementById('doc-new').addEventListener('click', newDocument);
  document.getElementById('doc-open').addEventListener('click', function () {
    flush();
    send({ type: 'docOpen', path: null });
  });
  document.getElementById('doc-save-as').addEventListener('click', saveAs);
  document.getElementById('doc-side-toggle').addEventListener('click', function () {
    root.classList.toggle('side-closed');
  });

  function saveAs() {
    flush();
    send({ type: 'docSaveAs', doc: serialize() });
  }

  // ---- Title ---------------------------------------------------------------

  titleEl.addEventListener('input', changed);
  titleEl.addEventListener('change', function () {
    if (path) send({ type: 'docRename', path: path, title: titleEl.value });
  });
  titleEl.addEventListener('keydown', function (e) {
    if (e.key === 'Enter' || e.key === 'ArrowDown') {
      e.preventDefault();
      focusLine(0, 'start');
    }
  });

  // Clicking below the last line goes to it, or starts a new one.
  document.getElementById('doc-tail').addEventListener('mousedown', function (e) {
    e.preventDefault();
    var last = lines[lines.length - 1];
    if (last && isEmpty(last)) focusLine(lines.length - 1, 'end');
    else focusLine(lines.length, 'start', newMath('', lines.length));
  });

  // ---- Switching views -----------------------------------------------------

  function show(doc) {
    var was = visible();
    document.body.classList.toggle('doc-view', doc);
    if (doc !== was) send({ type: 'view', view: doc ? 'doc' : 'graph' });
    if (doc) {
      setTimeout(function () { focusLine(Math.min(lastFocus, lines.length - 1), 'end'); }, 0);
    } else {
      flush();
    }
  }

  root.querySelectorAll('#views button').forEach(function (b) {
    b.addEventListener('click', function () { show(b.dataset.view === 'doc'); });
  });

  function openHelp(open) {
    helpBox.classList.toggle('open', open);
  }
  document.getElementById('doc-help').addEventListener('click', function () { openHelp(true); });
  helpBox.querySelector('.close').addEventListener('click', function () { openHelp(false); });
  helpBox.addEventListener('mousedown', function (e) { if (e.target === helpBox) openHelp(false); });

  // Keys for the document. They run before the graph panel's own shortcuts,
  // which stand aside while the document shows.
  window.addEventListener('keydown', function (e) {
    var ctrl = e.ctrlKey || e.metaKey;
    var k = e.key.toLowerCase();
    if (ctrl && e.altKey && !e.shiftKey && (k === 'd' || k === 'g')) {
      e.preventDefault();
      e.stopPropagation();
      show(k === 'd');
      return;
    }
    if (!visible()) return;
    var handled = true;
    if (helpBox.classList.contains('open') && e.key === 'Escape') openHelp(false);
    else if (ctrl && !e.altKey && (k === 'z' || k === 'y') && e.target !== searchEl) {
      if (k === 'y' || e.shiftKey) undoTo(historyAt + 1); else undoTo(historyAt - 1);
    } else if (ctrl && !e.altKey && k === 's') {
      if (e.shiftKey) saveAs();
      else { dirty = true; save(); }
    } else if (ctrl && !e.altKey && !e.shiftKey && k === 'o') {
      flush();
      send({ type: 'docOpen', path: null });
    } else if (ctrl && !e.altKey && !e.shiftKey && k === 'n') {
      newDocument();
    } else if (ctrl && !e.altKey && (k === '/' || k === '?')) {
      openHelp(!helpBox.classList.contains('open'));
    } else {
      handled = false;
    }
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  }, true);

  // ---- Called by Rust through evaluate_script ------------------------------

  window.doc = {
    show: show,
    init: function (info) {
      library.folder = info.library || '';
      root.querySelector('.side-foot .folder').textContent = library.folder;
      root.querySelector('.side-foot .folder').title = library.folder;
      if (info.opened && info.opened.doc && !dirty) {
        path = info.opened.path;
        build(info.opened.doc);
        forgetHistory();
      }
      if (info.view === 'doc') show(true);
    },
    load: function (opened) {
      path = opened.path;
      dirty = false;
      clearTimeout(saveTimer);
      saveTimer = null;
      build(opened.doc);
      forgetHistory();
      showStatus('');
      renderLibrary();
      if (visible()) focusLine(0, 'start');
    },
    saved: function (info) {
      saving = false;
      path = info.path;
      showStatus('Saved');
      renderLibrary();
      if (saveAgain) {
        saveAgain = false;
        dirty = true;
        save();
      }
    },
    library: function (info) {
      library = info;
      root.querySelector('.side-foot .folder').textContent = info.folder;
      root.querySelector('.side-foot .folder').title = info.folder;
      renderLibrary();
    },
    error: function (message) {
      saving = false;
      showStatus(message, true);
    },
    // For tests: the document as it would be saved.
    serialize: serialize
  };

  build({ title: '', lines: [] });
  forgetHistory();
  renderLibrary();
  send({ type: 'docInit' });
})();
