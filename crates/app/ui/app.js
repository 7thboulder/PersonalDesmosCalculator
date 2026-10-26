// Expression list: one MathQuill field per row, set up the way Desmos
// configures it. Every change sends all rows to the Rust engine, which
// replies with what each row means (curve, value, slider, error).
(function () {
  'use strict';

  var MQ = MathQuill.getInterface(2);
  var COLORS = ['#c74440', '#2d70b3', '#388c46', '#6042a6', '#fa7e19', '#000000'];
  var DEFAULT_WIDTH = 2.5;
  // Seconds for a playing slider to cross its whole range once.
  var PLAY_SECONDS = 8;
  var LONG_PRESS_MS = 450;
  var nextColor = 0;
  var nextId = 1;
  var rows = [];
  var list = document.getElementById('list');

  // Options matching how Desmos's editor behaves when typing.
  var MATH_OPTIONS = {
    spaceBehavesLikeTab: true,
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
      'gcd lcm nCr nPr'
  };

  function send(message) {
    if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(JSON.stringify(message));
    }
  }

  function sendRows() {
    send({
      type: 'rows',
      rows: rows.map(function (r) {
        return {
          id: r.id, latex: r.field.latex(), color: r.color, hidden: r.hidden,
          style: r.style, width: r.width, opacity: r.opacity
        };
      })
    });
  }

  function indexOf(row) { return rows.indexOf(row); }

  function renumber() {
    rows.forEach(function (r, i) { r.indexEl.textContent = String(i + 1); });
  }

  function setActive(row) {
    rows.forEach(function (r) { r.el.classList.toggle('active', r === row); });
  }

  function focusRow(i, atEnd) {
    var row = rows[Math.max(0, Math.min(rows.length - 1, i))];
    if (!row) return;
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

  function setSlider(row, v) {
    var latex = row.field.latex();
    var eq = latex.indexOf('=');
    if (eq < 0) return;
    // Keep the left side exactly as typed, and replace only the number.
    row.settingSlider = true;
    row.field.latex(latex.slice(0, eq) + '=' + sliderValue(row, v));
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
      paintRow(menuRow);
      refreshMenu();
      sendRows();
    });
    menu.querySelector('.width').addEventListener('input', function (e) {
      if (!menuRow) return;
      menuRow.width = Number(e.target.value);
      sendRows();
    });
    menu.querySelector('.opacity').addEventListener('input', function (e) {
      if (!menuRow) return;
      menuRow.opacity = Number(e.target.value);
      paintRow(menuRow);
      sendRows();
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
    menu.querySelector('.width').value = String(menuRow.width);
    menu.querySelector('.opacity').value = String(menuRow.opacity);
  }

  function openMenu(row) {
    menuRow = row;
    var box = row.swatch.getBoundingClientRect();
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
      color: COLORS[nextColor++ % COLORS.length],
      hidden: false,
      style: 'solid',
      width: DEFAULT_WIDTH,
      opacity: 1,
      slider: { min: -10, max: 10, step: null },
      playing: false,
      settingSlider: false,
      playDir: 1,
      playPos: null,
      result: null,
      resultKey: '',
      field: null
    };

    var options = Object.assign({}, MATH_OPTIONS, {
      handlers: {
        edit: function () { if (!row.settingSlider) row.playPos = null; sendRows(); },
        enter: function () { addRow('', indexOf(row) + 1).field.focus(); },
        upOutOf: function () { focusRow(indexOf(row) - 1); },
        downOutOf: function () {
          var i = indexOf(row);
          if (i === rows.length - 1) addRow('', i + 1).field.focus(); else focusRow(i + 1);
        },
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
    if (latex) row.field.latex(latex);

    el.addEventListener('focusin', function (e) {
      if (!e.target.closest('.limits')) setActive(row);
    });
    el.querySelector('.math').addEventListener('mousedown', function (e) {
      if (e.target === e.currentTarget) { e.preventDefault(); row.field.focus(); }
    });
    el.querySelector('.remove').addEventListener('click', function () { removeRow(row); });

    // Click the color circle to hide or show; hold it (or right-click) for
    // the style menu, as in Desmos.
    var pressTimer = null, longPressed = false;
    row.swatch.addEventListener('mousedown', function (e) {
      if (e.button !== 0) return;
      e.stopPropagation();
      longPressed = false;
      pressTimer = setTimeout(function () { longPressed = true; openMenu(row); }, LONG_PRESS_MS);
    });
    row.swatch.addEventListener('mouseup', function () { clearTimeout(pressTimer); });
    row.swatch.addEventListener('mouseleave', function () { clearTimeout(pressTimer); });
    row.swatch.addEventListener('click', function () {
      if (longPressed) return;
      row.hidden = !row.hidden;
      paintRow(row);
      sendRows();
    });
    row.swatch.addEventListener('contextmenu', function (e) {
      e.preventDefault();
      openMenu(row);
    });

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

  document.getElementById('add').addEventListener('click', function () {
    addRow('', rows.length).field.focus();
  });
  list.addEventListener('mousedown', function (e) {
    // Clicking the empty space under the rows adds a row, like Desmos.
    if (e.target === list) { e.preventDefault(); addRow('', rows.length).field.focus(); }
  });
  buildMenu();

  // Called by Rust through evaluate_script.
  window.engine = {
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
      rows.forEach(function (row) { if (byId[row.id]) showResult(row, byId[row.id]); });
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
  send({ type: 'ready' });
  sendRows();
})();
