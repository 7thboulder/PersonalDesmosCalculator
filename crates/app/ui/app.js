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
    autoCommands: 'pi theta tau phi alpha beta gamma delta sqrt sum prod int nthroot infinity infty',
    autoOperatorNames: 'sin cos tan sec csc cot arcsin arccos arctan arcsec arccsc arccot ' +
      'sinh cosh tanh sech csch coth arcsinh arccosh arctanh ' +
      'ln log exp abs min max floor ceil round sign mod mean median total length ' +
      'gcd lcm nCr nPr for polygon join sort unique distance midpoint real imag conj arg ' +
      'det inv transpose rref trace rank eigenvalues eigenvectors identity ' +
      'mandelbrot julia cobweb bifurcation'
  };

  // Desmos subscripts hold only letters and digits, so typing anything else
  // at the end of one, like = or a comma, steps out of it first: t_rack=2
  // types as t_{rack}=2. MathQuill only does this for superscripts.
  document.addEventListener('keydown', function (e) {
    if (e.key.length !== 1 || /[A-Za-z0-9_]/.test(e.key) || e.ctrlKey || e.metaKey || e.altKey) return;
    var root = e.target.closest && e.target.closest('.mq-editable-field');
    var cursor = root && root.querySelector('.mq-cursor');
    // Typing -> makes the action arrow, as in Desmos.
    var before = cursor && cursor.previousElementSibling;
    if (e.key === '>' && before && /^[-\u2212]$/.test(before.textContent.trim())) {
      var mf = MQ(root);
      if (mf) {
        e.preventDefault();
        e.stopPropagation();
        mf.keystroke('Backspace');
        mf.cmd('\\to');
        return;
      }
    }
    if (!cursor) return;
    var block = cursor.parentElement;
    // An integral's bounds take any expression, as in Desmos, but MathQuill
    // keeps them in a subscript and superscript, which + - = < > leave.
    // Writing the character puts it in the bound instead.
    if (block.closest('.mq-int') && (block.classList.contains('mq-sub') ||
        block.classList.contains('mq-sup-inner')) && '+-=<>'.indexOf(e.key) >= 0) {
      var bound = MQ(root);
      if (bound) {
        e.preventDefault();
        e.stopPropagation();
        bound.write(e.key);
      }
      return;
    }
    if (cursor.nextElementSibling || !block.classList.contains('mq-sub')) return;
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
      dots: t.dots, connect: t.connect, fillOpacity: t.fillOpacity, complex: t.complex
    };
  }

  // Set while a saved graph is being loaded, so rows are sent once at the end.
  var loading = false;

  function sendRows() {
    if (loading) return;
    var sent = rows.map(function (r) {
        if (r.kind === 'note') return { id: r.id, latex: '', note: r.text.value };
        if (r.kind === 'matrix') {
          return Object.assign(styleOf(r), {
            id: r.id, latex: r.field.latex(),
            matrix: r.cells.map(function (line) { return line.map(function (f) { return f.latex(); }); }),
            transform: transformOf(r)
          });
        }
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
          id: r.id, latex: r.field.latex(), domain: r.domain, slider: r.slider,
          transform: transformOf(r),
          seeds: r.seeds, derivs: r.derivs
        });
      });
    // The ticker goes first, as a hidden row the engine analyzes like any
    // other; `ticker` marks it for saving and loading.
    if (ticker) {
      sent.unshift({
        id: ticker.id, latex: ticker.field.latex(), hidden: true,
        ticker: { step: ticker.step.latex(), playing: ticker.playing }
      });
    }
    if (ticker) ticker.waiting = true;
    send({ type: 'rows', rows: sent });
    remember(sent);
  }

  // ---- Undo and redo -------------------------------------------------------

  // Every state the rows have been in, as loadRows takes them, oldest first.
  // Changes in quick succession (typing a word, dragging a slider, a playing
  // ticker) make one step, as in Desmos.
  var history = [];
  var historyAt = -1;
  var lastChange = 0;
  var restoring = false;
  var MERGE_MS = 800;

  function snapshot(sent) {
    return JSON.stringify(sent.map(function (r) {
      var c = Object.assign({}, r);
      delete c.id;
      if (c.ticker) c.ticker = { step: c.ticker.step, playing: false };
      return c;
    }));
  }

  function remember(sent) {
    if (restoring) return;
    var state = snapshot(sent);
    if (history[historyAt] === state) return;
    var now = Date.now();
    history.length = historyAt + 1;
    // A change soon after the last one joins its step, unless that step is
    // the starting state.
    if (historyAt > 0 && now - lastChange < MERGE_MS) history[historyAt] = state;
    else history.push(state);
    historyAt = history.length - 1;
    lastChange = now;
    showUndoState();
  }

  function forgetHistory() {
    history = [];
    historyAt = -1;
  }

  function undoTo(at) {
    if (at < 0 || at >= history.length || at === historyAt) return;
    var focused = activeIndex();
    historyAt = at;
    restoring = true;
    loadRows(JSON.parse(history[at]));
    restoring = false;
    // The next change starts a new step.
    lastChange = 0;
    if (focused >= 0) focusRow(Math.min(focused, rows.length - 1));
    showUndoState();
  }

  function undo() { undoTo(historyAt - 1); }
  function redo() { undoTo(historyAt + 1); }

  var undoButton = document.getElementById('undo');
  var redoButton = document.getElementById('redo');
  function showUndoState() {
    undoButton.disabled = historyAt <= 0;
    redoButton.disabled = historyAt >= history.length - 1;
  }
  undoButton.addEventListener('click', undo);
  redoButton.addEventListener('click', redo);
  // Ctrl+Z undoes, Ctrl+Y or Ctrl+Shift+Z redoes, wherever the focus is.
  document.addEventListener('keydown', function (e) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
    var k = e.key.toLowerCase();
    if (k !== 'z' && k !== 'y') return;
    e.preventDefault();
    e.stopPropagation();
    if (k === 'y' || e.shiftKey) redo(); else undo();
  }, true);

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
    if (row.kind === 'matrix') {
      var f = fromBelow ? row.cells[row.cells.length - 1][0] : row.field;
      f.focus();
      if (atEnd === false) f.moveToLeftEnd(); else f.moveToRightEnd();
      return;
    }
    if (row.kind === 'note') {
      row.text.focus();
      var end = atEnd === false ? 0 : row.text.value.length;
      row.text.setSelectionRange(end, end);
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
      var mode = s.loop || 'bounce';
      if (mode !== 'bounce') r.playDir = 1;
      r.playPos += r.playDir * span * dt * (s.speed || 1) / PLAY_SECONDS;
      var done = false;
      if (mode === 'bounce') {
        // Back and forth between the limits, Desmos's default.
        if (r.playPos >= s.max) { r.playPos = s.max; r.playDir = -1; }
        if (r.playPos <= s.min) { r.playPos = s.min; r.playDir = 1; }
      } else if (mode === 'forward' && r.playPos >= s.max) {
        r.playPos = s.min;
      } else if (mode === 'once' && r.playPos >= s.max) {
        r.playPos = s.max;
        done = true;
      }
      setSlider(r, r.playPos);
      if (done) togglePlay(r);
    });
    requestAnimationFrame(tick);
  }

  // Desmos's play speeds, slowest first.
  var SPEEDS = [0.05, 0.1, 0.15, 0.2, 0.35, 0.5, 0.7, 1, 1.5, 2, 3, 4, 5, 10, 20];
  // Desmos's loop modes, in the order its loop button cycles through them.
  var LOOPS = [
    { mode: 'bounce', glyph: '\u21C4', title: 'Loop back and forth' },
    { mode: 'forward', glyph: '\u21BB', title: 'Loop forward' },
    { mode: 'once', glyph: '\u21E5', title: 'Play once' },
    { mode: 'forever', glyph: '\u221E', title: 'Play indefinitely' }
  ];

  function showPlayOptions(row) {
    var s = row.slider;
    var speed = s.speed || 1;
    row.speedEl.textContent = formatNumber(speed, 2) + '\u00D7';
    var loop = LOOPS.filter(function (l) { return l.mode === (s.loop || 'bounce'); })[0] || LOOPS[0];
    row.loopEl.textContent = loop.glyph;
    row.loopEl.title = loop.title;
  }

  function changeSpeed(row, by) {
    var speed = row.slider.speed || 1;
    // The nearest listed speed, then one step along.
    var at = 0;
    SPEEDS.forEach(function (v, i) {
      if (Math.abs(v - speed) < Math.abs(SPEEDS[at] - speed)) at = i;
    });
    row.slider.speed = SPEEDS[Math.max(0, Math.min(SPEEDS.length - 1, at + by))];
    showPlayOptions(row);
    sendRows();
  }

  function nextLoop(row) {
    var at = LOOPS.map(function (l) { return l.mode; }).indexOf(row.slider.loop || 'bounce');
    row.slider.loop = LOOPS[(at + 1) % LOOPS.length].mode;
    showPlayOptions(row);
    sendRows();
  }

  function togglePlay(row) {
    row.playing = !row.playing;
    row.playPos = null;
    // Playing once from the end starts again from the beginning.
    var s = row.slider;
    if (row.playing && s.loop === 'once' && row.result && row.result.slider >= s.max) {
      row.playPos = s.min;
    }
    if (row.playing) showPlayOptions(row);
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
      if (t.dataset.complex) menuRow.complex = t.dataset.complex;
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
    menu.querySelector('.fill-opacity').addEventListener('input', function (e) {
      if (!menuRow) return;
      menuRow.fillOpacity = Number(e.target.value);
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
    menu.querySelectorAll('[data-complex]').forEach(function (b) {
      b.classList.toggle('chosen', b.dataset.complex === menuRow.complex);
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
    menu.classList.toggle('has-fill', !!r.polygons);
    // A complex plot is drawn as colors or as a warped grid.
    menu.classList.toggle('has-complex', !!r.complex);
    menu.querySelector('.fill-opacity').value = String(menuRow.fillOpacity);
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
    if (t.isColumn) paintColumn(t);
    else if (t.kind === 'matrix') paintMatrix(t);
    else paintRow(t);
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
      fillOpacity: 0.4,
      dots: true,
      connect: false,
      complex: 'colors'
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

    el.classList.toggle('action', r.kind === 'action' && !warn);

    var showValue = (r.value != null || r.fit != null) && !hasSlider && !warn;
    el.classList.toggle('has-value', showValue);
    if (showValue) row.valueMath.latex(r.fit != null ? r.fit : '=' + r.value);
    showMatrixValue(row, warn ? null : r.matrix);
    showTransform(row, r);

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

    // A differential equation's initial points, which its solution
    // curves pass through. The first time, one at (0, 1) to start from.
    var ode = r.ode;
    el.classList.toggle('has-seeds', !!ode);
    if (ode) {
      if (!row.seedsShown) {
        row.seedsShown = true;
        if (row.seeds == null) {
          row.seeds = '\\left(0,1\\right)';
          setTimeout(sendRows, 0);
        }
        row.settingSeeds = true;
        row.seedsField.latex(row.seeds);
        row.settingSeeds = false;
      }
      var seedsEl = el.querySelector('.seeds');
      seedsEl.classList.toggle('bad', !!ode.error);
      seedsEl.title = ode.error || '';
      showDerivs(row, ode.order);
      var derivsEl = el.querySelector('.derivs');
      derivsEl.classList.toggle('bad', !!ode.derivsError);
      derivsEl.title = ode.derivsError || '';
    }

    var missing = r.missing || [];
    el.classList.toggle('has-missing', missing.length > 0);
    row.missingButtons.innerHTML = '';
    missing.forEach(function (m) {
      row.missingButtons.appendChild(sliderButton(row, m.latex, [m]));
    });
    if (missing.length > 1) row.missingButtons.appendChild(sliderButton(row, null, missing, 'all'));
  }

  // An equation of order n starts from y'(x₀) to y⁽ⁿ⁻¹⁾(x₀) as well as
  // its points: one box each, kept when the order drops so nothing typed
  // is lost.
  function showDerivs(row, order) {
    var want = Math.max(0, order - 1);
    if (row.derivFields.length === want) return;
    var box = row.el.querySelector('.derivs');
    box.innerHTML = '';
    row.derivFields = [];
    for (var i = 0; i < want; i++) {
      var label = document.createElement('span');
      MQ.StaticMath(label).latex('y' + "'".repeat(i + 1) + '\\left(x_0\\right)=');
      var field = document.createElement('span');
      field.className = 'bound';
      box.appendChild(label);
      box.appendChild(field);
      (function (i) {
        var mf = MQ.MathField(field, Object.assign({}, MATH_OPTIONS, {
          handlers: {
            edit: function () {
              if (row.settingSeeds) return;
              while (row.derivs.length <= i) row.derivs.push('');
              row.derivs[i] = mf.latex();
              sendRows();
            },
            enter: function () { row.field.focus(); }
          }
        }));
        row.settingSeeds = true;
        mf.latex(row.derivs[i] || '0');
        row.settingSeeds = false;
        row.derivFields.push(mf);
      })(i);
    }
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
      '<div class="matrix-value"><span class="mx-equals">=</span><div class="mx-bracket"></div></div>' +
      TRANSFORM_HTML +
      '<div class="run-box"><button class="run" title="Run this action">&#x25B6; Run</button></div>' +
      '<div class="slider-box"><span class="lim min" title="Edit range"></span>' +
      '<input type="range" min="-10" max="10" step="any">' +
      '<span class="lim max" title="Edit range"></span>' +
      '<span class="play-options"><button class="slower" title="Slower">\u2212</button>' +
      '<span class="speed"></span><button class="faster" title="Faster">+</button>' +
      '<button class="loop"></button></span></div>' +
      '<div class="limits"><input class="lmin" size="5"> ≤ <span class="lname"></span> ≤ ' +
      '<input class="lmax" size="5"> Step: <input class="lstep" size="4"></div>' +
      '<div class="domain"><span class="bound dmin"></span> ≤ <span class="dname"></span> ≤ ' +
      '<span class="bound dmax"></span></div>' +
      '<div class="seeds" title="">through <span class="bound seeds-field"></span>' +
      '<span class="derivs"></span></div>' +
      '<div class="missing">add slider: <span class="buttons"></span></div>' +
      '<div class="steps"></div>' +
      '</div>' +
      '<button class="solve-btn" title="Solve with steps (Ctrl+Alt+S)">Solve</button>' +
      '<button class="remove" title="Delete">&#x2715;</button>';
    var row = {
      id: nextId++,
      el: el,
      indexEl: el.querySelector('.index'),
      swatch: el.querySelector('.swatch'),
      play: el.querySelector('.play'),
      warn: el.querySelector('.warn'),
      range: el.querySelector('.slider-box input[type=range]'),
      minEl: el.querySelector('.lim.min'),
      maxEl: el.querySelector('.lim.max'),
      limMin: el.querySelector('.lmin'),
      limMax: el.querySelector('.lmax'),
      limStep: el.querySelector('.lstep'),
      speedEl: el.querySelector('.speed'),
      loopEl: el.querySelector('.loop'),
      missingButtons: el.querySelector('.missing .buttons'),
      stepsEl: el.querySelector('.steps'),
      solveLatex: null,
      solveOp: null,
      valueMath: null,
      domain: null,
      domainShown: false,
      settingDomain: false,
      domainMin: null,
      domainMax: null,
      seeds: null,
      derivs: [],
      derivFields: [],
      seedsShown: false,
      settingSeeds: false,
      seedsField: null,
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
        edit: function () {
          // Typing table into an empty row makes a table there, as in Desmos,
          // and matrix makes a matrix.
          if (!loading && row.field.latex() === 'table') { replaceWithTable(row); return; }
          if (!loading && row.field.latex() === 'matrix') { replaceWithMatrix(row); return; }
          if (!row.settingSlider) row.playPos = null;
          markStepsStale(row);
          sendRows();
        },
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
    row.seedsField = MQ.MathField(el.querySelector('.seeds-field'), Object.assign({}, MATH_OPTIONS, {
      handlers: {
        edit: function () {
          if (row.settingSeeds) return;
          row.seeds = row.seedsField.latex();
          sendRows();
        },
        enter: function () { row.field.focus(); }
      }
    }));
    if (latex) row.field.latex(latex);
    el.querySelector('.math').addEventListener('keydown', function (e) {
      if (e.key === '"' && row.field.latex() === '') {
        e.preventDefault();
        e.stopPropagation();
        replaceWithNote(row);
      }
    }, true);

    el.addEventListener('focusin', function (e) {
      if (!e.target.closest('.limits')) setActive(row);
    });
    el.querySelector('.math').addEventListener('mousedown', function (e) {
      if (e.target === e.currentTarget) { e.preventDefault(); row.field.focus(); }
    });
    el.querySelector('.remove').addEventListener('click', function () { removeRow(row); });
    el.querySelector('.solve-btn').addEventListener('mousedown', function (e) { e.preventDefault(); });
    el.querySelector('.solve-btn').addEventListener('click', function () { requestSolve(row, null); });
    el.querySelector('.run').addEventListener('mousedown', function (e) { e.preventDefault(); });
    el.querySelector('.run').addEventListener('click', function () {
      if (row.result) runUpdates(row.result.updates);
    });

    bindSwatch(row);
    bindTransform(row);

    row.play.addEventListener('click', function () { togglePlay(row); });
    el.querySelector('.slower').addEventListener('click', function () { changeSpeed(row, -1); });
    el.querySelector('.faster').addEventListener('click', function () { changeSpeed(row, 1); });
    row.loopEl.addEventListener('click', function () { nextLoop(row); });
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

  // ---- Solve ---------------------------------------------------------------

  // Symbolic algebra runs only when the Solve button (or Ctrl+Alt+S) asks for
  // it, never while typing. The engine sends back the working, step by step.
  function requestSolve(row, op) {
    if (!row || row.kind) return;
    var latex = row.field.latex();
    row.solveLatex = latex;
    row.solveOp = op;
    row.el.classList.add('has-steps', 'solving');
    row.el.classList.remove('steps-stale');
    if (!row.stepsEl.firstChild) row.stepsEl.innerHTML = '<div class="steps-wait">Solving…</div>';
    send({ type: 'solve', id: row.id, latex: latex, op: op });
  }

  function closeSteps(row) {
    row.el.classList.remove('has-steps', 'solving', 'steps-stale');
    row.stepsEl.innerHTML = '';
    row.solveLatex = null;
  }

  function markStepsStale(row) {
    if (row.solveLatex == null) return;
    row.el.classList.toggle('steps-stale', row.field.latex() !== row.solveLatex);
  }

  // Words with maths between $ signs, as the engine writes its notes.
  function richText(el, text) {
    text.split('$').forEach(function (part, i) {
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

  function mathBlock(className, latex) {
    var div = document.createElement('div');
    div.className = className;
    var span = document.createElement('span');
    div.appendChild(span);
    MQ.StaticMath(span).latex(latex);
    return div;
  }

  function showSteps(row, r) {
    row.el.classList.remove('solving');
    var box = row.stepsEl;
    box.innerHTML = '';
    var head = document.createElement('div');
    head.className = 'steps-head';
    (r.choices || []).forEach(function (c) {
      var b = document.createElement('button');
      b.className = 'chip' + (c.op === r.op ? ' on' : '');
      richText(b, c.label);
      b.addEventListener('mousedown', function (e) { e.preventDefault(); });
      b.addEventListener('click', function () { requestSolve(row, c.op); });
      head.appendChild(b);
    });
    var close = document.createElement('button');
    close.className = 'steps-close';
    close.title = 'Close the working';
    close.innerHTML = '&#x2715;';
    close.addEventListener('click', function () { closeSteps(row); });
    head.appendChild(close);
    box.appendChild(head);

    var stale = document.createElement('div');
    stale.className = 'steps-stale-note';
    stale.textContent = 'The expression changed. ';
    var again = document.createElement('button');
    again.textContent = 'Solve again';
    again.addEventListener('click', function () { requestSolve(row, row.solveOp); });
    stale.appendChild(again);
    box.appendChild(stale);

    var list = document.createElement('ol');
    list.className = 'steps-list';
    (r.steps || []).forEach(function (st) {
      var li = document.createElement('li');
      if (st.note) {
        var note = document.createElement('div');
        note.className = 'step-note';
        richText(note, st.note);
        li.appendChild(note);
      }
      if (st.latex) li.appendChild(mathBlock('step-math', st.latex));
      list.appendChild(li);
    });
    box.appendChild(list);

    if (r.result) {
      var res = mathBlock('steps-result', r.result);
      if (r.answer) {
        var add = document.createElement('button');
        add.className = 'steps-add';
        add.textContent = 'Add to graph';
        add.title = 'Add the answer as a new expression below';
        add.addEventListener('click', function () {
          addRow(r.answer, indexOf(row) + 1);
          sendRows();
        });
        res.appendChild(add);
      }
      box.appendChild(res);
    }
    if (r.error) {
      var err = document.createElement('div');
      err.className = 'steps-error';
      richText(err, r.error);
      box.appendChild(err);
    }
    markStepsStale(row);
  }

  // ---- Notes ---------------------------------------------------------------

  // A note: a row of plain text between expressions, as in Desmos. The engine
  // gets it as an empty row, so it draws nothing and has no result.
  function addNote(at, text) {
    var el = document.createElement('div');
    el.className = 'row note-row';
    el.innerHTML =
      '<div class="tab"><span class="index"></span></div>' +
      '<div class="body"><textarea class="note-text" rows="1" spellcheck="true"></textarea></div>' +
      '<button class="remove" title="Delete">&#x2715;</button>';
    var note = {
      kind: 'note',
      id: nextId++,
      el: el,
      indexEl: el.querySelector('.index'),
      text: el.querySelector('.note-text')
    };
    var area = note.text;
    area.value = text || '';
    var fit = function () {
      area.style.height = 'auto';
      area.style.height = area.scrollHeight + 'px';
    };
    var target = rows[at];
    if (target) list.insertBefore(el, target.el); else list.appendChild(el);
    rows.splice(at, 0, note);
    fit();
    area.addEventListener('input', function () { fit(); sendRows(); });
    area.addEventListener('keydown', function (e) {
      var i = indexOf(note);
      var start = area.selectionStart, end = area.selectionEnd;
      if (e.key === 'Enter' && !e.shiftKey) {
        // Enter starts a new expression below; Shift+Enter breaks the line.
        e.preventDefault();
        addRow('', i + 1).field.focus();
        sendRows();
      } else if (e.key === 'ArrowUp' && start === end &&
          area.value.lastIndexOf('\n', start - 1) < 0) {
        e.preventDefault();
        focusRow(i - 1, true, true);
      } else if (e.key === 'ArrowDown' && start === end &&
          area.value.indexOf('\n', start) < 0) {
        e.preventDefault();
        leaveDown(note);
      } else if (e.key === 'Backspace' && area.value === '' && rows.length > 1) {
        e.preventDefault();
        removeRow(note);
      }
    });
    el.addEventListener('focusin', function () { setActive(note); });
    el.querySelector('.remove').addEventListener('click', function () { removeRow(note); });
    renumber();
    return note;
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
      stepsEl: el.querySelector('.steps'),
      solveLatex: null,
      solveOp: null,
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


  // ---- Matrices --------------------------------------------------------------

  // A matrix row: a name, then a grid of cells, each an expression that may
  // use sliders. Rows such as det(A), A^{-1} or A(1, 2) use it by name.
  var MATRIX_LIMIT = 20;
  var TRANSFORM_SECONDS = 2;

  // The first capital letter no row has taken as a name yet.
  function nextMatrixName() {
    var taken = {};
    rows.forEach(function (r) {
      if (!r.field) return;
      var m = /^([A-Za-z](?:_\{?\w+\}?)?)=?/.exec(r.field.latex());
      if (m) taken[m[1]] = true;
    });
    var letters = 'ABCDEFGHJKLMNPQRSUVW';
    for (var i = 0; i < letters.length; i++) if (!taken[letters[i]]) return letters[i];
    return 'M_{' + rows.length + '}';
  }

  function matrixFocus(m, ri, ci, atStart) {
    var line = m.cells[Math.max(0, Math.min(m.cells.length - 1, ri))];
    var f = line[Math.max(0, Math.min(line.length - 1, ci))];
    f.focus();
    if (atStart) f.moveToLeftEnd(); else f.moveToRightEnd();
  }

  function where(m, field) {
    for (var ri = 0; ri < m.cells.length; ri++) {
      var ci = m.cells[ri].indexOf(field);
      if (ci >= 0) return [ri, ci];
    }
    return [0, 0];
  }

  function matrixCellOptions(m, getField) {
    return Object.assign({}, MATH_OPTIONS, {
      handlers: {
        edit: function () { if (!m.filling) sendRows(); },
        // Enter starts the next row of the matrix, so it is typed row by
        // row; from the last row it goes on to a new expression.
        enter: function () {
          var at = where(m, getField());
          if (at[0] < m.cells.length - 1) matrixFocus(m, at[0] + 1, 0);
          else leaveDown(m);
        },
        upOutOf: function () {
          var at = where(m, getField());
          if (at[0] > 0) matrixFocus(m, at[0] - 1, at[1]); else m.field.focus();
        },
        downOutOf: function () {
          var at = where(m, getField());
          if (at[0] < m.cells.length - 1) matrixFocus(m, at[0] + 1, at[1]); else leaveDown(m);
        },
        moveOutOf: function (dir) {
          var at = where(m, getField());
          var line = m.cells[at[0]];
          if (dir === MQ.L) {
            if (at[1] > 0) matrixFocus(m, at[0], at[1] - 1);
            else if (at[0] > 0) matrixFocus(m, at[0] - 1, line.length - 1);
            else { m.field.focus(); m.field.moveToRightEnd(); }
          } else if (at[1] < line.length - 1) {
            matrixFocus(m, at[0], at[1] + 1, true);
          } else if (at[0] < m.cells.length - 1) {
            matrixFocus(m, at[0] + 1, 0, true);
          }
        },
        deleteOutOf: function (dir) {
          if (dir !== MQ.L) return;
          var at = where(m, getField());
          if (at[1] > 0) matrixFocus(m, at[0], at[1] - 1);
          else if (at[0] > 0) matrixFocus(m, at[0] - 1, m.cells[at[0] - 1].length - 1);
        }
      }
    });
  }

  // Rebuilds the grid at a new size, keeping what fits.
  function resizeMatrix(m, nrows, ncols) {
    nrows = Math.max(1, Math.min(MATRIX_LIMIT, nrows));
    ncols = Math.max(1, Math.min(MATRIX_LIMIT, ncols));
    var old = m.cells.map(function (line) { return line.map(function (f) { return f.latex(); }); });
    fillMatrix(m, nrows, ncols, old);
    sendRows();
  }

  function fillMatrix(m, nrows, ncols, values) {
    m.grid.innerHTML = '';
    m.cells = [];
    m.filling = true;
    for (var ri = 0; ri < nrows; ri++) {
      var tr = document.createElement('tr');
      var line = [];
      for (var ci = 0; ci < ncols; ci++) {
        var td = document.createElement('td');
        td.innerHTML = '<span class="mx-cell"></span>';
        tr.appendChild(td);
        (function (td) {
          var field = MQ.MathField(td.firstChild, matrixCellOptions(m, function () { return field; }));
          var v = values && values[ri] && values[ri][ci];
          if (v) field.latex(v);
          line.push(field);
          td.addEventListener('mousedown', function (e) {
            if (!e.target.closest('.mq-editable-field')) { e.preventDefault(); field.focus(); }
          });
        })(td);
      }
      m.cells.push(line);
      m.grid.appendChild(tr);
    }
    m.filling = false;
    m.sizeEl.textContent = nrows + ' × ' + ncols;
  }

  function addMatrix(at, name, values) {
    var el = document.createElement('div');
    el.className = 'row matrix-row';
    el.innerHTML =
      '<div class="tab"><span class="index"></span>' +
      '<button class="swatch"></button>' +
      '<span class="warn" tabindex="-1">⚠</span></div>' +
      '<div class="body">' +
      '<div class="mx-line"><span class="mx-name"></span><span class="mx-equals">=</span>' +
      '<div class="mx-bracket"><table class="mx-grid"></table></div></div>' +
      '<div class="mx-size">' +
      '<button data-size="r-" title="Remove a row">−</button><span class="mx-dim">rows</span>' +
      '<button data-size="r+" title="Add a row">+</button>' +
      '<button data-size="c-" title="Remove a column">−</button><span class="mx-dim">columns</span>' +
      '<button data-size="c+" title="Add a column">+</button>' +
      '<span class="mx-size-label"></span></div>' +
      TRANSFORM_HTML +
      '<div class="missing">add slider: <span class="buttons"></span></div>' +
      '</div>' +
      '<button class="remove" title="Delete">&#x2715;</button>';
    var m = Object.assign(newStyle(), {
      kind: 'matrix',
      id: nextId++,
      el: el,
      indexEl: el.querySelector('.index'),
      swatch: el.querySelector('.swatch'),
      warn: el.querySelector('.warn'),
      grid: el.querySelector('.mx-grid'),
      sizeEl: el.querySelector('.mx-size-label'),
      missingButtons: el.querySelector('.missing .buttons'),
      cells: [],
      filling: false,
      result: null,
      resultKey: ''
    });
    var target = rows[at];
    if (target) list.insertBefore(el, target.el); else list.appendChild(el);
    rows.splice(at, 0, m);
    m.field = MQ.MathField(el.querySelector('.mx-name'), Object.assign({}, MATH_OPTIONS, {
      handlers: {
        edit: function () { if (!m.filling) sendRows(); },
        enter: function () { matrixFocus(m, 0, 0); },
        moveOutOf: function (dir) { if (dir === MQ.R) matrixFocus(m, 0, 0, true); },
        downOutOf: function () { matrixFocus(m, 0, 0); },
        upOutOf: function () { focusRow(indexOf(m) - 1, true, true); },
        deleteOutOf: function (dir) {
          var empty = m.cells.every(function (line) {
            return line.every(function (f) { return f.latex() === ''; });
          });
          if (dir === MQ.L && m.field.latex() === '' && empty && rows.length > 1) removeRow(m);
        }
      }
    }));
    m.filling = true;
    m.field.latex(name == null ? nextMatrixName() : name);
    m.filling = false;
    values = values && values.length ? values : [['', ''], ['', '']];
    fillMatrix(m, values.length, Math.max.apply(null, values.map(function (l) { return l.length || 1; })), values);
    el.querySelector('.mx-size').addEventListener('mousedown', function (e) {
      var b = e.target.closest('button');
      if (!b) return;
      e.preventDefault();
      var nr = m.cells.length, nc = m.cells[0].length;
      var k = b.dataset.size;
      resizeMatrix(m, nr + (k === 'r+' ? 1 : k === 'r-' ? -1 : 0), nc + (k === 'c+' ? 1 : k === 'c-' ? -1 : 0));
    });
    el.addEventListener('focusin', function () { setActive(m); });
    el.querySelector('.remove').addEventListener('click', function () { removeRow(m); });
    bindSwatch(m);
    bindTransform(m);
    paintMatrix(m);
    renumber();
    return m;
  }

  function paintMatrix(m) {
    m.swatch.style.borderColor = m.color;
    m.swatch.style.background = m.hidden ? '#fff' : m.color;
    m.swatch.style.opacity = String(0.35 + 0.65 * m.opacity);
  }

  function showMatrixResult(m, r) {
    var key = JSON.stringify(r);
    if (key === m.resultKey) return;
    m.resultKey = key;
    m.result = r;
    var warn = !!r.error;
    m.el.classList.toggle('has-error', warn);
    m.el.classList.toggle('graphed', !!r.graphed && !warn);
    m.warn.dataset.tip = r.error || '';
    showTransform(m, r);
    var missing = r.missing || [];
    m.el.classList.toggle('has-missing', missing.length > 0);
    m.missingButtons.innerHTML = '';
    missing.forEach(function (x) { m.missingButtons.appendChild(sliderButton(m, x.latex, [x])); });
    if (missing.length > 1) m.missingButtons.appendChild(sliderButton(m, null, missing, 'all'));
  }

  // A matrix value under an expression row, such as rref(A).
  function showMatrixValue(row, mx) {
    var box = row.el.querySelector('.matrix-value');
    row.el.classList.toggle('has-matrix', !!mx);
    var key = mx ? JSON.stringify(mx.cells) : '';
    if (key === row.matrixKey) return;
    row.matrixKey = key;
    var bracket = box.querySelector('.mx-bracket');
    bracket.innerHTML = '';
    if (!mx) return;
    var table = document.createElement('table');
    table.className = 'mx-grid';
    mx.cells.forEach(function (line) {
      var tr = document.createElement('tr');
      line.forEach(function (latex) {
        var td = document.createElement('td');
        var span = document.createElement('span');
        span.textContent = latex;
        td.appendChild(span);
        tr.appendChild(td);
        MQ.StaticMath(span);
      });
      table.appendChild(tr);
    });
    bracket.appendChild(table);
  }

  // ---- The transformation view ---------------------------------------------

  // A 2×2 matrix can show what it does to the plane, 3Blue1Brown style: the
  // grid, the unit square and the basis arrows move from where they are to
  // where the matrix sends them. t runs from 0 (nothing done) to 1.
  var TRANSFORM_HTML =
    '<div class="transform-box">' +
    '<button class="tf-toggle" title="Show what this matrix does to the plane">' +
    '<span class="tf-check"></span>Show transformation</button>' +
    '<span class="tf-controls"><button class="tf-play" title="Play the transformation">' +
    '<span class="tri"></span><span class="bars"></span></button>' +
    '<input class="tf-t" type="range" min="0" max="1" step="any">' +
    '<span class="tf-label"></span></span></div>';

  function transformOf(row) {
    return row.transform && row.transform.on ? row.transform.t : undefined;
  }

  function bindTransform(row) {
    var box = row.el.querySelector('.transform-box');
    row.tfRange = box.querySelector('.tf-t');
    row.tfLabel = box.querySelector('.tf-label');
    box.addEventListener('mousedown', function (e) {
      if (e.target.closest('button')) e.preventDefault();
    });
    box.querySelector('.tf-toggle').addEventListener('click', function () {
      var on = !(row.transform && row.transform.on);
      row.transform = { on: on, t: on ? 0 : 1 };
      paintTransform(row);
      sendRows();
      if (on) playTransform(row);
    });
    box.querySelector('.tf-play').addEventListener('click', function () {
      if (!row.transform) return;
      if (row.transform.playing) { row.transform.playing = false; paintTransform(row); return; }
      if (row.transform.t >= 1) row.transform.t = 0;
      playTransform(row);
    });
    row.tfRange.addEventListener('input', function () {
      row.transform.playing = false;
      row.transform.t = Number(row.tfRange.value);
      paintTransform(row);
      sendRows();
    });
  }

  function paintTransform(row) {
    var tf = row.transform || { on: false, t: 1 };
    row.el.classList.toggle('transforming', !!tf.on);
    row.el.classList.toggle('tf-playing', !!tf.playing);
    row.tfRange.value = String(tf.t);
    row.tfLabel.textContent = 't = ' + tf.t.toFixed(2);
  }

  function playTransform(row) {
    var tf = row.transform;
    tf.playing = true;
    paintTransform(row);
    var last = null;
    function frame(now) {
      if (!tf.playing || row.transform !== tf || !document.body.contains(row.el)) return;
      if (last != null) tf.t = Math.min(1, tf.t + (now - last) / 1000 / TRANSFORM_SECONDS);
      last = now;
      if (tf.t >= 1) tf.playing = false;
      paintTransform(row);
      sendRows();
      if (tf.playing) requestAnimationFrame(frame);
    }
    requestAnimationFrame(frame);
  }

  function showTransform(row, r) {
    var can = !!(r.matrix && r.matrix.transforms) && !r.error;
    row.el.classList.toggle('can-transform', can);
    if (!can && row.transform && row.transform.on) {
      row.transform = { on: false, t: 1 };
      paintTransform(row);
    }
  }

  function replaceWithMatrix(row) {
    var i = indexOf(row);
    row.el.parentNode.removeChild(row.el);
    rows.splice(i, 1);
    var m = addMatrix(i);
    renumber();
    matrixFocus(m, 0, 0);
    sendRows();
  }

  // ---- Actions and the ticker -----------------------------------------------

  // Runs an action: each variable it sets has its row rewritten with the
  // new value, keeping the left side as typed, as Desmos does.
  function runUpdates(updates) {
    if (!updates || !updates.length) return;
    // Every row changes before the rows go out once, so no analysis (and no
    // ticker run) ever sees some of an action's updates without the rest.
    loading = true;
    updates.forEach(function (u) {
      var row = rows.filter(function (r) { return r.id === u.row; })[0];
      if (!row || row.kind) return;
      var latex = row.field.latex();
      var eq = latex.indexOf('=');
      if (eq < 0) return;
      row.settingSlider = true;
      row.field.latex(latex.slice(0, eq) + '=' + u.latex);
      row.settingSlider = false;
      row.playPos = null;
    });
    loading = false;
    sendRows();
  }

  // The ticker runs an action over and over, at most once per `step`
  // milliseconds, as Desmos's does. It waits for each run's results before
  // the next, so every run sees the values the last one set.
  var ticker = null;

  function addTicker(latex, step) {
    if (ticker) return ticker;
    var el = document.createElement('div');
    el.id = 'ticker';
    el.innerHTML =
      '<button class="tick-play" title="Run the ticker"><span class="tri"></span><span class="bars"></span></button>' +
      '<div class="tick-body"><div class="tick-line"><span class="tick-label">Run</span>' +
      '<span class="tick-field"></span></div>' +
      '<div class="tick-line"><span class="tick-label">every</span><span class="tick-step"></span>' +
      '<span class="tick-label">ms</span></div>' +
      '<div class="tick-error"></div></div>' +
      '<button class="remove" title="Delete ticker">&#x2715;</button>';
    list.parentNode.insertBefore(el, list);
    ticker = {
      id: nextId++, el: el, playing: false, waiting: false, last: 0, result: null,
      field: MQ.MathField(el.querySelector('.tick-field'), Object.assign({}, MATH_OPTIONS, {
        handlers: { edit: function () { sendRows(); } }
      })),
      step: MQ.MathField(el.querySelector('.tick-step'), Object.assign({}, MATH_OPTIONS, {
        handlers: { edit: function () { sendRows(); } }
      }))
    };
    ticker.field.latex(latex);
    ticker.step.latex(step);
    el.querySelector('.tick-play').addEventListener('click', function () { toggleTicker(); });
    el.querySelector('.remove').addEventListener('click', function () { removeTicker(); sendRows(); });
    return ticker;
  }

  function removeTicker() {
    if (!ticker) return;
    ticker.el.parentNode.removeChild(ticker.el);
    ticker = null;
  }

  function toggleTicker() {
    if (!ticker) return;
    ticker.playing = !ticker.playing;
    ticker.el.classList.toggle('playing', ticker.playing);
    if (ticker.playing) {
      ticker.last = 0;
      requestAnimationFrame(tickerFrame);
    }
  }

  function tickerStep() {
    var v = Number(ticker.step.latex().replace(/\\left|\\right|[{}]/g, ''));
    return isFinite(v) && v > 0 ? v : 0;
  }

  function tickerFrame(now) {
    if (!ticker || !ticker.playing) return;
    if (!ticker.waiting && now - ticker.last >= tickerStep()) {
      var r = ticker.result;
      if (r && r.kind === 'action' && !r.error) {
        ticker.last = now;
        runUpdates(r.updates);
      }
    }
    requestAnimationFrame(tickerFrame);
  }

  function showTickerResult(r) {
    ticker.waiting = false;
    ticker.result = r || null;
    var error = r && r.error ? r.error :
      r && r.kind !== 'action' && r.kind !== 'empty' ? 'The ticker runs an action, like a \u2192 a+1' : '';
    ticker.el.classList.toggle('has-error', !!error);
    ticker.el.querySelector('.tick-error').textContent = error;
  }

  // ---- On-screen keypad ------------------------------------------------------

  // The keys type into whichever field was focused last, the way Desmos's
  // keypad does; pressing one never takes the focus away from it.
  var lastField = null;
  document.addEventListener('focusin', function (e) {
    var f = e.target.closest && e.target.closest('.mq-editable-field');
    if (f) lastField = f;
  });
  function currentField() {
    var mf = lastField && document.body.contains(lastField) && MQ(lastField);
    if (mf) return mf;
    focusRow(Math.max(0, activeIndex()));
    return lastField && MQ(lastField);
  }

  // Each key is [label, action, class]. Actions: t: types text as if from
  // the keyboard, c: a MathQuill command, w: writes LaTeX, k: a keystroke,
  // and fn/abc/123/shift switch panels.
  var MAIN_KEYS = [
    ['<i>x</i>', 't:x'], ['<i>y</i>', 't:y'], ['<i>a</i><sup>2</sup>', 'w:^2'], ['<i>a</i><sup>b</sup>', 'c:^'], null,
    ['7', 't:7'], ['8', 't:8'], ['9', 't:9'], ['\u00f7', 'c:/'], null,
    ['functions', 'fn', 'dark wide2'],
    ['(', 't:('], [')', 't:)'], ['&lt;', 't:<'], ['&gt;', 't:>'], null,
    ['4', 't:4'], ['5', 't:5'], ['6', 't:6'], ['\u00d7', 't:*'], null,
    ['\u2190', 'k:Left', 'dark'], ['\u2192', 'k:Right', 'dark'],
    ['|<i>a</i>|', 'c:|'], [',', 't:,'], ['\u2264', 'c:\\le'], ['\u2265', 'c:\\ge'], null,
    ['1', 't:1'], ['2', 't:2'], ['3', 't:3'], ['\u2212', 't:-'], null,
    ['\u232b', 'k:Backspace', 'dark wide2'],
    ['ABC', 'abc', 'dark'], ['<i>a</i><sub>b</sub>', 'c:_'], ['\u221a', 'c:\\sqrt'], ['\u03c0', 'c:\\pi'], null,
    ['0', 't:0'], ['.', 't:.'], ['=', 't:='], ['+', 't:+'], null,
    ['\u21b5', 'k:Enter', 'blue wide2']
  ];
  var ABC_KEYS = 'qwertyuiop'.split('').concat('asdfghjkl'.split('')).map(function (ch) {
    return ['<i>' + ch + '</i>', 't:' + ch, 'letter'];
  });
  ABC_KEYS.push(['\u03b8', 'c:\\theta']);
  ABC_KEYS.push(['\u21e7', 'shift', 'dark']);
  'zxcvbnm'.split('').forEach(function (ch) { ABC_KEYS.push(['<i>' + ch + '</i>', 't:' + ch, 'letter']); });
  ABC_KEYS.push(['\u232b', 'k:Backspace', 'dark wide2']);
  ABC_KEYS.push(['123', '123', 'dark wide2'], ['<i>a</i><sub>b</sub>', 'c:_'], ['!', 't:!'],
    ['[', 't:['], [']', 't:]'], ['{', 't:{'], ['~', 'c:\\sim'], ['\u21b5', 'k:Enter', 'blue wide2']);

  // Named functions are typed out, so they come out exactly as typing them
  // would.
  function named(list) {
    return list.split(' ').map(function (n) { return [n, 'f:' + n]; });
  }
  var FUNCTION_GROUPS = [
    ['TRIG FUNCTIONS', named('sin cos tan csc sec cot')],
    ['INVERSE TRIG', named('arcsin arccos arctan arccsc arcsec arccot')],
    ['HYPERBOLIC', named('sinh cosh tanh csch sech coth')],
    ['STATISTICS', named('mean median min max total length')],
    ['LIST OPERATIONS', named('join sort unique').concat([['for', 't: for ']])],
    ['CALCULUS', [['d/dx', 'w:\\frac{d}{dx}'], ['f\u2032', "t:'"], ['\u03a3', 'c:\\sum'], ['\u220f', 'c:\\prod'],
      ['\u222b', 'c:\\int'], ['\u221e', 'c:\\infty'], ['exp', 'f:exp'], ['ln', 'f:ln'], ['log', 'f:log']]],
    ['NUMBER THEORY', named('gcd lcm mod floor ceil round abs sign nCr nPr').concat([['n!', 't:!']])],
    ['GEOMETRY', named('polygon distance midpoint')],
    ['COMPLEX', named('real imag conj arg')],
    ['MATRICES', named('det inv transpose rref trace rank eigenvalues eigenvectors identity')],
    ['FRACTALS', named('mandelbrot julia cobweb bifurcation')]
  ];

  var keypad = document.getElementById('keypad');
  var keysButton = document.getElementById('keys');
  var keypadMode = 'main';
  var shifted = false;

  function keyButtons(keys) {
    return keys.map(function (k) {
      if (!k) return '<span class="gap"></span>';
      return '<button class="' + (k[2] || '') + '" data-key="' + k[1].replace(/"/g, '&quot;') + '">' +
        k[0] + '</button>';
    }).join('');
  }

  function drawKeypad() {
    var html = '';
    if (keypadMode === 'fn') {
      html += '<div class="functions">' + FUNCTION_GROUPS.map(function (g) {
        return '<h3>' + g[0] + '</h3><div class="keys">' + keyButtons(g[1]) + '</div>';
      }).join('') + '</div>';
    }
    html += keypadMode === 'abc' ?
      '<div class="keys abc">' + keyButtons(ABC_KEYS) + '</div>' :
      '<div class="keys main">' + keyButtons(MAIN_KEYS) + '</div>';
    keypad.innerHTML = html;
    if (keypadMode === 'abc') {
      keypad.querySelectorAll('.letter').forEach(function (b) {
        var ch = b.dataset.key.slice(2);
        if (shifted) ch = ch.toUpperCase();
        b.dataset.key = 't:' + ch;
        b.innerHTML = '<i>' + ch + '</i>';
      });
      keypad.querySelector('[data-key="shift"]').classList.toggle('held', shifted);
    }
    if (keypadMode === 'fn') keypad.querySelector('[data-key="fn"]').classList.add('held');
  }

  function showKeypad(open) {
    keypad.classList.toggle('open', open);
    keysButton.classList.toggle('on', open);
    keysButton.title = (open ? 'Hide' : 'Show') + ' keypad (Ctrl+Alt+K)';
    if (open) drawKeypad();
    try { localStorage.setItem('keypad', open ? '1' : ''); } catch (e) { /* storage is optional */ }
  }

  function pressKey(action) {
    if (action === 'fn' || action === 'abc' || action === '123') {
      keypadMode = action === '123' || keypadMode === action ? 'main' : action;
      shifted = false;
      drawKeypad();
      return;
    }
    if (action === 'shift') {
      shifted = !shifted;
      drawKeypad();
      return;
    }
    var mf = currentField();
    if (!mf) return;
    var arg = action.slice(2);
    switch (action[0]) {
      case 't': mf.typedText(arg); break;
      case 'c': mf.cmd(arg); break;
      case 'w': mf.write(arg); break;
      case 'k':
        // MathQuill's keystroke() skips the enter handler, so ↵ calls it
        // through the (vendored, pinned) controller to make the next row.
        if (arg === 'Enter') {
          mf.__controller.handle('enter');
        } else {
          mf.keystroke(arg);
        }
        break;
      case 'f': mf.typedText(arg); mf.typedText('('); break;
    }
    // A function is picked once, and a capital is typed once, as in Desmos.
    if (keypadMode === 'fn' && action !== 'fn') {
      keypadMode = 'main';
      drawKeypad();
    } else if (shifted && action[0] === 't') {
      shifted = false;
      drawKeypad();
    }
  }

  // Holding an arrow or backspace repeats it.
  var repeat = null;
  function stopRepeat() { clearTimeout(repeat); clearInterval(repeat); repeat = null; }
  keypad.addEventListener('mousedown', function (e) {
    e.preventDefault();
    var b = e.target.closest('button');
    if (!b) return;
    var action = b.dataset.key;
    pressKey(action);
    if (/^k:(Left|Right|Backspace)$/.test(action)) {
      stopRepeat();
      repeat = setTimeout(function () {
        repeat = setInterval(function () { pressKey(action); }, 60);
      }, 400);
    }
  });
  document.addEventListener('mouseup', stopRepeat);
  keysButton.addEventListener('mousedown', function (e) {
    e.preventDefault();
    showKeypad(!keypad.classList.contains('open'));
  });
  try { if (localStorage.getItem('keypad')) showKeypad(true); } catch (e) { /* storage is optional */ }

  // ---- Importing a Desmos graph -------------------------------------------

  // A desmos.com link, or the text Calc.getState() gives. The engine checks
  // it properly; this only decides whether a paste is meant as a graph.
  function looksLikeDesmos(text) {
    text = text.trim();
    return /^(https?:\/\/)?(www\.)?desmos\.com\/calculator\/\w+\S*$/.test(text) ||
      (text.charAt(0) === '{' && text.indexOf('"expressions"') >= 0);
  }

  var importBox = document.getElementById('import');
  var importText = importBox.querySelector('textarea');
  function openImport() {
    importText.value = '';
    importBox.classList.add('open');
    importText.focus();
  }
  function closeImport() {
    importBox.classList.remove('open');
    focusRow(Math.max(0, activeIndex()));
  }
  importBox.addEventListener('mousedown', function (e) {
    if (e.target === importBox) { e.preventDefault(); closeImport(); }
  });
  importBox.querySelector('.cancel').addEventListener('click', closeImport);
  importBox.querySelector('.go').addEventListener('click', function () {
    var text = importText.value;
    if (!text.trim()) return;
    closeImport();
    send({ type: 'paste', text: text });
  });

  // Export: the engine sends the graph as Desmos's Calc.setState takes it,
  // and the box shows it ready to copy into desmos.com's console.
  var exportBox = document.getElementById('export');
  var exportText = exportBox.querySelector('textarea');
  function closeExport() {
    exportBox.classList.remove('open');
    focusRow(Math.max(0, activeIndex()));
  }
  function showExport(reply) {
    exportText.value = 'Calc.setState(' + reply.state + ')';
    var site = reply.threeD ? 'desmos.com/3d' : 'desmos.com/calculator';
    exportBox.querySelector('.site').textContent = site;
    var list = exportBox.querySelector('.warnings');
    list.innerHTML = '';
    reply.warnings.forEach(function (w) {
      var li = document.createElement('li');
      li.textContent = w;
      list.appendChild(li);
    });
    exportBox.querySelector('.go').textContent = 'Copy';
    exportBox.classList.add('open');
    exportText.focus();
    exportText.select();
  }
  exportBox.addEventListener('mousedown', function (e) {
    if (e.target === exportBox) { e.preventDefault(); closeExport(); }
  });
  exportBox.querySelector('.cancel').addEventListener('click', closeExport);
  exportBox.querySelector('.go').addEventListener('click', function () {
    exportText.focus();
    exportText.select();
    var copied = false;
    try { copied = document.execCommand('copy'); } catch (err) { copied = false; }
    this.textContent = copied ? 'Copied' : 'Press Ctrl+C';
  });

  // Ctrl+V of a Desmos graph anywhere opens it, instead of typing it in.
  document.addEventListener('paste', function (e) {
    if (e.target === importText) return;
    var text = e.clipboardData && e.clipboardData.getData('text/plain');
    if (!text || !looksLikeDesmos(text)) return;
    e.preventDefault();
    e.stopPropagation();
    send({ type: 'paste', text: text });
  }, true);

  var toast = document.getElementById('toast');

  // ---- Keyboard shortcuts, as Desmos lists them under Ctrl+/ ---------------

  // Every function the calculator knows, with an example of each, for the
  // help box's Functions tab.
  var FUNCTION_REFERENCE = [
    ['GRAPHING', [
      ['y=x^2', 'Curve; x=y^2 works too'], ['x^2+y^2=25', 'Implicit equation'],
      ['y>x^2', 'Inequality, shaded'], ['r=2\\sin\\left(3\\theta\\right)', 'Polar curve'],
      ['\\left(\\cos t,\\sin t\\right)', 'Parametric curve'],
      ['y=x^2\\left\\{x>0\\right\\}', 'Restriction'],
      ['\\left\\{x<0:-x,x\\right\\}', 'Piecewise'],
      ['y_1\\sim mx_1+b', 'Regression: fits m and b'],
      ["y'=x-y", 'Slope field, with solution curves through the points under it (drag them)'],
      ["y''=-y", "Any order up to 6; set y'(x\u2080) under it"],
      ["\\left(y'\\right)^2+y^2=1", "Not solved for y': every branch is drawn"],
      ["x'=y,y'=-x", 'System: direction field and trajectories from t = 0'],
      ['a\\to a+1', 'Action (type ->)']]],
    ['TRIG (RADIANS)', [
      ['\\sin x,\\ \\cos x,\\ \\tan x', 'Also csc, sec, cot'],
      ['\\arcsin x,\\ \\sin^{-1}x', 'Inverses; also arccos, arctan, arccsc, arcsec, arccot'],
      ['\\arctan\\left(b,a\\right)', 'Angle of the point (a, b)'],
      ['\\sinh x,\\ \\operatorname{arcsinh}x', 'Hyperbolic: sinh to coth and their inverses']]],
    ['POWERS AND LOGS', [
      ['e^x,\\ \\exp\\left(x\\right)', 'Exponential'], ['\\ln x', 'Natural log'],
      ['\\log x', 'Log base 10'], ['\\log_2x', 'Log base 2'],
      ['\\sqrt{x},\\ \\sqrt[3]{x}', 'Square and nth roots (type sqrt, nthroot)'], ['n!', 'Factorial']]],
    ['NUMBERS', [
      ['\\left|x\\right|,\\ \\operatorname{abs}\\left(x\\right)', 'Absolute value'],
      ['\\operatorname{floor}\\left(x\\right)', 'Also ceil'],
      ['\\operatorname{round}\\left(x,2\\right)', 'Round, optionally to places'],
      ['\\operatorname{sign}\\left(x\\right)', 'Sign: -1, 0 or 1'],
      ['\\operatorname{mod}\\left(a,b\\right)', 'Remainder'],
      ['\\operatorname{gcd}\\left(a,b\\right)', 'Also lcm'],
      ['\\operatorname{nCr}\\left(5,2\\right)', 'Combinations; nPr for permutations']]],
    ['STATISTICS', [
      ['\\operatorname{mean}\\left(L\\right)', 'Also median'],
      ['\\min\\left(L\\right),\\ \\max\\left(L\\right)', 'Smallest and largest'],
      ['\\operatorname{total}\\left(L\\right)', 'Sum of a list'],
      ['\\operatorname{length}\\left(L\\right)', 'Number of elements']]],
    ['LISTS', [
      ['\\left[1,2,3\\right],\\ \\left[1...10\\right]', 'Lists and ranges'],
      ['L\\left[2\\right],\\ L\\left[2...4\\right]', 'Element (from 1) and slice'],
      ['L\\left[L>0\\right]', 'Filter: the elements where a condition holds'],
      ['\\left[n^2\\ \\operatorname{for}\\ n=\\left[1...5\\right]\\right]', 'Comprehension'],
      ['\\operatorname{sort}\\left(L\\right)', 'Sort; sort(L, keys) sorts by keys'],
      ['\\operatorname{join}\\left(A,B\\right)', 'Join lists'],
      ['\\operatorname{unique}\\left(L\\right)', 'Drop repeats']]],
    ['CALCULUS', [
      ['\\frac{d}{dx}\\sin\\left(x^2\\right)', 'Derivative (exact)'], ["f'\\left(x\\right)", "Also f''"],
      ['\\sum_{n=1}^{10}n^2', 'Sum (type sum); prod for products'],
      ['\\int_0^1x^2dx', 'Integral (type int)'],
      ['\\int_0^{\\infty}e^{-x}dx', 'Type infinity (or infty) for \u221e, as in improper integrals and bounds']]],
    ['POINTS AND GEOMETRY', [
      ['p=\\left(1,2\\right),\\ p.x', 'Point and its coordinates'],
      ['\\operatorname{distance}\\left(p,q\\right)', 'Distance between points'],
      ['\\operatorname{midpoint}\\left(p,q\\right)', 'Midpoint'],
      ['\\operatorname{polygon}\\left(\\left(0,0\\right),\\left(4,0\\right),\\left(2,3\\right)\\right)', 'Filled polygon']]],
    ['COMPLEX NUMBERS', [
      ['3+2i,\\ e^{i\\pi}', 'i is the imaginary unit'],
      ['\\operatorname{real}\\left(z\\right)', 'Also imag'],
      ['\\operatorname{conj}\\left(z\\right)', 'Conjugate'],
      ['\\operatorname{arg}\\left(z\\right)', 'Angle; |z| is the size'],
      ['z^2+1', 'A row in z colors the plane by its value (hold the circle for Grid)'],
      ['\\sin z,\\ \\ln z', 'Also cos, tan, sinh, cosh, tanh, log, exp, sqrt']]],
    ['MATRICES (+ THEN MATRIX, OR TYPE matrix)', [
      ['\\det\\left(A\\right)', 'Determinant'],
      ['A^{-1},\\ \\operatorname{inv}\\left(A\\right)', 'Inverse'],
      ['A^T,\\ \\operatorname{transpose}\\left(A\\right)', 'Transpose'],
      ['AB,\\ 2A,\\ A+B,\\ A^3', 'Products, sums and whole-number powers'],
      ['\\operatorname{rref}\\left(A\\right)', 'Reduced row echelon form; also rank'],
      ['\\operatorname{trace}\\left(A\\right)', 'Sum of the diagonal'],
      ['\\operatorname{eigenvalues}\\left(A\\right)', 'Eigenvalues, complex ones too'],
      ['\\operatorname{eigenvectors}\\left(A\\right)', 'Eigenvectors as columns, drawn as arrows'],
      ['A\\left(1,2\\right),\\ A\\left(\\cos t,\\sin t\\right)', 'A point or curve moved by A'],
      ['A\\left[2,1\\right]', 'Entry in row 2, column 1'],
      ['\\operatorname{identity}\\left(3\\right)', 'Identity matrix'],
      ['B=\\left[\\left[1,2\\right],\\left[3,4\\right]\\right]', 'A matrix written out, row by row']]],
    ['FRACTALS AND ITERATION', [
      ['\\operatorname{mandelbrot}\\left(\\right)', 'Mandelbrot set, sharp however far you zoom; mandelbrot(n) sets the iterations'],
      ['\\operatorname{julia}\\left(-0.8+0.156i\\right)', 'Julia set of z^2+c; julia(c, n) sets the iterations'],
      ['\\operatorname{cobweb}\\left(f,0.2,30\\right)', 'Cobweb of x \u2192 f(x) from 0.2, 30 steps; f(x) = \u2026 on its own row'],
      ['\\operatorname{bifurcation}\\left(f,0.5\\right)', 'Where x \u2192 f(r, x) settles, r along the x axis; f(r, x) = \u2026 on its own row']]]
  ];

  var shortcuts = document.getElementById('shortcuts');
  var functionsTab = shortcuts.querySelector('.tab.functions');
  function showHelpTab(name) {
    shortcuts.querySelectorAll('.tabs [data-tab]').forEach(function (b) {
      b.classList.toggle('on', b.dataset.tab === name);
    });
    shortcuts.querySelectorAll('.tab').forEach(function (t) { t.classList.toggle('on', t.classList.contains(name)); });
    if (name === 'functions' && !functionsTab.firstChild) {
      functionsTab.innerHTML = FUNCTION_REFERENCE.map(function (g) {
        return '<h3>' + g[0] + '</h3><table>' + g[1].map(function (f) {
          return '<tr><td><span class="ref-math"></span></td><td></td></tr>';
        }).join('') + '</table>';
      }).join('');
      var cells = functionsTab.querySelectorAll('tr');
      var i = 0;
      FUNCTION_REFERENCE.forEach(function (g) {
        g[1].forEach(function (f) {
          var tr = cells[i++];
          tr.children[1].textContent = f[1];
          var m = tr.querySelector('.ref-math');
          m.textContent = f[0];
          MQ.StaticMath(m);
        });
      });
    }
  }
  function openHelp(tab) {
    showHelpTab(tab || 'keys');
    shortcuts.classList.add('open');
  }
  shortcuts.addEventListener('mousedown', function (e) {
    var tab = e.target.closest('[data-tab]');
    if (tab) { e.preventDefault(); showHelpTab(tab.dataset.tab); return; }
    if (e.target === shortcuts || e.target.closest('.close')) {
      e.preventDefault();
      shortcuts.classList.remove('open');
    }
  });
  document.getElementById('help').addEventListener('mousedown', function (e) {
    e.preventDefault();
    if (shortcuts.classList.contains('open')) shortcuts.classList.remove('open'); else openHelp();
  });

  // The row (or table) the focus is in.
  function focusedRow() {
    var el = document.activeElement && document.activeElement.closest &&
      document.activeElement.closest('.row');
    return rows.filter(function (r) { return r.el === el; })[0] || rows[activeIndex()] || null;
  }

  // Typing " into an empty expression makes it a note, as in Desmos.
  function replaceWithNote(row) {
    var i = indexOf(row);
    row.el.parentNode.removeChild(row.el);
    rows.splice(i, 1);
    var note = addNote(i, '');
    renumber();
    note.text.focus();
    sendRows();
  }

  function replaceWithTable(row) {
    var i = indexOf(row);
    row.el.parentNode.removeChild(row.el);
    rows.splice(i, 1);
    var t = addTable(i);
    renumber();
    tableFocus(t, 0, 0);
    sendRows();
  }

  document.addEventListener('keydown', function (e) {
    if (shortcuts.classList.contains('open') && (e.key === 'Escape' || e.key === '/')) {
      e.preventDefault();
      shortcuts.classList.remove('open');
      return;
    }
    if (importBox.classList.contains('open')) {
      if (e.key === 'Escape') { e.preventDefault(); closeImport(); }
      return;
    }
    if (exportBox.classList.contains('open')) {
      if (e.key === 'Escape') { e.preventDefault(); closeExport(); }
      return;
    }
    var ctrl = e.ctrlKey || e.metaKey;
    var k = e.key.toLowerCase();
    var row = focusedRow();
    var at = row ? indexOf(row) + 1 : rows.length;
    var handled = true;
    if (ctrl && !e.altKey && !e.shiftKey && (k === '/' || k === '?')) {
      openHelp();
    } else if (ctrl && e.altKey && k === 'x') {
      addRow('', at).field.focus();
      sendRows();
    } else if (ctrl && e.altKey && k === 't') {
      tableFocus(addTable(at), 0, 0);
      renumber();
      sendRows();
    } else if (ctrl && e.altKey && k === 'o') {
      addNote(at, '').text.focus();
      sendRows();
    } else if (ctrl && e.altKey && k === 'e') {
      focusRow(Math.max(0, activeIndex()));
    } else if (ctrl && e.altKey && k === 's') {
      requestSolve(row, null);
    } else if (ctrl && e.altKey && k === 'k') {
      showKeypad(!keypad.classList.contains('open'));
    } else if (ctrl && e.shiftKey && !e.altKey && k === 'd') {
      if (row) removeRow(row);
    } else if (e.key === 'Escape' && !ctrl && !e.altKey && document.activeElement &&
        document.activeElement.closest && document.activeElement.closest('#list')) {
      document.activeElement.blur();
    } else {
      handled = false;
    }
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  }, true);

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
    if (b.dataset.add === 'ticker') {
      addTicker('', '0').field.focus();
      sendRows();
    } else if (b.dataset.add === 'note') {
      addNote(at, '').text.focus();
      sendRows();
    } else if (b.dataset.add === 'table') {
      var t = addTable(at);
      tableFocus(t, 0, 0);
      sendRows();
    } else if (b.dataset.add === 'matrix') {
      matrixFocus(addMatrix(at), 0, 0);
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
    if (b.dataset.file === 'import') openImport();
    else if (b.dataset.file === 'help') openHelp();
    else send({ type: b.dataset.file });
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
    ['color', 'hidden', 'style', 'width', 'opacity', 'fillOpacity', 'pointStyle', 'pointSize',
      'dots', 'connect', 'complex']
      .forEach(function (k) { if (saved[k] !== undefined && saved[k] !== null) target[k] = saved[k]; });
  }

  function loadTransform(row, saved) {
    if (typeof saved.transform !== 'number') return;
    row.transform = { on: true, t: Math.max(0, Math.min(1, saved.transform)) };
    paintTransform(row);
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
    removeTicker();
    (saved || []).filter(function (s) { return s.ticker; }).slice(0, 1).forEach(function (s) {
      addTicker(s.latex || '', s.ticker.step || '0');
    });
    (saved || []).filter(function (s) { return !s.ticker; }).forEach(function (s, i) {
      if (typeof s.note === 'string') {
        addNote(i, s.note);
        return;
      }
      if (s.matrix) {
        var m = addMatrix(i, s.latex || '', s.matrix);
        applyStyle(m, s);
        paintMatrix(m);
        loadTransform(m, s);
        return;
      }
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
      loadTransform(row, s);
      if (typeof s.seeds === 'string') row.seeds = s.seeds;
      if (Array.isArray(s.derivs)) row.derivs = s.derivs.map(String);
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

  // ---- 2D or 3D ----------------------------------------------------------------

  var modeButtons = document.querySelectorAll('#mode button');
  function showMode(threeD) {
    modeButtons.forEach(function (b) {
      b.classList.toggle('on', (b.dataset.mode === '3d') === threeD);
    });
  }
  modeButtons.forEach(function (b) {
    b.addEventListener('mousedown', function (e) { e.preventDefault(); });
    b.addEventListener('click', function () {
      showMode(b.dataset.mode === '3d');
      send({ type: 'mode', mode: b.dataset.mode });
    });
  });

  // Called by Rust through evaluate_script.
  window.engine = {
    // The graph became 2D or 3D, from the switch, a file or a new graph.
    mode: showMode,
    // While desmos.com is asked for a pasted link.
    fetching: function (on) {
      toast.textContent = 'Getting the graph from desmos.com\u2026';
      toast.classList.toggle('open', on);
    },
    // A graph opened from a file starts a fresh history.
    load: function (saved) {
      forgetHistory();
      loadRows(saved);
    },
    // The graph for Desmos, after Export to Desmos in the menu.
    exported: showExport,
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
      if (ticker) showTickerResult(byId[ticker.id]);
      rows.forEach(function (row) {
        var r = byId[row.id];
        if (!r || row.kind === 'note') return;
        if (row.kind === 'table') showTableResult(row, r);
        else if (row.kind === 'matrix') showMatrixResult(row, r);
        else showResult(row, r);
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
        if (!row || row.kind) return;
        row.playPos = null;
        setSlider(row, p[1], true);
      });
    },
    // The working for a row, after its Solve button was pressed.
    solved: function (r) {
      var row = rows.filter(function (x) { return x.id === r.id; })[0];
      if (row && !row.kind && row.solveLatex != null) showSteps(row, r);
    },
    // Dragging a differential equation's initial point on the graph
    // rewrites its initial points.
    setSeeds: function (id, latex) {
      var row = rows.filter(function (r) { return r.id === id; })[0];
      if (!row || row.kind || !row.seedsField) return;
      row.seeds = latex;
      row.settingSeeds = true;
      row.seedsField.latex(latex);
      row.settingSeeds = false;
      sendRows();
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
  // Rows first, so a graph the engine loads on 'ready' replaces them. History
  // starts from here, not from the rows being built.
  forgetHistory();
  sendRows();
  send({ type: 'ready' });
})();
