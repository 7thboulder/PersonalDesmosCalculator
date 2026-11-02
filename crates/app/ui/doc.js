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

  // Documents also write lim as an operator, as in lim_{h→0}.
  var DOC_OPTIONS = Object.assign({}, panel.mathOptions, {
    autoOperatorNames: (panel.mathOptions.autoOperatorNames || 'sin cos tan ln log') + ' lim'
  });

  function mathOptions(handlers) {
    return Object.assign({}, DOC_OPTIONS, { handlers: handlers });
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

  // Line ids for references: short, unique enough within a document.
  function newId() { return 'g' + Math.random().toString(36).slice(2, 9); }

  function makeMath(latex, at, opts) {
    opts = opts || {};
    var el = lineShell('math');
    var box = document.createElement('div');
    box.className = 'dmath';
    var span = document.createElement('span');
    box.appendChild(span);
    el.appendChild(box);
    var line = { kind: 'math', el: el, step: !!opts.step, id: opts.id || newId(), live: !!opts.live };
    // A live line computes: its value shows on the right, as in Desmos.
    var res = document.createElement('div');
    res.className = 'dres';
    el.appendChild(res);
    line.resEl = res;
    var dot = document.createElement('button');
    dot.className = 'dlive';
    dot.tabIndex = -1;
    dot.title = 'Live: compute this line (Ctrl+L)';
    dot.addEventListener('mousedown', function (e) { e.preventDefault(); });
    dot.addEventListener('click', function () { setLive([line], !line.live); });
    el.appendChild(dot);
    // A live line that draws a curve can go in a graph block below it.
    var gb = document.createElement('button');
    gb.className = 'dgraphbtn';
    gb.tabIndex = -1;
    gb.title = 'Graph this line (Ctrl+Shift+G)';
    gb.textContent = 'Graph';
    gb.addEventListener('mousedown', function (e) { e.preventDefault(); });
    gb.addEventListener('click', function () { graphLine(line); });
    el.appendChild(gb);
    el.classList.toggle('live', line.live);
    line.field = MQ.MathField(span, mathOptions({
      edit: function () { clearCheck(line); changed(); },
      enter: function () {
        var i = indexOf(line);
        var latex = line.field.latex();
        // Enter on an empty step ends the group of steps.
        if (line.step && latex === '') {
          line.step = false;
          changed();
          return;
        }
        // After an equation (or inside a group) the next line is a step,
        // lined up under it.
        var step = line.step || relationAt(latex) >= 0;
        focusLine(i + 1, 'start', newMath('', i + 1, { step: step }));
        changed();
      },
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
    // Check as I type: leaving a changed line checks its group again.
    el.addEventListener('focusout', function (e) {
      if (!checkAsType || el.contains(e.relatedTarget)) return;
      if (line.checkedLatex === line.field.latex()) return;
      setTimeout(function () { if (lines.indexOf(line) >= 0) checkGroups([groupOf(line)], false); }, 0);
    });
    el.addEventListener('keydown', function (e) {
      var i = indexOf(line);
      if (e.key === 'Enter' && e.shiftKey && !e.ctrlKey && !e.altKey) {
        // Shift+Enter: a plain line, outside the group.
        e.preventDefault();
        e.stopPropagation();
        focusLine(i + 1, 'start', newMath('', i + 1));
        changed();
        return;
      }
      if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && !e.altKey) {
        // Carry down: the line again as the next step. An equation's copy
        // starts at its relation, ready for the next = ...
        e.preventDefault();
        e.stopPropagation();
        var latex = line.field.latex();
        var at = relationAt(latex);
        var copy = at > 0 ? latex.slice(at) : latex;
        focusLine(i + 1, 'end', newMath(copy, i + 1, { step: true }));
        changed();
        return;
      }
      if (e.key === '@' && !e.ctrlKey && !e.altKey) {
        e.preventDefault();
        e.stopPropagation();
        var cursor = el.querySelector('.mq-cursor');
        openRefPicker(cursor || el, function (id) {
          line.field.focus();
          if (id) line.field.write('\\embed{ref}[' + id + ']');
        });
        return;
      }
      // " or # on an empty line makes it text or a heading, the way " makes
      // a note in Desmos.
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

  function newMath(latex, at, opts) {
    var line = makeMath(latex, at, opts);
    scheduleLayout();
    return line;
  }

  // ---- Steps, alignment and equation numbers -------------------------------

  var RELATION_CMDS = ['le', 'leq', 'ge', 'geq', 'ne', 'neq', 'approx', 'to', 'Rightarrow',
    'Leftarrow', 'Leftrightarrow', 'iff', 'implies', 'sim', 'equiv', 'lt', 'gt'];
  var RELATION_SYMBOLS = ['=', '<', '>', '\u2264', '\u2265', '\u2260', '\u2248', '\u2192', '\u21D2',
    '\u21D0', '\u21D4', '\u223C', '\u2261', '\u27F9', '\u27FA'];

  // Where the first relation (=, <, \le, ...) outside any brackets starts
  // in a line's LaTeX, or -1.
  function relationAt(latex) {
    var depth = 0;
    for (var i = 0; i < latex.length; i++) {
      var c = latex[i];
      if (c === '{') depth++;
      else if (c === '}') depth--;
      else if (c === '\\') {
        var m = /^\\([A-Za-z]+)/.exec(latex.slice(i));
        if (!m) { i++; continue; }
        if (m[1] === 'left') depth++;
        else if (m[1] === 'right') depth--;
        else if (depth === 0 && RELATION_CMDS.indexOf(m[1]) >= 0) return i;
        i += m[0].length - 1;
      } else if (depth === 0 && (c === '=' || c === '<' || c === '>')) {
        return i;
      }
    }
    return -1;
  }

  var numbers = {};
  var layoutFrame = 0;
  function scheduleLayout() {
    if (layoutFrame) return;
    layoutFrame = requestAnimationFrame(function () {
      layoutFrame = 0;
      layout();
    });
  }

  // The x position of a math line's first top-level relation, in pixels
  // from the start of its math.
  function relationX(line) {
    var root = line.el.querySelector('.mq-root-block');
    if (!root) return null;
    for (var c = root.firstElementChild; c; c = c.nextElementSibling) {
      if (c.classList.contains('mq-binary-operator') &&
          RELATION_SYMBOLS.indexOf(c.textContent.trim()) >= 0) {
        return c.getBoundingClientRect().left - root.getBoundingClientRect().left;
      }
    }
    return null;
  }

  // Groups steps, numbers the groups that hold an equation, lines up each
  // group on its relations and renumbers references.
  function layout() {
    var groups = [];
    var current = null;
    lines.forEach(function (l) {
      if (l.kind !== 'math') { current = null; return; }
      if (!l.step || !current) {
        l.step = false;
        current = { lines: [l] };
        groups.push(current);
      } else {
        current.lines.push(l);
      }
    });
    lines.forEach(function (l) {
      if (l.kind === 'math') return;
      l.el.classList.remove('step', 'cont');
      l.el.querySelector('.gut').textContent = '';
    });
    numbers = {};
    var n = 0;
    groups.forEach(function (g) {
      var numbered = g.lines.some(function (l) { return relationAt(l.field.latex()) >= 0; });
      if (numbered) numbers[g.lines[0].id] = ++n;
      g.lines.forEach(function (l, k) {
        l.el.classList.toggle('step', k > 0);
        l.el.classList.toggle('cont', k < g.lines.length - 1);
        l.el.querySelector('.gut').textContent = k === 0 && numbered ? '(' + n + ')' : '';
      });
      var xs = g.lines.map(relationX);
      var known = xs.filter(function (x) { return x !== null; });
      var max = known.length > 1 ? Math.max.apply(null, known) : 0;
      g.lines.forEach(function (l, k) {
        var pad = known.length > 1 && xs[k] !== null ? max - xs[k] : 0;
        l.el.querySelector('.dmath').style.paddingLeft = pad ? pad + 'px' : '';
      });
    });
    document.querySelectorAll('#doc .dref[data-g]').forEach(function (el) {
      var num = numbers[el.dataset.g];
      el.textContent = '(' + (num || '?') + ')';
      el.classList.toggle('missing', !num);
      el.title = num ? 'Equation ' + num : 'That equation was deleted';
    });
  }

  // References to a group of steps: (3), kept up to date as lines move.
  MQ.registerEmbed('ref', function (id) {
    return {
      htmlString: '<span class="dref" data-g="' + id + '">(' + (numbers[id] || '?') + ')</span>',
      text: function () { return '(' + (numbers[id] || '?') + ')'; },
      latex: function () { return '\\embed{ref}[' + id + ']'; }
    };
  });

  // @ opens a list of the numbered equations; typing a number or arrowing
  // picks one.
  var picker = document.getElementById('ref-pick');
  var pickerInput = picker.querySelector('input');
  var pickerList = picker.querySelector('.items');
  var pickerDone = null;
  var pickerAt = 0;

  function pickerItems() {
    var q = pickerInput.value.replace(/[()\s]/g, '');
    var out = [];
    lines.forEach(function (l) {
      if (l.kind !== 'math' || !numbers[l.id]) return;
      var num = String(numbers[l.id]);
      if (!q || num.indexOf(q) === 0) out.push({ id: l.id, num: num, latex: l.field.latex() });
    });
    return out;
  }

  function renderPicker() {
    var items = pickerItems();
    pickerAt = Math.max(0, Math.min(pickerAt, items.length - 1));
    pickerList.textContent = '';
    if (!items.length) {
      var none = document.createElement('div');
      none.className = 'none';
      none.textContent = Object.keys(numbers).length ? 'No equation with that number.' : 'No numbered equations yet.';
      pickerList.appendChild(none);
    }
    items.forEach(function (it, k) {
      var row = document.createElement('div');
      row.className = 'item' + (k === pickerAt ? ' on' : '');
      var num = document.createElement('span');
      num.className = 'num';
      num.textContent = '(' + it.num + ')';
      var m = document.createElement('span');
      m.className = 'preview';
      m.textContent = it.latex.replace(/\\embed\{[a-z]+\}(\[[^\]]*\])?/g, '');
      row.appendChild(num);
      row.appendChild(m);
      row.addEventListener('mousedown', function (e) {
        e.preventDefault();
        closePicker(it.id);
      });
      pickerList.appendChild(row);
      try { MQ.StaticMath(m); } catch (err) { /* plain text is fine */ }
    });
    return items;
  }

  function openRefPicker(anchor, done) {
    pickerDone = done;
    pickerAt = 0;
    pickerInput.value = '';
    var r = anchor.getBoundingClientRect();
    picker.style.left = Math.min(r.left, window.innerWidth - 340) + 'px';
    picker.style.top = Math.min(r.bottom + 6, window.innerHeight - 260) + 'px';
    picker.classList.add('open');
    renderPicker();
    pickerInput.focus();
  }

  function closePicker(id) {
    if (!picker.classList.contains('open')) return;
    picker.classList.remove('open');
    var done = pickerDone;
    pickerDone = null;
    if (done) done(id || null);
  }

  pickerInput.addEventListener('input', function () { pickerAt = 0; renderPicker(); });
  pickerInput.addEventListener('keydown', function (e) {
    var items = pickerItems();
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      pickerAt += e.key === 'ArrowDown' ? 1 : -1;
      renderPicker();
    } else if (e.key === 'Enter' || e.key === 'Tab' || e.key === ' ') {
      e.preventDefault();
      closePicker(items[pickerAt] && items[pickerAt].id);
    } else if (e.key === 'Escape' || (e.key === 'Backspace' && !pickerInput.value)) {
      e.preventDefault();
      closePicker(null);
    }
    e.stopPropagation();
  });
  pickerInput.addEventListener('blur', function () { setTimeout(function () { closePicker(null); }, 0); });

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
    // Removing a group's first line makes the next step its first line,
    // with the same id, so references to it still work.
    var next = lines[i];
    if (line.kind === 'math' && !line.step && next && next.kind === 'math' && next.step) {
      next.step = false;
      next.id = line.id;
    }
    scheduleLayout();
  }

  function isEmpty(line) {
    if (!line) return false;
    if (line.kind === 'math') return line.field.latex() === '';
    if (line.kind === 'graph') return false;
    return lineText(line) === '';
  }

  function focusLine(i, where, line) {
    line = line || lines[i];
    if (!line) return;
    // Moving down lands at the end of a math line, as between Desmos rows,
    // and at the start of text.
    if (where === 'down') where = line.kind === 'math' ? 'end' : 'start';
    if (line.kind === 'graph') {
      line.el.focus();
    } else if (line.kind === 'math') {
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
        else if (n.classList.contains('dref')) out += '\\ref{' + n.dataset.g + '}';
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
      var ref = c === '\\' && /^\\ref\{([-\w]+)\}/.exec(text.slice(i));
      if (ref) {
        if (run) parts.push({ text: run });
        run = '';
        parts.push({ ref: ref[1] });
        i += ref[0].length;
      } else if (c === '\\' && i + 1 < text.length && (text[i + 1] === '$' || text[i + 1] === '\\')) {
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
      else if (p.ref) t.appendChild(makeChip(p.ref));
      else {
        p.text.split('\n').forEach(function (piece, k) {
          if (k > 0) t.appendChild(document.createElement('br'));
          if (piece) t.appendChild(document.createTextNode(piece));
        });
      }
    });
  }

  // A reference inside text, shown as (3).
  function makeChip(id) {
    var chip = document.createElement('span');
    chip.className = 'dref chip';
    chip.contentEditable = 'false';
    chip.dataset.g = id;
    chip.textContent = '(' + (numbers[id] || '?') + ')';
    return chip;
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
    if (e.key === '@') {
      e.preventDefault();
      var saved = r.cloneRange();
      openRefPicker(t, function (id) {
        t.focus();
        var s = window.getSelection();
        s.removeAllRanges();
        s.addRange(saved);
        if (!id) return;
        saved.deleteContents();
        var chip = makeChip(id);
        saved.insertNode(chip);
        if (!chip.nextSibling || chip.nextSibling.nodeType !== 3) {
          t.insertBefore(document.createTextNode(ZW), chip.nextSibling);
        }
        caretBeside(chip, 1);
        changed();
      });
    } else if (e.key === '$') {
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

  // ---- Live math -----------------------------------------------------------

  // Lines are typeset unless marked live. Live lines work like Desmos rows:
  // a = 3 defines a for the live lines below it, and an expression shows its
  // value on the right. A name defined again starts over from there, so
  // each problem can use x and f of its own.

  var evalTimer = 0;
  var evalSeq = 0;
  var evalSent = null;
  var EVAL_MS = 120;
  var CONTEXT_ID = 1000000;

  function scheduleEval() {
    clearTimeout(evalTimer);
    evalTimer = setTimeout(evaluate, EVAL_MS);
  }

  function liveLines() {
    return lines.filter(function (l) { return l.kind === 'math' && l.live; });
  }

  // What the engine reads: references and blanks taken out, and a step that
  // starts with = worked out from its right side.
  function evalLatex(latex) {
    latex = latex.replace(/\\embed\{ref\}\[[^\]]*\]/g, '').replace(/\\embed\{slot\}/g, '');
    if (relationAt(latex) === 0) {
      var m = /^(=|<|>|\\[A-Za-z]+)\s*/.exec(latex);
      latex = latex.slice(m ? m[0].length : 1);
    }
    return latex.trim();
  }

  // The name a line defines (a, a_1, \alpha, f), or null. x = … and
  // y = … are curves, not definitions.
  function definedName(latex) {
    var m = /^\s*((?:[A-Za-z]|\\[A-Za-z]+)(?:_(?:\{[^{}]*\}|[A-Za-z0-9]))?)\s*(?:\\left\((?:(?!\\right\)).)*\\right\))?\s*=/.exec(latex);
    if (!m) return null;
    var name = m[1].replace(/[{}]/g, '');
    if (name === 'x' || name === 'y' || name === 'z' || /^\\(sin|cos|tan|ln|log|exp|operatorname)/.test(name)) return null;
    return name;
  }

  // Chunks of live lines for the engine: each holds the latest definition
  // of every name above it, then lines until a name is defined again.
  function liveChunks(live) {
    var chunks = [];
    var latest = {};
    var cur = null;
    var contextId = CONTEXT_ID;
    live.forEach(function (l, k) {
      var latex = evalLatex(l.field.latex());
      var name = definedName(latex);
      if (!cur || (name && cur.names[name])) {
        cur = { rows: [], names: {} };
        Object.keys(latest).forEach(function (n) {
          if (n === name) return;
          cur.rows.push({ id: contextId++, latex: latest[n] });
          cur.names[n] = true;
        });
        chunks.push(cur);
      }
      cur.rows.push({ id: k, latex: latex });
      if (name) {
        cur.names[name] = true;
        latest[name] = latex;
      }
    });
    return chunks.map(function (c) { return c.rows; });
  }

  // ---- Checking steps ------------------------------------------------------

  var checkSeq = 0;
  var checkSent = {};
  var checkAsType = false;
  var checkTypeBox = document.querySelector('#doc-check-type input');
  try { checkAsType = localStorage.getItem('graphcalc.checkAsType') === '1'; } catch (e) {}
  checkTypeBox.checked = checkAsType;
  checkTypeBox.addEventListener('change', function () {
    checkAsType = checkTypeBox.checked;
    try { localStorage.setItem('graphcalc.checkAsType', checkAsType ? '1' : '0'); } catch (e) {}
    if (checkAsType) checkAll();
  });
  document.getElementById('doc-check').addEventListener('mousedown', function (e) { e.preventDefault(); });
  document.getElementById('doc-check').addEventListener('click', function () { checkAll(); });

  // A step as the checker reads it: references and blanks taken out, its
  // leading = kept.
  function checkLatex(latex) {
    return latex.replace(/\\embed\{ref\}\[[^\]]*\]/g, '').replace(/\\embed\{slot\}/g, '').trim();
  }

  // Every group of math lines that says something to check.
  function allGroups() {
    var groups = [];
    var cur = null;
    lines.forEach(function (l) {
      if (l.kind !== 'math') { cur = null; return; }
      if (!l.step || !cur) { cur = []; groups.push(cur); }
      cur.push(l);
    });
    return groups.filter(function (g) {
      return g.some(function (l) { return relationAt(l.field.latex()) >= 0; }) &&
        (g.length > 1 || /=.*=|[<>]|\\[lg]e/.test(g[0].field.latex()));
    });
  }

  function checkAll() {
    lines.forEach(function (l) { if (l.kind === 'math') clearCheck(l); });
    var groups = allGroups();
    if (!groups.length) {
      showStatus('No steps to check');
      return;
    }
    showStatus('Checking steps…');
    checkGroups(groups, true);
  }

  function checkGroups(groups, report) {
    var seq = ++checkSeq;
    var sent = groups.map(function (g) {
      return g.map(function (l) { return { line: l, latex: l.field.latex() }; });
    });
    checkSent[seq] = { groups: sent, report: report };
    var context = liveLines().map(function (l) { return evalLatex(l.field.latex()); })
      .filter(function (l) { return l; });
    send({
      type: 'docCheck',
      seq: seq,
      groups: sent.map(function (g) { return g.map(function (x) { return checkLatex(x.latex); }); }),
      context: context
    });
  }

  function checked(reply) {
    var sent = checkSent[reply.seq];
    delete checkSent[reply.seq];
    if (!sent) return;
    var bad = 0;
    var ok = 0;
    (reply.groups || []).forEach(function (results, gi) {
      var g = sent.groups[gi] || [];
      results.forEach(function (r, k) {
        var x = g[k];
        if (!x || lines.indexOf(x.line) < 0 || x.line.field.latex() !== x.latex) return;
        x.line.checkedLatex = x.latex;
        if (r.status === 'bad') bad++;
        else if (r.status === 'ok') ok++;
        markCheck(x.line, r.status === 'bad' ? r.reason : null);
      });
    });
    if (sent.report) {
      if (bad) showStatus(bad === 1 ? '1 step doesn’t follow' : bad + ' steps don’t follow', true);
      else if (ok) showStatus(ok === 1 ? '1 step checked: it follows' : ok + ' steps checked: they all follow');
      else showStatus('Nothing here could be checked');
    }
  }

  function markCheck(line, reason) {
    line.el.classList.toggle('check-bad', !!reason);
    if (reason) {
      if (!line.checkEl) {
        line.checkEl = document.createElement('span');
        line.checkEl.className = 'dcheck';
        line.resEl.insertBefore(line.checkEl, line.resEl.firstChild);
      }
      line.checkEl.textContent = reason;
      line.checkEl.title = reason;
      line.el.classList.add('has-res');
    } else if (line.checkEl) {
      line.checkEl.remove();
      line.checkEl = null;
      line.el.classList.toggle('has-res', line.resEl.childNodes.length > 0);
    }
  }

  function clearCheck(line) {
    line.checkedLatex = null;
    if (line.checkEl || line.el.classList.contains('check-bad')) markCheck(line, null);
  }

  function evaluate() {
    clearTimeout(evalTimer);
    redrawGraphs();
    var live = liveLines();
    lines.forEach(function (l) { if (l.kind === 'math' && !l.live) showResult(l, null); });
    if (!live.length) { evalSent = null; return; }
    evalSent = { seq: ++evalSeq, lines: live };
    send({ type: 'docEval', seq: evalSeq, chunks: liveChunks(live) });
  }

  function matrixLatex(cells) {
    return '\\begin{bmatrix}' + cells.map(function (r) { return r.join('&'); }).join('\\\\') +
      '\\end{bmatrix}';
  }

  function showResult(line, r) {
    var value = null;
    var warn = null;
    if (r && line.live) {
      if (r.error && r.kind !== 'empty') warn = r.error;
      else if (r.missing && r.missing.length) {
        warn = 'Define ' + r.missing.map(function (m) { return m.name; }).join(', ') +
          ' on a live line above';
      } else if (r.matrix) {
        // A matrix written out in full needs no value beside it.
        if (!(r.kind === 'variable' && /=\s*\\begin\{bmatrix\}(?:(?!\\end\{bmatrix\}).)*\\end\{bmatrix\}\s*$/.test(line.field.latex()))) {
          value = '=' + matrixLatex(r.matrix.cells);
        }
      }
      else if (r.value != null && r.slider == null) value = '=' + r.value;
    }
    line.el.classList.toggle('graphable', !!(r && line.live && r.graphed && !warn));
    var key = JSON.stringify([value, warn]);
    if (line.resKey === key) return;
    line.resKey = key;
    var el = line.resEl;
    el.innerHTML = '';
    el.title = '';
    // A step check's reason stays.
    if (line.checkEl) el.appendChild(line.checkEl);
    line.el.classList.toggle('has-res', !!(value || warn || line.checkEl));
    if (value) {
      var span = document.createElement('span');
      el.appendChild(span);
      MQ.StaticMath(span).latex(value);
    } else if (warn) {
      var w = document.createElement('span');
      w.className = 'dwarn';
      w.textContent = '!';
      el.appendChild(w);
      el.title = warn;
    }
  }

  function evaluated(reply) {
    if (!evalSent || reply.seq !== evalSent.seq) return;
    var live = evalSent.lines;
    var got = {};
    (reply.results || []).forEach(function (chunk) {
      (chunk || []).forEach(function (r) { if (r.id < CONTEXT_ID) got[r.id] = r; });
    });
    live.forEach(function (l, k) { showResult(l, got[k] || null); });
  }

  // Marks lines live or typeset.
  function setLive(list, on) {
    list.forEach(function (l) {
      if (l.kind !== 'math') return;
      l.live = on;
      l.el.classList.toggle('live', on);
      if (!on) showResult(l, null);
    });
    changed();
  }

  // The group of steps a line belongs to.
  function groupOf(line) {
    var i = indexOf(line);
    var start = i;
    while (start > 0 && lines[start].kind === 'math' && lines[start].step &&
      lines[start - 1].kind === 'math') start--;
    var end = i;
    while (end + 1 < lines.length && lines[end + 1].kind === 'math' && lines[end + 1].step) end++;
    return lines.slice(start, end + 1);
  }

  function currentLine() {
    var a = document.activeElement;
    for (var i = 0; i < lines.length; i++) {
      if (a && lines[i].el.contains(a)) return lines[i];
    }
    return lines[lastFocus] || null;
  }

  // ---- Solve ---------------------------------------------------------------

  // Ctrl+Alt+S works out a line with the same algebra as the calculator's
  // Solve, using the live lines' definitions. It runs in the background:
  // the page keeps working, and Esc stops waiting.

  var solveToken = 0;

  function solveLine(line, op) {
    if (!line || line.kind !== 'math') return;
    var latex = evalLatex(line.field.latex());
    if (!latex) return;
    if (line.solve && line.solve.timer) clearInterval(line.solve.timer);
    var others = liveLines().filter(function (l) { return l !== line; })
      .map(function (l) { return evalLatex(l.field.latex()); })
      .filter(function (t) { return t; });
    var job = { token: ++solveToken, latex: latex, op: op || null, started: Date.now() };
    line.solve = job;
    var box = solveBox(line);
    box.innerHTML = '';
    box.classList.add('waiting');
    var wait = document.createElement('div');
    wait.className = 'dsolve-wait';
    box.appendChild(wait);
    var tick = function () {
      var secs = Math.floor((Date.now() - job.started) / 1000);
      wait.textContent = 'Solving' + (secs >= 1 ? ' (' + secs + ' s)' : '') + '… Esc stops it';
    };
    tick();
    job.timer = setInterval(tick, 500);
    send({ type: 'docSolve', token: job.token, latex: latex, op: job.op, lines: others });
  }

  function solveBox(line) {
    if (!line.solveEl) {
      line.solveEl = document.createElement('div');
      line.solveEl.className = 'dsolve';
      line.el.appendChild(line.solveEl);
    }
    line.el.classList.add('has-solve');
    return line.solveEl;
  }

  function closeSolve(line) {
    if (line.solve && line.solve.timer) clearInterval(line.solve.timer);
    line.solve = null;
    if (line.solveEl) line.solveEl.remove();
    line.solveEl = null;
    line.el.classList.remove('has-solve');
  }

  // Stops waiting for every solve in progress, and stops Giac if it is
  // working on one. The engine's answer, if it comes, is dropped.
  function cancelSolves() {
    var any = false;
    lines.forEach(function (l) {
      if (l.solve && l.solve.timer) {
        any = true;
        closeSolve(l);
      }
    });
    if (any) send({ type: 'docCancel' });
    return any;
  }

  function richText(el, text) {
    String(text).split('$').forEach(function (part, i) {
      var span = document.createElement('span');
      if (i % 2) {
        span.className = 'inline-math';
        el.appendChild(span);
        MQ.StaticMath(span).latex(part);
      } else {
        span.textContent = part;
        el.appendChild(span);
      }
    });
  }

  function staticBlock(className, latex) {
    var div = document.createElement('div');
    div.className = className;
    var span = document.createElement('span');
    div.appendChild(span);
    MQ.StaticMath(span).latex(latex);
    return div;
  }

  function solved(msg) {
    var line = lines.filter(function (l) { return l.solve && l.solve.token === msg.token; })[0];
    if (!line) return;
    clearInterval(line.solve.timer);
    line.solve.timer = null;
    var r = msg.reply || {};
    line.solve.reply = r;
    var box = solveBox(line);
    box.classList.remove('waiting');
    box.innerHTML = '';
    var head = document.createElement('div');
    head.className = 'dsolve-head';
    (r.choices || []).forEach(function (c) {
      var b = document.createElement('button');
      b.className = 'chip' + (c.op === r.op ? ' on' : '');
      richText(b, c.label);
      b.addEventListener('mousedown', function (e) { e.preventDefault(); });
      b.addEventListener('click', function () { solveLine(line, c.op); });
      head.appendChild(b);
    });
    var spacer = document.createElement('span');
    spacer.className = 'grow';
    head.appendChild(spacer);
    if ((r.steps || []).length || r.result) {
      var ins = document.createElement('button');
      ins.className = 'dsolve-insert';
      ins.textContent = 'Insert steps';
      ins.title = 'Write the working into the document as steps below this line';
      ins.addEventListener('mousedown', function (e) { e.preventDefault(); });
      ins.addEventListener('click', function () { insertSteps(line, r); });
      head.appendChild(ins);
    }
    var close = document.createElement('button');
    close.className = 'dsolve-close';
    close.title = 'Close the working';
    close.innerHTML = '&#x2715;';
    close.addEventListener('mousedown', function (e) { e.preventDefault(); });
    close.addEventListener('click', function () { closeSolve(line); });
    head.appendChild(close);
    box.appendChild(head);
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
      if (st.latex) li.appendChild(staticBlock('math', st.latex));
      list.appendChild(li);
    });
    if (list.firstChild) box.appendChild(list);
    if (r.result) box.appendChild(staticBlock('dsolve-result', r.result));
    if (r.error) {
      var err = document.createElement('div');
      err.className = 'dsolve-error';
      richText(err, r.error);
      box.appendChild(err);
    }
  }

  // The working as lines of the document: steps lined up under the line.
  // A step that continues the same left side is written from its = on.
  function stepLines(line, r) {
    var out = [];
    var prev = evalLatex(line.field.latex());
    var prevLeft = null;
    var at0 = relationAt(prev);
    if (at0 > 0) prevLeft = prev.slice(0, at0);
    var steps = (r.steps || []).map(function (s) { return s.latex; });
    // The answer, unless the last step already ends with it.
    var last = steps[steps.length - 1] || '';
    if (r.result && last.slice(relationAt(last) + 1) !== r.result && last !== r.result) {
      steps.push(r.result);
    }
    steps.forEach(function (latex) {
      if (!latex || latex === prev) return;
      var at = relationAt(latex);
      var left = at > 0 ? latex.slice(0, at) : null;
      out.push(left !== null && left === prevLeft ? latex.slice(at) : latex);
      prev = latex;
      if (left !== null) prevLeft = left;
    });
    return out;
  }

  function insertSteps(line, r) {
    var i = indexOf(line);
    stepLines(line, r).forEach(function (latex, k) {
      newMath(latex, i + 1 + k, { step: true });
    });
    closeSolve(line);
    changed();
    focusLine(i, 'end');
  }

  // ---- The whole document --------------------------------------------------

  // ---- Graph blocks --------------------------------------------------------

  // A graph block draws its rows with the calculator's engine, off screen,
  // as an image. Rows that came from a line of the document follow that
  // line (src is its id), and the live definitions above the block are
  // there too, so a = 3 on a live line above sets a.
  var GRAPH_HEIGHT = 320;
  var graphSeq = 0;
  var editingGraph = null;

  function defaultView() { return { x: 0, y: 0, w: 20 }; }

  function lineById(id) {
    return lines.filter(function (l) { return l.kind === 'math' && l.id === id; })[0] || null;
  }

  // The block's rows with linked rows brought up to date, and the live
  // definitions above it that its rows don't make themselves.
  function graphRows(line, forEditing) {
    var own = line.rows.map(function (r) {
      var copy = Object.assign({}, r);
      var from = r.src ? lineById(r.src) : null;
      if (from) copy.latex = evalLatex(from.field.latex());
      return copy;
    });
    var named = {};
    own.forEach(function (r) {
      var n = definedName(r.latex || '');
      if (n) named[n] = true;
    });
    var latest = {};
    var order = [];
    lines.slice(0, indexOf(line)).forEach(function (l) {
      if (l.kind !== 'math' || !l.live) return;
      var latex = evalLatex(l.field.latex());
      var n = definedName(latex);
      if (!n || named[n]) return;
      if (!(n in latest)) order.push(n);
      latest[n] = { latex: latex, src: l.id };
    });
    var context = order.map(function (n, k) {
      var r = { latex: latest[n].latex, src: latest[n].src };
      if (!forEditing) { r.id = CONTEXT_ID + k; r.hidden = true; }
      return r;
    });
    return context.concat(own);
  }

  function makeGraph(block, at) {
    var el = lineShell('graph');
    el.tabIndex = 0;
    var line = {
      kind: 'graph', el: el, id: block.id || newId(),
      rows: (block.rows || []).filter(function (r) { return r && typeof r === 'object'; }),
      view: block.view && block.view.w > 0 ? block.view : defaultView(),
      height: block.height > 0 ? block.height : GRAPH_HEIGHT,
      shown: null, inflight: false, again: false
    };
    var box = document.createElement('div');
    box.className = 'dgraph';
    box.style.height = line.height + 'px';
    var img = document.createElement('img');
    img.alt = 'Graph';
    img.draggable = false;
    box.appendChild(img);
    var bar = document.createElement('div');
    bar.className = 'dgraph-bar';
    [['open', 'Open in graph view', function () { openGraph(line); }],
      ['home', 'Reset the view', function () { line.view = defaultView(); changed(); drawGraph(line); }],
      ['remove', 'Remove the graph', function () {
        var i = indexOf(line);
        removeLine(line);
        focusLine(Math.min(i, lines.length - 1), 'end');
        changed();
      }]].forEach(function (b) {
      var btn = document.createElement('button');
      btn.className = 'dgraph-' + b[0];
      btn.textContent = b[1];
      btn.addEventListener('mousedown', function (e) { e.preventDefault(); });
      btn.addEventListener('click', b[2]);
      bar.appendChild(btn);
    });
    box.appendChild(bar);
    var note = document.createElement('div');
    note.className = 'dgraph-note';
    box.appendChild(note);
    el.appendChild(box);
    line.box = box;
    line.img = img;
    line.note = note;

    // Drag to pan, scroll to zoom about the pointer, as on the graph.
    var drag = null;
    box.addEventListener('mousedown', function (e) {
      if (e.button !== 0 || e.target.closest('button')) return;
      e.preventDefault();
      el.focus();
      drag = { x: e.clientX, y: e.clientY, view: Object.assign({}, line.view) };
    });
    window.addEventListener('mousemove', function (e) {
      if (!drag) return;
      var per = drag.view.w / box.clientWidth;
      line.view = { x: drag.view.x - (e.clientX - drag.x) * per, y: drag.view.y + (e.clientY - drag.y) * per, w: drag.view.w };
      placeImage(line);
      drawGraph(line);
    });
    window.addEventListener('mouseup', function () {
      if (!drag) return;
      drag = null;
      changed();
    });
    box.addEventListener('wheel', function (e) {
      e.preventDefault();
      var r = box.getBoundingClientRect();
      var per = line.view.w / r.width;
      var px = e.clientX - r.left - r.width / 2;
      var py = e.clientY - r.top - r.height / 2;
      var wx = line.view.x + px * per;
      var wy = line.view.y - py * per;
      var f = Math.pow(1.0015, e.deltaY);
      var w = Math.min(1e12, Math.max(1e-9, line.view.w * f));
      per = w / r.width;
      line.view = { x: wx - px * per, y: wy + py * per, w: w };
      placeImage(line);
      drawGraph(line);
      clearTimeout(line.wheelTimer);
      line.wheelTimer = setTimeout(changed, 300);
    }, { passive: false });
    box.addEventListener('dblclick', function (e) {
      if (!e.target.closest('button')) openGraph(line);
    });

    el.addEventListener('keydown', function (e) {
      if (e.target !== el) return;
      var i = indexOf(line);
      var handled = true;
      if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') focusLine(i - 1, 'end');
      else if (e.key === 'ArrowDown' || e.key === 'ArrowRight') {
        if (i + 1 < lines.length) focusLine(i + 1, 'start');
        else focusLine(i + 1, 'start', newMath('', i + 1));
      } else if (e.key === 'Enter' && !e.ctrlKey && !e.altKey) {
        if (e.shiftKey) openGraph(line);
        else { focusLine(i + 1, 'start', newMath('', i + 1)); changed(); }
      } else if (e.key === 'Backspace' || e.key === 'Delete') {
        removeLine(line);
        focusLine(e.key === 'Backspace' ? Math.max(0, i - 1) : Math.min(i, lines.length - 1), 'end');
        changed();
      } else handled = false;
      if (handled) { e.preventDefault(); e.stopPropagation(); }
    });
    insertLine(line, at);
    setTimeout(function () { drawGraph(line); }, 0);
    return line;
  }

  // While a new image is drawn, the last one moves with the view.
  function placeImage(line) {
    var s = line.shown;
    if (!s) return;
    var w = line.box.clientWidth;
    var k = s.w / line.view.w;
    var dx = (s.x - line.view.x) / line.view.w * w;
    var dy = -(s.y - line.view.y) / line.view.w * w;
    line.img.style.transform = 'translate(' + dx + 'px,' + dy + 'px) scale(' + k + ')';
  }

  // Asks the engine for the block's picture; one request at a time.
  function drawGraph(line) {
    if (indexOf(line) < 0) return;
    if (line.inflight) { line.again = true; return; }
    var width = line.box.clientWidth;
    if (!width) { setTimeout(function () { drawGraph(line); }, 100); return; }
    var ask = {
      type: 'docGraph', id: line.id, rows: graphRows(line, false), view: Object.assign({}, line.view),
      width: Math.round(width * scale()), height: Math.round(line.height * scale()), scale: scale()
    };
    // Nothing changed since the picture showing was drawn.
    var key = JSON.stringify(ask);
    if (key === line.drawnKey) return;
    line.inflight = true;
    line.again = false;
    line.seq = ++graphSeq;
    line.asked = ask.view;
    line.askedKey = key;
    ask.seq = line.seq;
    send(ask);
  }

  // Sharp on screen and in print.
  function scale() { return Math.max(2, window.devicePixelRatio || 1); }

  function graphRendered(reply) {
    var line = lines.filter(function (l) { return l.kind === 'graph' && l.id === reply.id; })[0];
    if (!line || reply.seq !== line.seq) return;
    line.inflight = false;
    if (reply.png) {
      line.img.src = reply.png;
      line.shown = line.asked;
      line.drawnKey = line.askedKey;
      placeImage(line);
      line.note.textContent = '';
    } else if (reply.error) {
      line.note.textContent = reply.error;
    }
    var empty = !line.rows.length;
    line.el.classList.toggle('empty', empty);
    if (empty) line.note.textContent = 'Graph a live line with its Graph button, or open this in the graph view to add curves.';
    if (line.again) drawGraph(line);
  }

  function redrawGraphs() {
    lines.forEach(function (l) { if (l.kind === 'graph') drawGraph(l); });
  }

  // The Graph button: the line goes into the graph block under it (or under
  // its group of steps), which is made if there is none.
  function graphLine(line) {
    if (!line || line.kind !== 'math') return;
    if (!line.live) setLive([line], true);
    var i = indexOf(line);
    var end = i;
    while (end + 1 < lines.length && lines[end + 1].kind === 'math' && lines[end + 1].step) end++;
    var block = lines[end + 1] && lines[end + 1].kind === 'graph' ? lines[end + 1] : null;
    if (!block) block = makeGraph({}, end + 1);
    var have = block.rows.some(function (r) { return r.src === line.id; });
    if (!have) block.rows.push({ latex: evalLatex(line.field.latex()), src: line.id });
    changed();
    drawGraph(block);
    block.el.scrollIntoView({ block: 'nearest' });
  }

  // Opens the block in the graph view; its rows come back as they change
  // and its view when Done.
  function openGraph(line) {
    editingGraph = { line: line, sent: graphRows(line, true) };
    send({ type: 'graphEdit', rows: editingGraph.sent, view: line.view, title: titleEl.value || 'Untitled' });
    show(false);
  }

  function graphEdited(msg) {
    var e = editingGraph;
    if (!e || indexOf(e.line) < 0) { editingGraph = null; return; }
    // A row keeps its link to a line while it reads the same.
    var linked = {};
    e.sent.forEach(function (r) { if (r.src) linked[r.latex] = r.src; });
    e.line.rows = (msg.rows || []).filter(function (r) {
      return r && (r.latex || r.note || r.table || r.matrix);
    }).map(function (r) {
      var copy = Object.assign({}, r);
      delete copy.id;
      if (!copy.src && linked[copy.latex]) copy.src = linked[copy.latex];
      return copy;
    });
    if (msg.view && msg.view.w > 0) e.line.view = msg.view;
    if (msg.done) editingGraph = null;
    changed();
    drawGraph(e.line);
  }

  function serialize() {
    return {
      format: 'graphcalc-document',
      version: 1,
      title: titleEl.value,
      lines: lines.map(function (l) {
        if (l.kind === 'math') {
          var m = { type: 'math', latex: l.field.latex() };
          if (l.step) m.step = true; else m.id = l.id;
          if (l.live) m.live = true;
          return m;
        }
        if (l.kind === 'graph') {
          return { type: 'graph', id: l.id, rows: l.rows, view: l.view, height: l.height };
        }
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
      else if (l.type === 'graph') makeGraph(l, i);
      else newMath(l.latex || '', i, { step: !!l.step, id: l.id, live: !!l.live });
    });
    if (!lines.length) newMath('', 0);
    loading--;
    scheduleLayout();
    scheduleEval();
  }

  function isBlank(doc) {
    return !doc.title && doc.lines.every(function (l) { return !(l.latex || l.text); });
  }

  function changed() {
    if (loading) return;
    scheduleLayout();
    scheduleEval();
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

  // Ctrl+Alt+P: the next "Problem N" heading, with a math line under it.
  function insertProblem() {
    var n = 0;
    lines.forEach(function (l) {
      var m = l.kind === 'heading' && /^problem\s+(\d+)/i.exec(lineText(l));
      if (m) n = Math.max(n, +m[1]);
    });
    var at = Math.min(lastFocus + 1, lines.length);
    var current = lines[lastFocus];
    if (current && isEmpty(current)) {
      at = lastFocus;
      removeLine(current);
    }
    newText('Problem ' + (n + 1), 'heading', 1, at);
    var math = newMath('', at + 1);
    focusLine(at + 1, 'start', math);
    changed();
  }

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
    else if (scBox.classList.contains('open') && e.key === 'Escape') closeShortcuts();
    else if (pdfBox.classList.contains('open') && e.key === 'Escape') closePdf();
    else if (e.key === 'Escape' && !ctrl && cancelSolves()) handled = true;
    else if (e.key === 'F9' && !ctrl && !e.altKey) checkAll();
    else if (ctrl && !e.altKey && k === 'l' && !scBox.classList.contains('open')) {
      var cur = currentLine();
      if (cur && cur.kind === 'math') {
        var list = e.shiftKey ? groupOf(cur) : [cur];
        setLive(list, !cur.live);
      }
    } else if (ctrl && e.altKey && !e.shiftKey && k === 's') {
      solveLine(currentLine(), null);
    } else if (ctrl && e.shiftKey && !e.altKey && k === 'g') {
      graphLine(currentLine());
    }
    else if (ctrl && !e.altKey && !e.shiftKey && k === 'p') openPdf();
    else if (scBox.classList.contains('open') && !(ctrl && e.altKey)) handled = false;
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
    } else if (ctrl && e.altKey && !e.shiftKey && k === 'p') {
      insertProblem();
    } else {
      handled = false;
    }
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  }, true);

  // ---- Shortcuts menu ------------------------------------------------------

  var scBox = document.getElementById('doc-shortcuts');
  var scList = scBox.querySelector('.sc-list');
  var scTrigger = scBox.querySelector('.sc-trigger');
  var scNote = scBox.querySelector('.sc-note');
  var scWhere = scBox.querySelector('.sc-where');
  var scWarn = scBox.querySelector('.sc-warn');
  var scSave = scBox.querySelector('.sc-save');
  var scTitle = scBox.querySelector('.sc-form-title');
  var scTemplate = MQ.MathField(scBox.querySelector('.sc-template'),
    Object.assign({}, DOC_OPTIONS, { handlers: { edit: function () { checkForm(); } } }));
  var editing = null;   // the shortcut the form changes, or null to add one
  var scReturn = null;  // where the keyboard goes back to

  function sc() { return window.shortcuts; }

  function renderShortcuts() {
    scList.textContent = '';
    var table = document.createElement('table');
    table.innerHTML = '<tr><th>Trigger</th><th>Gives</th><th>Name</th><th>Works in</th><th>On</th><th></th></tr>';
    sc().list().forEach(function (s) {
      var tr = document.createElement('tr');
      if (!s.on) tr.className = 'off';
      var trig = document.createElement('td');
      var code = document.createElement('code');
      code.textContent = s.trigger;
      trig.appendChild(code);
      var gives = document.createElement('td');
      var m = document.createElement('span');
      m.textContent = s.latex;
      gives.appendChild(m);
      var note = document.createElement('td');
      note.textContent = s.note || '';
      var where = document.createElement('td');
      where.textContent = s.where === 'both' ? 'Documents and calculator' : 'Documents';
      var on = document.createElement('td');
      var box = document.createElement('input');
      box.type = 'checkbox';
      box.checked = s.on;
      box.title = s.on ? 'Turn off' : 'Turn on';
      box.addEventListener('change', function () { s.on = box.checked; sc().replace(sc().list()); });
      on.appendChild(box);
      var acts = document.createElement('td');
      acts.className = 'acts';
      var edit = document.createElement('button');
      edit.textContent = 'Edit';
      edit.addEventListener('click', function () { fillForm(s); });
      acts.appendChild(edit);
      var base = s.builtin && sc().builtIn().filter(function (b) { return b.base === s.base; })[0];
      if (base) {
        var changedFromBase = base.trigger !== s.trigger || base.latex !== s.latex || base.where !== s.where;
        if (changedFromBase) {
          var reset = document.createElement('button');
          reset.textContent = 'Reset';
          reset.title = 'Back to ' + base.trigger;
          reset.addEventListener('click', function () {
            var list = sc().list();
            list[list.indexOf(s)] = base;
            sc().replace(list);
          });
          acts.appendChild(reset);
        }
      } else {
        var del = document.createElement('button');
        del.textContent = 'Delete';
        del.addEventListener('click', function () {
          var list = sc().list().filter(function (x) { return x !== s; });
          if (editing === s) fillForm(null);
          sc().replace(list);
        });
        acts.appendChild(del);
      }
      [trig, gives, note, where, on, acts].forEach(function (td) { tr.appendChild(td); });
      table.appendChild(tr);
      try { MQ.StaticMath(m); } catch (err) { /* shown as LaTeX */ }
    });
    scList.appendChild(table);
  }

  function fillForm(s, latex) {
    editing = s || null;
    scTitle.textContent = s ? 'Change ' + s.trigger : 'Add a shortcut';
    scSave.textContent = s ? 'Save changes' : 'Add shortcut';
    scTrigger.value = s ? s.trigger : '';
    scNote.value = s ? s.note || '' : '';
    scWhere.value = s ? s.where : 'doc';
    scTemplate.latex(s ? s.latex : latex || '');
    checkForm();
    (s || latex ? scTrigger : scTrigger).focus();
  }

  // Problems that stop the shortcut being saved, then ones only worth knowing.
  function formProblems() {
    var trigger = scTrigger.value.trim();
    var all = sc().warnings(trigger, editing);
    var blocking = all.filter(function (w) {
      return /^Type a trigger|cannot contain spaces|already uses/.test(w);
    });
    if (!scTemplate.latex()) blocking.push('Type the math it should give.');
    return { blocking: blocking, all: all.concat(scTemplate.latex() ? [] : ['Type the math it should give.']) };
  }

  function checkForm() {
    var p = formProblems();
    scWarn.textContent = '';
    if (scTrigger.value || scTemplate.latex()) {
      p.all.forEach(function (w) {
        var li = document.createElement('li');
        li.textContent = w;
        if (p.blocking.indexOf(w) >= 0) li.className = 'stop';
        scWarn.appendChild(li);
      });
    }
    scSave.disabled = p.blocking.length > 0;
  }
  scTrigger.addEventListener('input', checkForm);

  scBox.querySelector('.sc-slot').addEventListener('click', function () {
    scTemplate.focus();
    scTemplate.write(sc().SLOT);
    checkForm();
  });
  scBox.querySelector('.sc-cancel').addEventListener('click', function () { fillForm(null); });
  scSave.addEventListener('click', function () {
    if (formProblems().blocking.length) return;
    var list = sc().list();
    var entry = editing || { on: true, builtin: false, base: '' };
    entry.trigger = scTrigger.value.trim();
    entry.latex = scTemplate.latex();
    entry.note = scNote.value.trim();
    entry.where = scWhere.value;
    if (!editing) list.push(entry);
    sc().replace(list);
    fillForm(null);
  });

  function openShortcuts(latex) {
    scReturn = document.activeElement;
    scBox.classList.add('open');
    renderShortcuts();
    fillForm(null, latex);
  }
  function closeShortcuts() {
    scBox.classList.remove('open');
    if (scReturn && document.body.contains(scReturn)) scReturn.focus();
  }
  if (window.shortcuts) window.shortcuts.onChange(function () { if (scBox.classList.contains('open')) renderShortcuts(); });
  document.getElementById('doc-shortcuts-btn').addEventListener('click', function () { openShortcuts(''); });
  scBox.querySelector('.close').addEventListener('click', closeShortcuts);
  scBox.addEventListener('mousedown', function (e) { if (e.target === scBox) closeShortcuts(); });

  // Right-clicking math offers to make a shortcut of the selection, or of
  // the whole line when nothing is selected.
  var contextMenu = document.getElementById('doc-context');
  var contextLatex = '';
  linesEl.addEventListener('contextmenu', function (e) {
    var math = e.target.closest && e.target.closest('.dmath, .dinl');
    if (!math) return;
    var root = math.querySelector('.mq-editable-field');
    var field = root && MQ(root);
    if (!field) return;
    e.preventDefault();
    var area = root.querySelector('textarea');
    var selected = area && area.value;
    contextLatex = selected || field.latex();
    if (!contextLatex) return;
    contextMenu.querySelector('button').textContent = selected ?
      'Make a shortcut from the selection…' : 'Make a shortcut from this line…';
    contextMenu.style.left = Math.min(e.clientX, window.innerWidth - 260) + 'px';
    contextMenu.style.top = Math.min(e.clientY, window.innerHeight - 40) + 'px';
    contextMenu.classList.add('open');
  });
  contextMenu.addEventListener('mousedown', function (e) {
    e.preventDefault();
    contextMenu.classList.remove('open');
    openShortcuts(contextLatex);
  });
  document.addEventListener('mousedown', function (e) {
    if (!contextMenu.contains(e.target)) contextMenu.classList.remove('open');
  });

  // ---- PDF export ----------------------------------------------------------

  // The engine has WebView2 print this page with its print styles (doc.css,
  // @media print): only the title and the lines, without the app around them.
  var pdfBox = document.getElementById('doc-pdf');
  var pdfName = document.getElementById('pdf-name');
  var pdfPage = document.getElementById('pdf-page');
  var pdfHeader = document.getElementById('pdf-header');
  var pdfBreaks = document.getElementById('pdf-breaks');
  var pdfOpen = document.getElementById('pdf-open');
  var pdfPrefs = {};
  var pdfBusy = false;

  function showPdfPrefs(prefs) {
    pdfPrefs = prefs && typeof prefs === 'object' ? prefs : {};
    pdfName.value = pdfPrefs.name || '';
    pdfPage.value = pdfPrefs.page === 'a4' ? 'a4' : 'letter';
    pdfHeader.checked = pdfPrefs.header !== false;
    pdfBreaks.checked = !!pdfPrefs.breaks;
    pdfOpen.checked = pdfPrefs.open !== false;
  }

  function openPdf() {
    if (pdfBusy) return;
    showPdfPrefs(pdfPrefs);
    pdfBox.classList.add('open');
    (pdfName.value ? pdfBox.querySelector('.go') : pdfName).focus();
  }
  function closePdf() { pdfBox.classList.remove('open'); }

  // Marks the lines for print: empty ones are left out, and Problem
  // headings after the first can start a page.
  function preparePrint(breaks) {
    if (document.activeElement && document.activeElement.blur) document.activeElement.blur();
    lines.forEach(function (l) { l.el.classList.remove('focused'); });
    var seenProblem = false;
    lines.forEach(function (l) {
      l.el.classList.toggle('print-empty', isEmpty(l));
      var problem = l.kind === 'heading' && /^problem\b/i.test(lineText(l));
      l.el.classList.toggle('page-break', breaks && problem && seenProblem);
      if (problem) seenProblem = true;
    });
    document.body.classList.toggle('print-untitled', !titleEl.value.trim());
  }

  function exportPdf() {
    var options = {
      name: pdfName.value.trim(),
      page: pdfPage.value,
      header: pdfHeader.checked,
      breaks: pdfBreaks.checked,
      open: pdfOpen.checked
    };
    pdfPrefs = options;
    closePdf();
    flush();
    layout();
    preparePrint(options.breaks);
    pdfBusy = true;
    showStatus('Saving PDF…');
    send({ type: 'exportPdf', title: titleEl.value || 'Untitled', options: options });
  }

  pdfBox.querySelector('.go').addEventListener('click', exportPdf);
  pdfBox.querySelector('.cancel').addEventListener('click', closePdf);
  pdfBox.querySelector('.close').addEventListener('click', closePdf);
  pdfBox.addEventListener('mousedown', function (e) { if (e.target === pdfBox) closePdf(); });
  pdfBox.addEventListener('keydown', function (e) {
    if (e.key === 'Enter') { e.preventDefault(); exportPdf(); }
  });
  document.getElementById('doc-pdf-btn').addEventListener('click', openPdf);

  function pdfDone(result) {
    pdfBusy = false;
    lines.forEach(function (l) { l.el.classList.remove('print-empty', 'page-break'); });
    if (result.error) showStatus(result.error, true);
    else if (result.path) showStatus('Saved ' + String(result.path).split(/[\\/]/).pop());
    else showStatus('');
    focusLine(Math.min(lastFocus, lines.length - 1), 'end');
  }

  // ---- Called by Rust through evaluate_script ------------------------------

  window.doc = {
    show: show,
    graphRendered: graphRendered,
    graphEdited: graphEdited,
    init: function (info) {
      library.folder = info.library || '';
      root.querySelector('.side-foot .folder').textContent = library.folder;
      root.querySelector('.side-foot .folder').title = library.folder;
      if (info.opened && info.opened.doc && !dirty) {
        path = info.opened.path;
        build(info.opened.doc);
        forgetHistory();
      }
      if (window.shortcuts) window.shortcuts.set(info.shortcuts);
      showPdfPrefs(info.pdf);
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
    evaluated: evaluated,
    solved: solved,
    checked: checked,
    // For tests: the document as it would be saved.
    serialize: serialize,
    stepLines: stepLines,
    pdfDone: pdfDone,
    // For tests: lay the page out as the PDF export does.
    preparePrint: preparePrint,
    openShortcuts: openShortcuts
  };

  build({ title: '', lines: [] });
  forgetHistory();
  renderLibrary();
  send({ type: 'docInit' });
})();
