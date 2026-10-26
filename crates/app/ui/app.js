// Expression list: one MathQuill field per row, set up the way Desmos
// configures it. Every change sends all rows to the Rust engine, which
// replies with what each row means (curve, value, slider, error).
(function () {
  'use strict';

  var MQ = MathQuill.getInterface(2);
  var COLORS = ['#c74440', '#2d70b3', '#388c46', '#6042a6', '#fa7e19', '#000000'];
  var DEFAULT_WIDTH = 2.5;
  var DEFAULT_POINT_SIZE = 9;
  // Seconds for a playing slider to cross its whole range once.
  var PLAY_SECONDS = 8;
  var LONG_PRESS_MS = 450;
  var nextColor = 0;
  var nextId = 1;
  var rows = [];
  var list = document.getElementById('list');

  // Options matching how Desmos's editor behaves when typing.
  var MATH_OPTIONS = {
    spaceBehavesLikeTab: false,
    leftRightIntoCmdGoes: 'up',
    restrictMismatchedBrackets: true,
    sumStartsWithNEquals: true,
    supSubsRequireOperand: true,
    charsThatBreakOutOfSupSub: '+-=<>',
    autoSubscriptNumerals: true,
    autoCommands: 'pi theta tau phi alpha beta gamma delta sqrt sum prod int nthroot',
    autoOperatorNames: 'sin cos tan sec csc cot arcsin arccos arctan arcsec arccsc arccot ' +
      'sinh cosh tanh sech csch coth arcsinh arccosh arctanh ' +
      'ln log exp abs min max floor ceil round sign mod mean median total length ' +
      'gcd lcm nCr nPr for'
  };

  // Desmos subscripts hold only letters and digits, so typing anything else
  // at the end of one, like = or a comma, steps out of it first: t_rack=2
  // types as t_{rack}=2. MathQuill only does this for superscripts.
  document.addEventListener('keydown', function (e) {
    if (e.key.length !== 1 || /[A-Za-z0-9_]/.test(e.key) || e.ctrlKey || e.metaKey || e.altKey) return;
    var root = e.target.closest && e.target.closest('.mq-editable-field');
    var cursor = root && root.querySelector('.mq-cursor');
    if (!cursor || cursor.nextElementSibling) return;
    var block = cursor.parentElement;
    if (!block.classList.contains('mq-sub')) return;
    var field = MQ(root);
    if (field) field.keystroke('Right');
  }, true);

  function send(message) {
    if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(JSON.stringify(message));
    }
  }

  // The style settings rows and table columns share.
  function styleOf(t) {
    return {
      color: t.color, hidden: t.hidden, style: t.style, width: t.width,
      opacity: t.opacity, pointStyle: t.pointStyle, pointSize: t.pointSize,
      dots: t.dots, connect: t.connect
    };
  }

  // Set while a saved graph is being loaded, so rows are sent once at the end.
  var loading = false;

  function sendRows() {
    if (loading) return;
    send({
      type: 'rows',
      rows: rows.map(function (r) {
        if (r.kind === 'table') {
          return {
            id: r.id, latex: '',
            table: r.columns.map(function (c) {
              return Object.assign(styleOf(c), {
                header: c.field.latex(),
                cells: c.cells.map(function (f) { return f.latex(); })
              });
            })
          };
        }
        return Object.assign(styleOf(r), {
          id: r.id, latex: r.field.latex(), domain: r.domain, slider: r.slider
        });
      })
    });
  }

  function indexOf(row) { return rows.indexOf(row); }

  function renumber() {
    rows.forEach(function (r, i) { r.indexEl.textContent = String(i + 1); });
  }

  // The active row is the selected one: the graph shows its points of
  // interest.
  var selectedId = null;
  function setActive(row) {
    rows.forEach(function (r) { r.el.classList.toggle('active', r === row); });
    var id = row ? row.id : null;
    if (id !== selectedId) {
      selectedId = id;
      send({ type: 'select', id: id });
    }
  }

  // `fromBelow` says focus is arriving from the row underneath, so a table
  // takes it in its last row instead of its header.
  function focusRow(i, atEnd, fromBelow) {
    var row = rows[Math.max(0, Math.min(rows.length - 1, i))];
    if (!row) return;
    if (row.kind === 'table') {
      tableFocus(row, 0, fromBelow ? row.nrows - 1 : -1);
      return;
    }
    row.field.focus();
    if (atEnd === false) row.field.moveToLeftEnd(); else row.field.moveToRightEnd();
  }

  function removeRow(row) {
    var i = indexOf(row);
    if (i < 0) return;
    row.el.parentNode.removeChild(row.el);
    rows.splice(i, 1);
    if (rows.length === 0) addRow('', 0);
    renumber();
    focusRow(Math.max(0, i - 1));
    sendRows();
  }

  // ---- Numbers -------------------------------------------------------------

  // Shortest decimal for v with at most `places` places: 2.50 -> 2.5, -0 -> 0.
  function formatNumber(v, places) {
    var s = v.toFixed(Math.max(0, Math.min(10, places)));
    if (s.indexOf('.') >= 0) s = s.replace(/0+$/, '').replace(/\.$/, '');
    return s === '-0' ? '0' : s;
  }

  function placesFor(step) {
    return Math.max(0, Math.ceil(-Math.log10(step) - 1e-9));
  }

  // Snaps a slider value to its step, or to a resolution fine enough for
  // dragging when it has none.
  function sliderValue(row, v) {
    var s = row.slider;
    if (s.step > 0) {
      v = s.min + Math.round((v - s.min) / s.step) * s.step;
      // Extra places absorb float noise (0.1 * 3); trailing zeros are trimmed.
      return formatNumber(v, placesFor(s.step) + 3);
    }
    return formatNumber(v, placesFor((s.max - s.min) / 1000));
  }

  function parseLimit(text) {
    var t = String(text).trim().replace(/−/g, '-');
    if (t === '') return null;
    var v = Number(t);
    return isFinite(v) ? v : null;
  }

  // ---- Sliders -------------------------------------------------------------

  // `exact` keeps v as given (already rounded by the engine for a dragged
  // point) unless the slider has a step to snap to.
  function setSlider(row, v, exact) {
    var latex = row.field.latex();
    var eq = latex.indexOf('=');
    if (eq < 0) return;
    var text = exact && !(row.slider.step > 0) ? formatNumber(v, 10) : sliderValue(row, v);
    // Keep the left side exactly as typed, and replace only the number.
    row.settingSlider = true;
    row.field.latex(latex.slice(0, eq) + '=' + text);
    row.settingSlider = false;
  }

  function refreshSliderBox(row) {
    var s = row.slider;
    row.minEl.textContent = formatNumber(s.min, 10).replace('-', '−');
    row.maxEl.textContent = formatNumber(s.max, 10).replace('-', '−');
    row.range.min = String(s.min);
    row.range.max = String(s.max);
    row.range.step = s.step > 0 ? String(s.step) : 'any';
    if (row.result && row.result.slider != null) row.range.value = String(row.result.slider);
  }

  function openLimits(row) {
    var s = row.slider;
    row.limMin.value = formatNumber(s.min, 10);
    row.limMax.value = formatNumber(s.max, 10);
    row.limStep.value = s.step > 0 ? formatNumber(s.step, 10) : '';
    row.el.classList.add('limits-open');
  }

  function applyLimits(row) {
    var min = parseLimit(row.limMin.value);
    var max = parseLimit(row.limMax.value);
    var step = parseLimit(row.limStep.value);
    if (min != null && max != null && max > min) {
      row.slider.min = min;
      row.slider.max = max;
    }
    row.slider.step = step != null && step > 0 ? step : null;
    refreshSliderBox(row);
  }

  var lastTick = null;
  function tick(now) {
    var playing = rows.filter(function (r) { return r.playing; });
    if (playing.length === 0) { lastTick = null; return; }
    var dt = lastTick == null ? 0 : Math.min(0.1, (now - lastTick) / 1000);
    lastTick = now;
    playing.forEach(function (r) {
      // The engine has not said what this row is yet.
      if (!r.result || r.result.slider == null) return;
      var s = r.slider;
      var span = s.max - s.min;
      if (r.playPos == null) r.playPos = r.result.slider;
      r.playPos += r.playDir * span * dt / PLAY_SECONDS;
      // Back and forth between the limits, Desmos's default.
      if (r.playPos >= s.max) { r.playPos = s.max; r.playDir = -1; }
      if (r.playPos <= s.min) { r.playPos = s.min; r.playDir = 1; }
      setSlider(r, r.playPos);
    });
    requestAnimationFrame(tick);
  }

  function togglePlay(row) {
    row.playing = !row.playing;
    row.playPos = null;
    row.el.classList.toggle('playing', row.playing);
    if (row.playing && lastTick == null) requestAnimationFrame(tick);
  }

  // ---- Style menu ----------------------------------------------------------

  var menu = document.getElementById('style-menu');
  var menuRow = null;

  function buildMenu() {
    var colors = menu.querySelector('.colors');
    COLORS.forEach(function (c) {
      var b = document.createElement('button');
      b.className = 'color';
      b.style.background = c;
      b.dataset.color = c;
      b.title = c;
      colors.appendChild(b);
    });
    menu.addEventListener('mousedown', function (e) { e.stopPropagation(); });
    menu.addEventListener('click', function (e) {
      if (!menuRow) return;
      var t = e.target.closest('button');
      if (!t) return;
      if (t.dataset.color) menuRow.color = t.dataset.color;
      if (t.dataset.style) menuRow.style = t.dataset.style;
      if (t.dataset.point) menuRow.pointStyle = t.dataset.point;
      paintTarget(menuRow);
      refreshMenu();
      sendRows();
    });
    menu.querySelector('.width').addEventListener('input', function (e) {
      if (!menuRow) return;
      menuRow.width = Number(e.target.value);
      sendRows();
    });
    menu.querySelector('.point-size').addEventListener('input', function (e) {
      if (!menuRow) return;
      menuRow.pointSize = Number(e.target.value);
      sendRows();
    });
    menu.querySelector('.opacity').addEventListener('input', function (e) {
      if (!menuRow) return;
      menuRow.opacity = Number(e.target.value);
      paintTarget(menuRow);
      sendRows();
    });
    ['dots', 'connect'].forEach(function (key) {
      menu.querySelector('.' + key).addEventListener('change', function (e) {
        if (!menuRow) return;
        menuRow[key] = e.target.checked;
        refreshMenu();
        sendRows();
      });
    });
    document.addEventListener('mousedown', closeMenu);
  }

  function refreshMenu() {
    if (!menuRow) return;
    menu.querySelectorAll('.color').forEach(function (b) {
      b.classList.toggle('chosen', b.dataset.color === menuRow.color);
    });
    menu.querySelectorAll('[data-style]').forEach(function (b) {
      b.classList.toggle('chosen', b.dataset.style === menuRow.style);
      b.style.color = menuRow.color;
    });
    menu.querySelectorAll('[data-point]').forEach(function (b) {
      b.classList.toggle('chosen', b.dataset.point === menuRow.pointStyle);
      b.style.color = menuRow.color;
    });
    // Point rows get point options, curves line options, as in Desmos.
    // Points can also be joined by lines, so they get both.
    var r = menuRow.result || {};
    var points = menuRow.isColumn || !!r.points;
    menu.classList.toggle('has-points', points);
    menu.classList.toggle('has-lines', !!r.lines || !points || menuRow.connect);
    menu.classList.toggle('no-dots', points && !menuRow.dots);
    menu.querySelector('.dots').checked = menuRow.dots;
    menu.querySelector('.connect').checked = menuRow.connect;
    menu.querySelector('.point-size').value = String(menuRow.pointSize);
    menu.querySelector('.width').value = String(menuRow.width);
    menu.querySelector('.opacity').value = String(menuRow.opacity);
  }

  function openMenu(target) {
    menuRow = target;
    var box = target.swatch.getBoundingClientRect();
    menu.style.top = Math.min(box.top, window.innerHeight - 230) + 'px';
    menu.style.left = (box.right + 8) + 'px';
    menu.classList.add('open');
    refreshMenu();
  }

  function closeMenu() {
    menu.classList.remove('open');
    menuRow = null;
  }

  // ---- Rows ----------------------------------------------------------------

  function paintTarget(t) {
    if (t.isColumn) paintColumn(t); else paintRow(t);
  }

  // Click a color circle to hide or show; hold it (or right-click) for the
  // style menu, as in Desmos.
  function bindSwatch(target) {
    var swatch = target.swatch;
    var pressTimer = null, longPressed = false;
    swatch.addEventListener('mousedown', function (e) {
      if (e.button !== 0) return;
      e.stopPropagation();
      e.preventDefault();
      longPressed = false;
      pressTimer = setTimeout(function () { longPressed = true; openMenu(target); }, LONG_PRESS_MS);
    });
    swatch.addEventListener('mouseup', function () { clearTimeout(pressTimer); });
    swatch.addEventListener('mouseleave', function () { clearTimeout(pressTimer); });
    swatch.addEventListener('click', function () {
      if (longPressed) return;
      target.hidden = !target.hidden;
      paintTarget(target);
      sendRows();
    });
    swatch.addEventListener('contextmenu', function (e) {
      e.preventDefault();
      openMenu(target);
    });
  }

  function newStyle() {
    return {
      color: COLORS[nextColor++ % COLORS.length],
      hidden: false,
      style: 'solid',
      width: DEFAULT_WIDTH,
      pointStyle: 'filled',
      pointSize: DEFAULT_POINT_SIZE,
      opacity: 1,
      dots: true,
      connect: false
    };
  }

  function paintRow(row) {
    row.swatch.style.borderColor = row.color;
    row.swatch.style.background = row.hidden ? '#fff' : row.color;
    row.swatch.style.opacity = String(0.35 + 0.65 * row.opacity);
    row.swatch.title = row.hidden ? 'Show (hold for style)' : 'Hide (hold for style)';
    row.play.style.color = row.color;
  }

  // Shows what the engine said about a row.
  function showResult(row, r) {
    var key = JSON.stringify(r);
    if (key === row.resultKey) return;
    row.resultKey = key;
    row.result = r;
    var el = row.el;

    var warn = r.error && r.kind !== 'empty';
    el.classList.toggle('has-error', !!warn);
    el.classList.toggle('not-yet', r.kind === 'notyet');
    row.warn.dataset.tip = r.error || '';

    var hasSlider = r.slider != null;
    el.classList.toggle('graphed', !!r.graphed && !warn);
    el.classList.toggle('slider', hasSlider);
    if (hasSlider) {
      el.querySelector('.lname').textContent = (r.name || '').replace(/_/g, '');
      var s = row.slider;
      if (r.slider > s.max) s.max = Math.ceil(r.slider);
      if (r.slider < s.min) s.min = Math.floor(r.slider);
      refreshSliderBox(row);
    } else if (row.playing) {
      togglePlay(row);
    }

    var showValue = r.value != null && !hasSlider && !warn;
    el.classList.toggle('has-value', showValue);
    if (showValue) row.valueMath.latex('=' + r.value);

    var p = r.parameter;
    el.classList.toggle('has-domain', !!p);
    if (p) {
      if (!row.domainShown) {
        // First time: Desmos's defaults, which the engine applies anyway.
        row.settingDomain = true;
        row.domainMin.latex(row.domain ? row.domain[0] : p['default'][0]);
        row.domainMax.latex(row.domain ? row.domain[1] : p['default'][1]);
        row.settingDomain = false;
        row.domainShown = true;
      }
      MQ.StaticMath(el.querySelector('.dname')).latex(p.latex);
      var domainEl = el.querySelector('.domain');
      domainEl.classList.toggle('bad', !!p.error);
      domainEl.title = p.error || '';
    }

    var missing = r.missing || [];
    el.classList.toggle('has-missing', missing.length > 0);
    row.missingButtons.innerHTML = '';
    missing.forEach(function (m) {
      row.missingButtons.appendChild(sliderButton(row, m.latex, [m]));
    });
    if (missing.length > 1) row.missingButtons.appendChild(sliderButton(row, null, missing, 'all'));
  }

  function sliderButton(row, latex, names, label) {
    var b = document.createElement('button');
    b.className = 'add-slider';
    if (latex) MQ.StaticMath(b).latex(latex); else b.textContent = label;
    b.addEventListener('click', function () {
      var at = indexOf(row) + 1;
      names.forEach(function (m, i) { addRow(m.latex + '=1', at + i); });
      sendRows();
    });
    return b;
  }

  function addRow(latex, at) {
    var el = document.createElement('div');
    el.className = 'row';
    el.innerHTML =
      '<div class="tab"><span class="index"></span>' +
      '<button class="swatch"></button>' +
      '<button class="play" title="Play"><span class="tri"></span><span class="bars"></span></button>' +
      '<span class="warn" tabindex="-1">⚠</span></div>' +
      '<div class="body">' +
      '<div class="math"><span class="field"></span></div>' +
      '<div class="value"><span class="value-math"></span></div>' +
      '<div class="slider-box"><span class="lim min" title="Edit range"></span>' +
      '<input type="range" min="-10" max="10" step="any">' +
      '<span class="lim max" title="Edit range"></span></div>' +
      '<div class="limits"><input class="lmin" size="5"> ≤ <span class="lname"></span> ≤ ' +
      '<input class="lmax" size="5"> Step: <input class="lstep" size="4"></div>' +
      '<div class="domain"><span class="bound dmin"></span> ≤ <span class="dname"></span> ≤ ' +
      '<span class="bound dmax"></span></div>' +
      '<div class="missing">add slider: <span class="buttons"></span></div>' +
      '</div>' +
      '<button class="remove" title="Delete">&#x2715;</button>';
    var row = {
      id: nextId++,
      el: el,
      indexEl: el.querySelector('.index'),
      swatch: el.querySelector('.swatch'),
      play: el.querySelector('.play'),
      warn: el.querySelector('.warn'),
      range: el.querySelector('input[type=range]'),
      minEl: el.querySelector('.lim.min'),
      maxEl: el.querySelector('.lim.max'),
      limMin: el.querySelector('.lmin'),
      limMax: el.querySelector('.lmax'),
      limStep: el.querySelector('.lstep'),
      missingButtons: el.querySelector('.missing .buttons'),
      valueMath: null,
      domain: null,
      domainShown: false,
      settingDomain: false,
      domainMin: null,
      domainMax: null,
      slider: { min: -10, max: 10, step: null },
      playing: false,
      settingSlider: false,
      playDir: 1,
      playPos: null,
      result: null,
      resultKey: '',
      field: null
    };
    Object.assign(row, newStyle());

    var options = Object.assign({}, MATH_OPTIONS, {
      handlers: {
        edit: function () { if (!row.settingSlider) row.playPos = null; sendRows(); },
        enter: function () { addRow('', indexOf(row) + 1).field.focus(); },
        upOutOf: function () { focusRow(indexOf(row) - 1, true, true); },
        downOutOf: function () { leaveDown(row); },
        deleteOutOf: function (dir) {
          if (dir === MQ.L && row.field.latex() === '' && rows.length > 1) removeRow(row);
        }
      }
    });

    var target = rows[at];
    if (target) list.insertBefore(el, target.el); else list.appendChild(el);
    rows.splice(at, 0, row);
    row.field = MQ.MathField(el.querySelector('.field'), options);
    row.valueMath = MQ.StaticMath(el.querySelector('.value-math'));
    // MathQuill keeps and rewrites the options it is given, so each field
    // needs an object of its own.
    var domainOptions = function () {
      return Object.assign({}, MATH_OPTIONS, {
        handlers: {
          edit: function () {
            if (row.settingDomain || !row.domainMin || !row.domainMax) return;
            row.domain = [row.domainMin.latex(), row.domainMax.latex()];
            sendRows();
          },
          enter: function () { row.field.focus(); }
        }
      });
    };
    row.domainMin = MQ.MathField(el.querySelector('.dmin'), domainOptions());
    row.domainMax = MQ.MathField(el.querySelector('.dmax'), domainOptions());
    if (latex) row.field.latex(latex);

    el.addEventListener('focusin', function (e) {
      if (!e.target.closest('.limits')) setActive(row);
    });
    el.querySelector('.math').addEventListener('mousedown', function (e) {
      if (e.target === e.currentTarget) { e.preventDefault(); row.field.focus(); }
    });
    el.querySelector('.remove').addEventListener('click', function () { removeRow(row); });

    bindSwatch(row);

    row.play.addEventListener('click', function () { togglePlay(row); });
    row.range.addEventListener('input', function () {
      row.playPos = null;
      setSlider(row, Number(row.range.value));
    });
    row.minEl.addEventListener('click', function () { openLimits(row); });
    row.maxEl.addEventListener('click', function () { openLimits(row); });
    [row.limMin, row.limMax, row.limStep].forEach(function (input) {
      input.addEventListener('change', function () { applyLimits(row); });
      input.addEventListener('keydown', function (e) {
        if (e.key === 'Enter' || e.key === 'Escape') {
          applyLimits(row);
          el.classList.remove('limits-open');
          row.field.focus();
        }
      });
    });
    el.addEventListener('focusout', function (e) {
      // Close the range editor once focus leaves it.
      if (e.relatedTarget && e.relatedTarget.closest && e.relatedTarget.closest('.limits')) return;
      if (e.target.closest('.limits')) {
        applyLimits(row);
        el.classList.remove('limits-open');
      }
    });

    paintRow(row);
    renumber();
    return row;
  }

  // ---- Tables --------------------------------------------------------------

  function activeIndex() {
    for (var i = 0; i < rows.length; i++) if (rows[i].el.classList.contains('active')) return i;
    return -1;
  }

  // Moving down out of the last line of a row: into the next row, or a new
  // expression when it was the last.
  function leaveDown(row) {
    var i = indexOf(row);
    if (i === rows.length - 1) addRow('', i + 1).field.focus(); else focusRow(i + 1, false);
  }

  // The lowest n with no x_n or y_n header in any table yet.
  function nextTableIndex() {
    var used = {};
    rows.forEach(function (r) {
      if (r.kind !== 'table') return;
      r.columns.forEach(function (c) {
        var m = /^[a-z]_\{?(\d+)\}?$/.exec(c.field.latex());
        if (m) used[m[1]] = true;
      });
    });
    var n = 1;
    while (used[n]) n++;
    return n;
  }

  // Next unused name for a new column: y_n, y_{n+1}, ...
  function nextColumnName(t) {
    var taken = {};
    rows.forEach(function (r) {
      if (r.kind === 'table') r.columns.forEach(function (c) { taken[c.field.latex()] = true; });
    });
    var n = t.index;
    while (taken['y_' + (n > 9 ? '{' + n + '}' : n)]) n++;
    return 'y_' + (n > 9 ? '{' + n + '}' : n);
  }

  function paintColumn(c) {
    c.swatch.style.borderColor = c.color;
    c.swatch.style.background = c.hidden ? '#fff' : c.color;
    c.swatch.style.opacity = String(0.35 + 0.65 * c.opacity);
  }

  // Focuses column `ci` at table row `ri` (-1 for the header).
  function tableFocus(t, ci, ri, atStart) {
    var c = t.columns[Math.max(0, Math.min(t.columns.length - 1, ci))];
    if (!c) return;
    var f = ri < 0 ? c.field : c.cells[Math.min(ri, t.nrows - 1)];
    f.focus();
    if (atStart) f.moveToLeftEnd(); else f.moveToRightEnd();
  }

  function cellOptions(t, c, ri) {
    // ri is a getter so the handlers follow the cell when rows move.
    return Object.assign({}, MATH_OPTIONS, {
      handlers: {
        edit: function () {
          if (t.filling) return;
          growTable(t);
          sendRows();
        },
        enter: function () {
          var r = ri();
          if (r === t.nrows - 1) addTableRow(t);
          tableFocus(t, t.columns.indexOf(c), r + 1);
        },
        upOutOf: function () {
          var r = ri();
          if (r < 0) focusRow(indexOf(t) - 1, true, true);
          else tableFocus(t, t.columns.indexOf(c), r - 1);
        },
        downOutOf: function () {
          var r = ri();
          if (r >= t.nrows - 1) leaveDown(t);
          else tableFocus(t, t.columns.indexOf(c), r + 1);
        },
        moveOutOf: function (dir) {
          var ci = t.columns.indexOf(c);
          if (dir === MQ.L) {
            if (ci > 0) tableFocus(t, ci - 1, ri());
            return;
          }
          // Moving right from the last column adds one, as in Desmos.
          if (ci === t.columns.length - 1) {
            if (c.field.latex() === '') return;
            addColumn(t, nextColumnName(t));
            sendRows();
          }
          tableFocus(t, ci + 1, ri(), true);
        },
        deleteOutOf: function (dir) {
          var ci = t.columns.indexOf(c);
          if (dir !== MQ.L) return;
          var f = ri() < 0 ? c.field : c.cells[ri()];
          if (f.latex() !== '') return;
          // Backspace in an empty header removes an empty column; in an
          // empty cell it steps left.
          var empty = c.cells.every(function (x) { return x.latex() === ''; });
          if (ri() < 0 && empty && t.columns.length > 1) {
            removeColumn(t, c);
            tableFocus(t, ci - 1, -1);
            sendRows();
          } else if (ci > 0) {
            tableFocus(t, ci - 1, ri());
          } else if (ri() > 0) {
            tableFocus(t, t.columns.length - 1, ri() - 1);
          }
        }
      }
    });
  }

  function addCell(t, c) {
    var ri = c.cells.length;
    var box = document.createElement('div');
    box.className = 'cell';
    box.innerHTML = '<span class="cell-field"></span><span class="cell-value"></span>';
    c.el.appendChild(box);
    var field = MQ.MathField(box.querySelector('.cell-field'), cellOptions(t, c, function () {
      return c.cells.indexOf(field);
    }));
    c.cells.push(field);
    c.values.push(MQ.StaticMath(box.querySelector('.cell-value')));
    box.addEventListener('mousedown', function (e) {
      if (!e.target.closest('.mq-editable-field')) { e.preventDefault(); field.focus(); }
    });
  }

  function addColumn(t, header, cells) {
    var c = Object.assign(newStyle(), {
      isColumn: true, table: t, cells: [], values: [], computed: false
    });
    c.el = document.createElement('div');
    c.el.className = 'col';
    c.el.innerHTML = '<div class="head"><button class="swatch col-swatch"></button>' +
      '<span class="head-field"></span><span class="warn col-warn" tabindex="-1">⚠</span></div>';
    t.grid.appendChild(c.el);
    c.swatch = c.el.querySelector('.swatch');
    c.warn = c.el.querySelector('.col-warn');
    c.field = MQ.MathField(c.el.querySelector('.head-field'), cellOptions(t, c, function () { return -1; }));
    t.columns.push(c);
    for (var r = 0; r < t.nrows; r++) addCell(t, c);
    t.filling = true;
    c.field.latex(header || '');
    (cells || []).forEach(function (v, r) { if (c.cells[r]) c.cells[r].latex(v); });
    t.filling = false;
    bindSwatch(c);
    paintColumn(c);
    return c;
  }

  function removeColumn(t, c) {
    var i = t.columns.indexOf(c);
    if (i < 0) return;
    c.el.parentNode.removeChild(c.el);
    t.columns.splice(i, 1);
  }

  function addTableRow(t) {
    t.nrows++;
    t.columns.forEach(function (c) { addCell(t, c); });
  }

  // Keeps one blank row at the bottom to type into.
  function growTable(t) {
    var last = t.nrows - 1;
    var used = t.columns.some(function (c) { return c.cells[last] && c.cells[last].latex() !== ''; });
    if (used) addTableRow(t);
  }

  function addTable(at, columns) {
    var el = document.createElement('div');
    el.className = 'row table-row';
    el.innerHTML =
      '<div class="tab"><span class="index"></span></div>' +
      '<div class="body"><div class="grid"></div>' +
      '<div class="missing">add slider: <span class="buttons"></span></div></div>' +
      '<button class="remove" title="Delete">&#x2715;</button>';
    var t = {
      kind: 'table',
      id: nextId++,
      el: el,
      indexEl: el.querySelector('.index'),
      grid: el.querySelector('.grid'),
      missingButtons: el.querySelector('.missing .buttons'),
      columns: [],
      nrows: 0,
      filling: false,
      result: null,
      resultKey: ''
    };
    var target = rows[at];
    if (target) list.insertBefore(el, target.el); else list.appendChild(el);
    rows.splice(at, 0, t);
    t.index = nextTableIndex();
    var sub = t.index > 9 ? '{' + t.index + '}' : String(t.index);
    columns = columns || [{ header: 'x_' + sub }, { header: 'y_' + sub }];
    t.nrows = Math.max(1, columns.reduce(function (n, c) { return Math.max(n, (c.cells || []).length); }, 0));
    columns.forEach(function (c) { addColumn(t, c.header, c.cells); });
    growTable(t);
    // The first column holds the inputs and is not drawn.
    t.columns[0].swatch.style.visibility = 'hidden';
    el.addEventListener('focusin', function () { setActive(t); });
    el.querySelector('.remove').addEventListener('click', function () { removeRow(t); });
    renumber();
    return t;
  }

  function showTableResult(t, r) {
    var key = JSON.stringify(r);
    if (key === t.resultKey) return;
    t.resultKey = key;
    t.result = r;
    (r.columns || []).forEach(function (cr, i) {
      var c = t.columns[i];
      if (!c) return;
      c.result = cr;
      c.el.classList.toggle('bad', !!cr.error);
      c.warn.dataset.tip = cr.error || '';
      c.swatch.classList.toggle('shown', i > 0 && !!cr.points && !cr.error);
      var computed = cr.computed != null;
      c.computed = computed;
      c.el.classList.toggle('computed', computed);
      var bad = {};
      (cr.cellErrors || []).forEach(function (e) { bad[e[0]] = e[1]; });
      c.cells.forEach(function (f, ri) {
        var box = f.el().parentNode;
        box.classList.toggle('bad', bad[ri] != null);
        box.title = bad[ri] || '';
        if (computed) c.values[ri].latex(cr.computed[ri] || '');
      });
    });
    var missing = r.missing || [];
    t.el.classList.toggle('has-missing', missing.length > 0);
    t.missingButtons.innerHTML = '';
    missing.forEach(function (m) {
      t.missingButtons.appendChild(sliderButton(t, m.latex, [m]));
    });
  }

  // The + button offers an expression or a table, as Desmos's does.
  var addMenu = document.getElementById('add-menu');
  document.getElementById('add').addEventListener('mousedown', function (e) {
    e.stopPropagation();
    addMenu.classList.toggle('open');
  });
  addMenu.addEventListener('mousedown', function (e) { e.stopPropagation(); });
  addMenu.addEventListener('click', function (e) {
    var b = e.target.closest('button');
    if (!b) return;
    addMenu.classList.remove('open');
    var at = activeIndex() + 1 || rows.length;
    if (b.dataset.add === 'table') {
      var t = addTable(at);
      tableFocus(t, 0, 0);
      sendRows();
    } else {
      addRow('', at).field.focus();
    }
  });
  document.addEventListener('mousedown', function () { addMenu.classList.remove('open'); });
  list.addEventListener('mousedown', function (e) {
    // Clicking the empty space under the rows adds a row, like Desmos.
    if (e.target === list) { e.preventDefault(); addRow('', rows.length).field.focus(); }
  });
  buildMenu();

  // ---- Saving and opening ---------------------------------------------------

  // The file menu (top left) and its shortcuts. The engine does the file
  // work; the panel only asks.
  var fileMenu = document.getElementById('file-menu');
  document.getElementById('menu').addEventListener('mousedown', function (e) {
    e.stopPropagation();
    fileMenu.classList.toggle('open');
  });
  fileMenu.addEventListener('mousedown', function (e) { e.stopPropagation(); });
  fileMenu.addEventListener('click', function (e) {
    var b = e.target.closest('button');
    if (!b) return;
    fileMenu.classList.remove('open');
    send({ type: b.dataset.file });
  });
  document.addEventListener('mousedown', function () { fileMenu.classList.remove('open'); });
  document.addEventListener('keydown', function (e) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
    var k = e.key.toLowerCase();
    var action = k === 's' ? (e.shiftKey ? 'saveAs' : 'save') : k === 'o' ? 'open' : null;
    if (!action) return;
    e.preventDefault();
    e.stopPropagation();
    send({ type: action });
  }, true);

  function applyStyle(target, saved) {
    ['color', 'hidden', 'style', 'width', 'opacity', 'pointStyle', 'pointSize', 'dots', 'connect']
      .forEach(function (k) { if (saved[k] !== undefined && saved[k] !== null) target[k] = saved[k]; });
  }

  // Replaces every row with the saved ones (the rows array the panel sends).
  function loadRows(saved) {
    loading = true;
    closeMenu();
    rows.slice().forEach(function (r) {
      if (r.playing) togglePlay(r);
      r.el.parentNode.removeChild(r.el);
    });
    rows = [];
    nextColor = 0;
    selectedId = null;
    (saved || []).forEach(function (s, i) {
      if (s.table) {
        var t = addTable(i, s.table.map(function (c) { return { header: c.header, cells: c.cells }; }));
        t.columns.forEach(function (c, ci) {
          applyStyle(c, s.table[ci] || {});
          paintColumn(c);
        });
        return;
      }
      var row = addRow(s.latex || '', i);
      applyStyle(row, s);
      if (s.slider && typeof s.slider.min === 'number') row.slider = s.slider;
      if (s.domain && s.domain.length === 2) row.domain = s.domain;
      paintRow(row);
    });
    if (rows.length === 0) addRow('', 0);
    // Colors carry on after the last one used, as in Desmos.
    nextColor = rows.length;
    loading = false;
    renumber();
    focusRow(0);
    sendRows();
  }

  // Called by Rust through evaluate_script.
  window.engine = {
    load: function (saved) { loadRows(saved); },
    info: function (adapter) {
      document.getElementById('engine').textContent = 'Graph engine: ' + adapter;
    },
    stats: function (sceneMs, frameMs, vertices) {
      document.getElementById('frame').textContent =
        'Last frame: ' + frameMs.toFixed(2) + ' ms (scene ' + sceneMs.toFixed(2) + ' ms, ' +
        vertices.toLocaleString() + ' vertices)';
    },
    analysis: function (results) {
      var byId = {};
      results.forEach(function (r) { byId[r.id] = r; });
      rows.forEach(function (row) {
        var r = byId[row.id];
        if (!r) return;
        if (row.kind === 'table') showTableResult(row, r); else showResult(row, r);
      });
    },
    // Clicking a curve on the graph selects its row; clicking empty paper
    // selects nothing, as in Desmos.
    select: function (id) {
      var row = rows.filter(function (r) { return r.id === id; })[0] || null;
      selectedId = id;
      setActive(row);
      if (row) row.el.scrollIntoView({ block: 'nearest' });
      else if (document.activeElement) document.activeElement.blur();
    },
    // Dragging a point on the graph moves the sliders it is made of.
    setSliders: function (pairs) {
      pairs.forEach(function (p) {
        var row = rows.filter(function (r) { return r.id === p[0]; })[0];
        if (!row || row.kind === 'table') return;
        row.playPos = null;
        setSlider(row, p[1], true);
      });
    },
    // Used by the smoke test: adds a slider and plays it.
    demo: function () {
      var at = rows.length;
      addRow('a=1', at);
      addRow('y=a\\cos\\left(x\\right)', at + 1);
      sendRows();
      togglePlay(rows[at]);
    }
  };

  addRow('y=\\sin\\left(x\\right)', 0);
  addRow('', 1);
  focusRow(0);
  // Rows first, so a graph the engine loads on 'ready' replaces them.
  sendRows();
  send({ type: 'ready' });
})();
