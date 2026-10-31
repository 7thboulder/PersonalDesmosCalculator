// Typing shortcuts: a trigger typed in a math field (such as ;pd) becomes a
// LaTeX template. Empty boxes in a template are slots: the cursor goes to the
// first one, and Tab moves to the next. They work in documents, and in the
// graphing calculator too when a shortcut says so. The list is the person's
// own, edited in the Shortcuts menu and saved by the engine.
(function () {
  'use strict';

  var MQ = MathQuill.getInterface(2);
  var SLOT = '\\embed{slot}';

  // A slot is an empty box that disappears when the cursor reaches it.
  MQ.registerEmbed('slot', function () {
    return {
      htmlString: '<span class="mq-slot">&#9633;</span>',
      text: function () { return ''; },
      latex: function () { return SLOT; }
    };
  });

  var BUILT_IN = [
    { trigger: ';pd', latex: '\\frac{\\partial ' + SLOT + '}{\\partial ' + SLOT + '}', note: 'Partial derivative' },
    { trigger: ';dd', latex: '\\frac{d' + SLOT + '}{d' + SLOT + '}', note: 'Derivative' },
    { trigger: ';dx', latex: '\\frac{d}{dx}', note: 'd/dx' },
    { trigger: ';lim', latex: '\\lim_{' + SLOT + '\\to ' + SLOT + '}', note: 'Limit' },
    { trigger: 'vv', latex: '\\vec{' + SLOT + '}', note: 'Vector arrow' },
    { trigger: ';cb', latex: '\\sqrt[3]{' + SLOT + '}', note: 'Cube root' },
    { trigger: ';nck', latex: '\\binom{' + SLOT + '}{' + SLOT + '}', note: 'Binomial coefficient' },
    { trigger: ';pm', latex: '\\pm ', note: 'Plus or minus' },
    { trigger: ';oo', latex: '\\infty ', note: 'Infinity' },
    { trigger: ';tf', latex: '\\therefore ', note: 'Therefore' },
    { trigger: ';=>', latex: '\\Rightarrow ', note: 'Implies' },
    { trigger: ';<=>', latex: '\\Leftrightarrow ', note: 'If and only if' }
  ].map(function (s) {
    return { trigger: s.trigger, latex: s.latex, note: s.note, where: 'doc', on: true, builtin: true, base: s.trigger };
  });

  function copy(list) { return JSON.parse(JSON.stringify(list)); }

  var list = copy(BUILT_IN);
  var listeners = [];

  // The saved list replaces the built-in one, but built-ins it doesn't
  // mention yet (added in a later version) still appear.
  function setList(saved) {
    if (!Array.isArray(saved)) { list = copy(BUILT_IN); return; }
    list = saved.filter(function (s) { return s && typeof s.trigger === 'string' && typeof s.latex === 'string'; })
      .map(function (s) {
        return { trigger: s.trigger, latex: s.latex, note: s.note || '', where: s.where === 'both' ? 'both' : 'doc',
          on: s.on !== false, builtin: !!s.builtin, base: s.base || (s.builtin ? s.trigger : '') };
      });
    BUILT_IN.forEach(function (b) {
      if (!list.some(function (s) { return s.builtin && s.base === b.base; })) list.push(copy(b));
    });
  }

  function save() {
    if (window.panel) window.panel.send({ type: 'shortcutsSave', list: list });
    listeners.forEach(function (f) { f(); });
  }

  // The words MathQuill turns into symbols as soon as they are typed. A
  // trigger that contains one before its last letter never gets typed whole.
  function autoWords() {
    var opts = (window.panel && window.panel.mathOptions) || {};
    return String(opts.autoCommands || '').split(/\s+/).filter(Boolean);
  }

  // Problems with a trigger, for the menu to show.
  function warnings(trigger, ignore) {
    var out = [];
    if (!trigger) return ['Type a trigger.'];
    if (/\s/.test(trigger)) out.push('A trigger cannot contain spaces.');
    if (trigger.length < 2) out.push('Use at least two characters, or it fires all the time.');
    autoWords().forEach(function (w) {
      var at = trigger.indexOf(w);
      if (at >= 0 && at + w.length < trigger.length) {
        out.push('"' + w + '" turns into a symbol before the trigger is finished.');
      }
    });
    if (/^[A-Za-z]+$/.test(trigger)) {
      out.push('Letters only: it also fires inside words that end in "' + trigger + '".');
    }
    list.forEach(function (s) {
      if (s === ignore || !s.on) return;
      if (s.trigger === trigger) out.push('Another shortcut already uses "' + trigger + '".');
      else if (trigger.length > s.trigger.length && trigger.indexOf(s.trigger) === 0) {
        out.push('"' + s.trigger + '" fires first, so this one never will.');
      }
    });
    return out;
  }

  // The characters just before the cursor, as typed (letters, digits and
  // symbols at the same level).
  function typedBefore(cursor) {
    var out = '';
    for (var n = cursor.previousElementSibling; n && out.length < 16; n = n.previousElementSibling) {
      if (n.classList.contains('mq-non-leaf') || n.children.length) break;
      var t = n.textContent.replace(/ /g, ' ').trim();
      if (t.length !== 1) break;
      // MathQuill writes some typed characters as other symbols.
      t = { '−': '-', '·': '*', '≤': '<=', '≥': '>=' }[t] || t;
      out = t + out;
    }
    return out;
  }

  function fieldIn(target) {
    var root = target && target.closest && target.closest('.mq-editable-field');
    if (!root || root.closest('.no-shortcuts')) return null;
    var inDoc = !!root.closest('#doc');
    var inPanel = !!root.closest('#list');
    if (!inDoc && !inPanel) return null;
    return { root: root, inDoc: inDoc };
  }

  // Moves the cursor onto a slot and removes it, so typing fills it. The
  // cursor is put there as a click would, since arrow keys never visit some
  // blocks (a fraction's denominator, for one).
  function gotoSlot(field, root, slot) {
    var r = slot.getBoundingClientRect();
    field.clickAt(r.left + 1, r.top + r.height / 2, slot);
    var cursor = root.querySelector('.mq-cursor');
    if (cursor && cursor.nextElementSibling === slot) field.keystroke('Del');
    else if (cursor && cursor.previousElementSibling === slot) field.keystroke('Backspace');
    else return false;
    return true;
  }

  function slotsIn(root) {
    return Array.prototype.slice.call(root.querySelectorAll('.mq-slot'));
  }

  function expand(field, root, latex) {
    field.write(latex);
    var slots = slotsIn(root);
    if (!slots.length) return;
    // The template's own first slot: the first one after where it began.
    gotoSlot(field, root, slots[0]);
  }

  // The next slot after the cursor, or the first one.
  function nextSlot(root) {
    var cursor = root.querySelector('.mq-cursor');
    var slots = slotsIn(root);
    if (!slots.length) return null;
    if (!cursor) return slots[0];
    for (var i = 0; i < slots.length; i++) {
      if (cursor.compareDocumentPosition(slots[i]) & Node.DOCUMENT_POSITION_FOLLOWING) return slots[i];
    }
    return slots[0];
  }

  document.addEventListener('keydown', function (e) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    var where = fieldIn(e.target);
    if (!where) return;
    var field = MQ(where.root);
    if (!field) return;
    if (e.key === 'Tab' && !e.shiftKey) {
      var slot = nextSlot(where.root);
      if (slot) {
        e.preventDefault();
        e.stopPropagation();
        gotoSlot(field, where.root, slot);
      }
      return;
    }
    if (e.key.length !== 1) return;
    var cursor = where.root.querySelector('.mq-cursor');
    if (!cursor) return;
    var typed = typedBefore(cursor) + e.key;
    var best = null;
    list.forEach(function (s) {
      if (!s.on || (!where.inDoc && s.where !== 'both')) return;
      if (typed.length >= s.trigger.length && typed.slice(-s.trigger.length) === s.trigger &&
          (!best || s.trigger.length > best.trigger.length)) best = s;
    });
    if (!best) return;
    e.preventDefault();
    e.stopPropagation();
    // The characters already typed are deleted one keystroke at a time (<=
    // counts as one symbol, ≤).
    var removing = best.trigger.slice(0, -1);
    var symbols = 0;
    var n = cursor.previousElementSibling;
    var got = '';
    while (n && got.length < removing.length) {
      var t = n.textContent.trim();
      got = ({ '−': '-', '·': '*', '≤': '<=', '≥': '>=' }[t] || t) + got;
      symbols++;
      n = n.previousElementSibling;
    }
    for (var k = 0; k < symbols; k++) field.keystroke('Backspace');
    expand(field, where.root, best.latex);
  }, true);

  window.shortcuts = {
    SLOT: SLOT,
    builtIn: function () { return copy(BUILT_IN); },
    list: function () { return list; },
    set: function (saved) { setList(saved); listeners.forEach(function (f) { f(); }); },
    replace: function (next) { list = next; save(); },
    warnings: warnings,
    expand: expand,
    onChange: function (f) { listeners.push(f); }
  };
})();
