// Expression list: one MathQuill field per row, set up the way Desmos
// configures it, sending every edit to the Rust engine.
(function () {
  'use strict';

  var MQ = MathQuill.getInterface(2);
  var COLORS = ['#c74440', '#2d70b3', '#388c46', '#6042a6', '#fa7e19', '#000000'];
  var nextColor = 0;
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
    autoOperatorNames: 'sin cos tan sec csc cot arcsin arccos arctan sinh cosh tanh ' +
      'ln log exp min max floor ceil round sign mod mean median total length gcd lcm'
  };

  function send(message) {
    if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(JSON.stringify(message));
    }
  }

  function sendExpressions() {
    send({ type: 'exprs', latex: rows.map(function (r) { return r.field.latex(); }) });
  }

  function indexOf(row) {
    return rows.indexOf(row);
  }

  function renumber() {
    rows.forEach(function (r, i) { r.indexEl.textContent = String(i + 1); });
  }

  function refreshSwatch(row) {
    var empty = row.field.latex().trim() === '';
    row.swatch.classList.toggle('empty', empty);
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
    sendExpressions();
  }

  function addRow(latex, at) {
    var el = document.createElement('div');
    el.className = 'row';
    el.innerHTML =
      '<div class="tab"><span class="index"></span><span class="swatch"></span></div>' +
      '<div class="math"><span class="field"></span></div>' +
      '<button class="remove" title="Delete">&#x2715;</button>';
    var row = {
      el: el,
      indexEl: el.querySelector('.index'),
      swatch: el.querySelector('.swatch'),
      color: COLORS[nextColor++ % COLORS.length],
      field: null
    };
    row.swatch.style.background = row.color;

    var options = Object.assign({}, MATH_OPTIONS, {
      handlers: {
        edit: function () { refreshSwatch(row); sendExpressions(); },
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
    if (latex) row.field.latex(latex);

    el.addEventListener('focusin', function () { setActive(row); });
    el.querySelector('.math').addEventListener('mousedown', function (e) {
      if (e.target === e.currentTarget) { e.preventDefault(); row.field.focus(); }
    });
    el.querySelector('.remove').addEventListener('click', function () { removeRow(row); });

    refreshSwatch(row);
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

  // Called by Rust through evaluate_script.
  window.engine = {
    info: function (adapter) {
      document.getElementById('engine').textContent = 'Graph engine: ' + adapter;
    },
    stats: function (sceneMs, frameMs, vertices) {
      document.getElementById('frame').textContent =
        'Last frame: ' + frameMs.toFixed(2) + ' ms (scene ' + sceneMs.toFixed(2) + ' ms, ' +
        vertices.toLocaleString() + ' vertices)';
    }
  };

  addRow('y=\\sin\\left(x\\right)', 0);
  addRow('', 1);
  focusRow(0);
  send({ type: 'ready' });
  sendExpressions();
})();
