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
    { trigger: ';lim+', latex: '\\lim_{' + SLOT + '\\to ' + SLOT + '^+}', note: 'Limit from above (the right)', where: 'both' },
    { trigger: ';lim-', latex: '\\lim_{' + SLOT + '\\to ' + SLOT + '^-}', note: 'Limit from below (the left)', where: 'both' },
    { trigger: 'vv', latex: '\\vec{' + SLOT + '}', note: 'Vector arrow' },
    { trigger: ';cb', latex: '\\sqrt[3]{' + SLOT + '}', note: 'Cube root' },
    { trigger: ';nck', latex: '\\binom{' + SLOT + '}{' + SLOT + '}', note: 'Binomial coefficient' },
    { trigger: ';pm', latex: '\\pm ', note: 'Plus or minus' },
    { trigger: ';oo', latex: '\\infty ', note: 'Infinity' },
    { trigger: ';tf', latex: '\\therefore ', note: 'Therefore' },
    { trigger: ';=>', latex: '\\Rightarrow ', note: 'Implies' },
    { trigger: ';<=>', latex: '\\Leftrightarrow ', note: 'If and only if' }
  ].map(function (s) {
    return { trigger: s.trigger, latex: s.latex, note: s.note, where: s.where || 'doc', on: true, builtin: true, base: s.trigger };
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
        out.push('"' + s.trigger + '" waits a moment for the rest of this one, so it fires a little later.');
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

  // Deletes `text`, typed just before the cursor, a keystroke at a time (<=
  // counts as one symbol, ≤).
  function removeTyped(field, cursor, text) {
    var symbols = 0;
    var n = cursor.previousElementSibling;
    var got = '';
    while (n && got.length < text.length) {
      var t = n.textContent.trim();
      got = ({ '−': '-', '·': '*', '≤': '<=', '≥': '>=' }[t] || t) + got;
      symbols++;
      n = n.previousElementSibling;
    }
    for (var k = 0; k < symbols; k++) field.keystroke('Backspace');
  }

  function applies(s, where) { return s.on && (where.inDoc || s.where === 'both'); }

  // A trigger that begins a longer one (;lim and ;lim+) is typed out and
  // waits: the next key either finishes the longer one or fires it.
  var pending = null;
  var PENDING_MS = 800;

  // Fires the waiting trigger, if it is still just before the cursor.
  function firePending() {
    var p = pending;
    pending = null;
    if (!p) return false;
    clearTimeout(p.timer);
    var cursor = p.root.querySelector('.mq-cursor');
    if (!cursor || !document.body.contains(p.root)) return false;
    var typed = typedBefore(cursor);
    if (typed.slice(-p.s.trigger.length) !== p.s.trigger) return false;
    removeTyped(p.field, cursor, p.s.trigger);
    expand(p.field, p.root, p.s.latex);
    return true;
  }

  document.addEventListener('keydown', function (e) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    var where = fieldIn(e.target);
    if (!where) return;
    var field = MQ(where.root);
    if (!field) return;
    if (pending && (pending.root !== where.root || e.key.length !== 1)) {
      // Editing or moving away leaves the trigger as typed.
      var cancel = pending.root !== where.root ||
        /^(Backspace|Delete|Escape|Arrow|Home|End)/.test(e.key);
      var fired = !cancel && firePending();
      if (cancel) clearTimeout(pending.timer);
      pending = null;
      // Tab after a trigger goes to its first blank, where firing put the cursor.
      if (fired && e.key === 'Tab') {
        e.preventDefault();
        e.stopPropagation();
        return;
      }
    }
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
      if (!applies(s, where)) return;
      if (typed.length >= s.trigger.length && typed.slice(-s.trigger.length) === s.trigger &&
          (!best || s.trigger.length > best.trigger.length)) best = s;
    });
    if (pending) {
      clearTimeout(pending.timer);
      var waiting = pending;
      pending = null;
      // The key doesn't finish a longer trigger: the waiting one fires, and
      // the key is typed into its first blank.
      if (!best || best.trigger.length <= waiting.s.trigger.length) {
        pending = waiting;
        firePending();
        return;
      }
    }
    if (!best) return;
    var longer = list.some(function (s) {
      return applies(s, where) && s.trigger.length > best.trigger.length && s.trigger.indexOf(best.trigger) === 0;
    });
    if (longer) {
      // Typed as usual for now; fired by the next key or after a moment.
      pending = { s: best, root: where.root, field: field, timer: setTimeout(firePending, PENDING_MS) };
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    removeTyped(field, cursor, best.trigger.slice(0, -1));
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
