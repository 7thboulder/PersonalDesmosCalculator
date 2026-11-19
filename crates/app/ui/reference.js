// The reference panel: trig identities, derivatives and integrals, and the
// definitions of the special functions the app knows. It opens beside the
// expression list (or on the right of a document) with the book button or
// Ctrl+Alt+R. Clicking one side of a formula types that side at the cursor
// of the last math field; the + button types the whole formula.
(function () {
  'use strict';

  var MQ = MathQuill.getInterface(2);

  // ---- LaTeX helpers ---------------------------------------------------------

  function P(x) { return '\\left(' + x + '\\right)'; }
  function B(x) { return '\\left[' + x + '\\right]'; }
  function A(x) { return '\\left|' + x + '\\right|'; }
  function F(a, b) { return '\\frac{' + a + '}{' + b + '}'; }
  function R(x) { return '\\sqrt{' + x + '}'; }
  // Operator names LaTeX knows are written \sin; the rest \operatorname{sech}.
  var BUILT_IN = ' sin cos tan sec csc cot arcsin arccos arctan sinh cosh tanh coth ln log exp ';
  function op(name) {
    return BUILT_IN.indexOf(' ' + name + ' ') >= 0 ? '\\' + name + ' ' : '\\operatorname{' + name + '}';
  }
  // f(x) with the argument in brackets, f x when it is one letter.
  function fn(name, arg) {
    return /^\\?[a-zA-Z]+$/.test(arg) && !/^[a-zA-Z]{2,}$/.test(arg) ? op(name) + arg : op(name) + P(arg);
  }
  var t = '\\theta';
  var al = '\\alpha', be = '\\beta', ga = '\\gamma';
  function sin(x) { return fn('sin', x); }
  function cos(x) { return fn('cos', x); }
  function tan(x) { return fn('tan', x); }
  function sec(x) { return fn('sec', x); }
  function csc(x) { return fn('csc', x); }
  function cot(x) { return fn('cot', x); }
  function ln(x) { return fn('ln', x); }
  function sq(f, x) { return op(f) + '^2' + x; }
  function pw(f, n, x) { return op(f) + '^{' + n + '}' + x; }
  function dx(x) { return '\\frac{d}{dx}' + x; }
  function I(body, d) { return '\\int ' + body + '\\ d' + (d || 'x'); }
  function DI(lo, hi, body, d) { return '\\int_{' + lo + '}^{' + hi + '}' + body + '\\ d' + (d || 't'); }
  // Antiderivatives leave off + C, so a side can be typed into a row as it is.
  var C = '';
  var Gam = function (x) { return '\\Gamma' + P(x); };
  var psi = function (x) { return '\\psi' + P(x); };
  var zeta = function (x) { return '\\zeta' + P(x); };
  var erf = function (x) { return op('erf') + P(x); };
  var half = F('1', '2');
  var inf = '\\infty';

  // ---- The reference ---------------------------------------------------------
  //
  // A section has groups; a group has a heading and items. An item is
  //   { m: [side, relation, side, ...], n: note }  a formula, each side clickable
  //     (ex: the same thing written out, shown under it but never inserted)
  //   { table: { cols: [...], rows: [[...]] } }     a table; '#text' cells are text
  // A group can also say how to type a function (type) and where it is
  // defined (domain), and add search words (k).

  var TRIG = [
    { h: 'Right triangle', n: 'For 0 < θ < π/2', items: [
      { m: [sin(t), '=', F('\\text{opposite}', '\\text{hypotenuse}')] },
      { m: [cos(t), '=', F('\\text{adjacent}', '\\text{hypotenuse}')] },
      { m: [tan(t), '=', F('\\text{opposite}', '\\text{adjacent}')] },
      { m: [csc(t), '=', F('\\text{hypotenuse}', '\\text{opposite}')] },
      { m: [sec(t), '=', F('\\text{hypotenuse}', '\\text{adjacent}')] },
      { m: [cot(t), '=', F('\\text{adjacent}', '\\text{opposite}')] }] },
    { h: 'Unit circle', n: 'For any angle θ, where (x, y) is the point at angle θ on the unit circle', items: [
      { m: [sin(t), '=', 'y'] }, { m: [cos(t), '=', 'x'] }, { m: [tan(t), '=', F('y', 'x')] },
      { m: [csc(t), '=', F('1', 'y')] }, { m: [sec(t), '=', F('1', 'x')] }, { m: [cot(t), '=', F('x', 'y')] }] },
    { h: 'Domain', n: 'n is any integer', items: [
      { m: [sin(t) + ',\\ ' + cos(t)], n: 'Defined for every θ' },
      { m: [t, '\\ne', P('n+' + half) + '\\pi'], n: 'Domain of tan θ and sec θ' },
      { m: [t, '\\ne', 'n\\pi'], n: 'Domain of csc θ and cot θ' }] },
    { h: 'Range', items: [
      { m: ['-1', '\\le', sin(t), '\\le', '1'] },
      { m: ['-1', '\\le', cos(t), '\\le', '1'] },
      { m: ['-' + inf, '<', tan(t), '<', inf], n: 'Same for cot θ' },
      { m: [sec(t), '\\ge', '1'], n: 'or sec θ ≤ −1' },
      { m: [csc(t), '\\ge', '1'], n: 'or csc θ ≤ −1' }] },
    { h: 'Period', k: 'periodic omega', items: [
      { m: ['T', '=', F('2\\pi', '\\omega')], n: 'Period of sin(ωθ), cos(ωθ), csc(ωθ) and sec(ωθ)' },
      { m: ['T', '=', F('\\pi', '\\omega')], n: 'Period of tan(ωθ) and cot(ωθ)' }] },
    { h: 'Degrees and radians', k: 'convert conversion', n: 'x in degrees, t in radians', items: [
      { m: [F('\\pi', '180'), '=', F('t', 'x')] },
      { m: ['t', '=', F('\\pi x', '180')] },
      { m: ['x', '=', F('180t', '\\pi')] }] }
  ];

  var IDENTITIES = [
    { h: 'Tangent and cotangent', k: 'quotient', items: [
      { m: [tan(t), '=', F(sin(t), cos(t))] },
      { m: [cot(t), '=', F(cos(t), sin(t))] }] },
    { h: 'Reciprocal', items: [
      { m: [csc(t), '=', F('1', sin(t))] }, { m: [sin(t), '=', F('1', csc(t))] },
      { m: [sec(t), '=', F('1', cos(t))] }, { m: [cos(t), '=', F('1', sec(t))] },
      { m: [cot(t), '=', F('1', tan(t))] }, { m: [tan(t), '=', F('1', cot(t))] }] },
    { h: 'Pythagorean', items: [
      { m: [sq('sin', t) + '+' + sq('cos', t), '=', '1'] },
      { m: [sq('tan', t) + '+1', '=', sq('sec', t)] },
      { m: ['1+' + sq('cot', t), '=', sq('csc', t)] }] },
    { h: 'Even and odd', k: 'negative', items: [
      { m: [sin('-' + t), '=', '-' + sin(t)] }, { m: [csc('-' + t), '=', '-' + csc(t)] },
      { m: [cos('-' + t), '=', cos(t)] }, { m: [sec('-' + t), '=', sec(t)] },
      { m: [tan('-' + t), '=', '-' + tan(t)] }, { m: [cot('-' + t), '=', '-' + cot(t)] }] },
    { h: 'Periodic', n: 'n is any integer', items: [
      { m: [sin(t + '+2\\pi n'), '=', sin(t)] }, { m: [csc(t + '+2\\pi n'), '=', csc(t)] },
      { m: [cos(t + '+2\\pi n'), '=', cos(t)] }, { m: [sec(t + '+2\\pi n'), '=', sec(t)] },
      { m: [tan(t + '+\\pi n'), '=', tan(t)] }, { m: [cot(t + '+\\pi n'), '=', cot(t)] }] },
    { h: 'Cofunction', items: [
      { m: [sin(F('\\pi', '2') + '-' + t), '=', cos(t)] }, { m: [cos(F('\\pi', '2') + '-' + t), '=', sin(t)] },
      { m: [csc(F('\\pi', '2') + '-' + t), '=', sec(t)] }, { m: [sec(F('\\pi', '2') + '-' + t), '=', csc(t)] },
      { m: [tan(F('\\pi', '2') + '-' + t), '=', cot(t)] }, { m: [cot(F('\\pi', '2') + '-' + t), '=', tan(t)] }] },
    { h: 'Sum and difference', k: 'addition angle', items: [
      { m: [sin(al + '\\pm ' + be), '=', sin(al) + cos(be) + '\\pm ' + cos(al) + sin(be)] },
      { m: [cos(al + '\\pm ' + be), '=', cos(al) + cos(be) + '\\mp ' + sin(al) + sin(be)] },
      { m: [tan(al + '\\pm ' + be), '=', F(tan(al) + '\\pm ' + tan(be), '1\\mp ' + tan(al) + tan(be))] }] },
    { h: 'Double angle', items: [
      { m: [sin('2' + t), '=', '2' + sin(t) + cos(t)] },
      { m: [cos('2' + t), '=', sq('cos', t) + '-' + sq('sin', t), '=', '2' + sq('cos', t) + '-1', '=', '1-2' + sq('sin', t)] },
      { m: [tan('2' + t), '=', F('2' + tan(t), '1-' + sq('tan', t))] }] },
    { h: 'Triple angle', items: [
      { m: [sin('3' + t), '=', '3' + sin(t) + '-4' + pw('sin', 3, t)] },
      { m: [cos('3' + t), '=', '4' + pw('cos', 3, t) + '-3' + cos(t)] },
      { m: [tan('3' + t), '=', F('3' + tan(t) + '-' + pw('tan', 3, t), '1-3' + sq('tan', t))] }] },
    { h: 'Half angle', n: 'The sign is the sign of the left side', items: [
      { m: [sin(F(t, '2')), '=', '\\pm ' + R(F('1-' + cos(t), '2'))] },
      { m: [cos(F(t, '2')), '=', '\\pm ' + R(F('1+' + cos(t), '2'))] },
      { m: [tan(F(t, '2')), '=', '\\pm ' + R(F('1-' + cos(t), '1+' + cos(t)))] },
      { m: [tan(F(t, '2')), '=', F('1-' + cos(t), sin(t)), '=', F(sin(t), '1+' + cos(t))] }] },
    { h: 'Power reducing', k: 'half angle alternate squared', items: [
      { m: [sq('sin', t), '=', F('1-' + cos('2' + t), '2')] },
      { m: [sq('cos', t), '=', F('1+' + cos('2' + t), '2')] },
      { m: [sq('tan', t), '=', F('1-' + cos('2' + t), '1+' + cos('2' + t))] },
      { m: [pw('sin', 3, t), '=', F('3' + sin(t) + '-' + sin('3' + t), '4')] },
      { m: [pw('cos', 3, t), '=', F('3' + cos(t) + '+' + cos('3' + t), '4')] },
      { m: [pw('sin', 4, t), '=', F('3-4' + cos('2' + t) + '+' + cos('4' + t), '8')] },
      { m: [pw('cos', 4, t), '=', F('3+4' + cos('2' + t) + '+' + cos('4' + t), '8')] },
      { m: [sq('sin', t) + sq('cos', t), '=', F('1-' + cos('4' + t), '8')] }] },
    { h: 'Product to sum', items: [
      { m: [sin(al) + sin(be), '=', half + B(cos(al + '-' + be) + '-' + cos(al + '+' + be))] },
      { m: [cos(al) + cos(be), '=', half + B(cos(al + '-' + be) + '+' + cos(al + '+' + be))] },
      { m: [sin(al) + cos(be), '=', half + B(sin(al + '+' + be) + '+' + sin(al + '-' + be))] },
      { m: [cos(al) + sin(be), '=', half + B(sin(al + '+' + be) + '-' + sin(al + '-' + be))] }] },
    { h: 'Sum to product', items: [
      { m: [sin(al) + '+' + sin(be), '=', '2' + sin(F(al + '+' + be, '2')) + cos(F(al + '-' + be, '2'))] },
      { m: [sin(al) + '-' + sin(be), '=', '2' + cos(F(al + '+' + be, '2')) + sin(F(al + '-' + be, '2'))] },
      { m: [cos(al) + '+' + cos(be), '=', '2' + cos(F(al + '+' + be, '2')) + cos(F(al + '-' + be, '2'))] },
      { m: [cos(al) + '-' + cos(be), '=', '-2' + sin(F(al + '+' + be, '2')) + sin(F(al + '-' + be, '2'))] }] },
    { h: 'Combining a sine and a cosine', k: 'harmonic addition amplitude phase', n: 'arctan(b, a) is the angle of the point (a, b)', items: [
      { m: ['a' + sin(t) + '+b' + cos(t), '=', R('a^2+b^2') + sin(t + '+\\varphi')] },
      { m: ['\\varphi', '=', op('arctan') + P('b,a')] }] },
    { h: 'Tangent half-angle substitution', k: 'weierstrass t substitution', n: 't = tan(θ/2), for integrals of rational functions of sin and cos', items: [
      { m: [sin(t), '=', F('2t', '1+t^2')] },
      { m: [cos(t), '=', F('1-t^2', '1+t^2')] },
      { m: [tan(t), '=', F('2t', '1-t^2')] },
      { m: ['d' + t, '=', F('2', '1+t^2') + 'dt'] }] },
    { h: 'Euler’s formula', k: 'complex exponential de moivre', items: [
      { m: ['e^{i' + t + '}', '=', cos(t) + '+i' + sin(t)] },
      { m: ['e^{i\\pi}+1', '=', '0'] },
      { m: [cos(t), '=', F('e^{i' + t + '}+e^{-i' + t + '}', '2')] },
      { m: [sin(t), '=', F('e^{i' + t + '}-e^{-i' + t + '}', '2i')] },
      { m: [P(cos(t) + '+i' + sin(t)) + '^n', '=', cos('n' + t) + '+i' + sin('n' + t)], n: 'De Moivre’s formula' }] }
  ];

  var aSin = function (x) { return fn('arcsin', x); };
  var aCos = function (x) { return fn('arccos', x); };
  var aTan = function (x) { return fn('arctan', x); };
  var INVERSE = [
    { h: 'Definitions', items: [
      { m: ['y=' + aSin('x'), '\\Longleftrightarrow', 'x=' + sin('y')] },
      { m: ['y=' + aCos('x'), '\\Longleftrightarrow', 'x=' + cos('y')] },
      { m: ['y=' + aTan('x'), '\\Longleftrightarrow', 'x=' + tan('y')] }] },
    { h: 'Domain and range', k: 'arcsin arccos arctan arccsc arcsec arccot', items: [
      { table: { cols: ['#Function', '#Domain', '#Range'], rows: [
        [aSin('x'), '-1\\le x\\le 1', '-' + F('\\pi', '2') + '\\le y\\le ' + F('\\pi', '2')],
        [aCos('x'), '-1\\le x\\le 1', '0\\le y\\le \\pi'],
        [aTan('x'), '-\\infty<x<\\infty', '-' + F('\\pi', '2') + '<y<' + F('\\pi', '2')],
        [fn('arccsc', 'x'), A('x') + '\\ge 1', '-' + F('\\pi', '2') + '\\le y\\le ' + F('\\pi', '2') + ',\\ y\\ne 0'],
        [fn('arcsec', 'x'), A('x') + '\\ge 1', '0\\le y\\le \\pi,\\ y\\ne ' + F('\\pi', '2')],
        [fn('arccot', 'x'), '-\\infty<x<\\infty', '0<y<\\pi']] } }] },
    { h: 'Inverse properties', items: [
      { m: [sin(aSin('x')), '=', 'x'], n: 'For −1 ≤ x ≤ 1' },
      { m: [aSin(sin(t)), '=', t], n: 'For −π/2 ≤ θ ≤ π/2' },
      { m: [cos(aCos('x')), '=', 'x'], n: 'For −1 ≤ x ≤ 1' },
      { m: [aCos(cos(t)), '=', t], n: 'For 0 ≤ θ ≤ π' },
      { m: [tan(aTan('x')), '=', 'x'] },
      { m: [aTan(tan(t)), '=', t], n: 'For −π/2 < θ < π/2' }] },
    { h: 'Alternate notation', items: [
      { m: [op('sin') + '^{-1}x', '=', aSin('x')] },
      { m: [op('cos') + '^{-1}x', '=', aCos('x')] },
      { m: [op('tan') + '^{-1}x', '=', aTan('x')] }] },
    { h: 'More inverse identities', items: [
      { m: [aSin('x') + '+' + aCos('x'), '=', F('\\pi', '2')] },
      { m: [aTan('x') + '+' + fn('arccot', 'x'), '=', F('\\pi', '2')] },
      { m: [aTan('x') + '+' + aTan(F('1', 'x')), '=', F('\\pi', '2')], n: 'For x > 0' },
      { m: [aSin('-x'), '=', '-' + aSin('x')] },
      { m: [aCos('-x'), '=', '\\pi-' + aCos('x')] },
      { m: [aTan('-x'), '=', '-' + aTan('x')] },
      { m: [fn('arccsc', 'x'), '=', aSin(F('1', 'x'))] },
      { m: [fn('arcsec', 'x'), '=', aCos(F('1', 'x'))] },
      { m: [sin(aCos('x')), '=', R('1-x^2')] },
      { m: [cos(aSin('x')), '=', R('1-x^2')] },
      { m: [tan(aSin('x')), '=', F('x', R('1-x^2'))] },
      { m: [tan(aCos('x')), '=', F(R('1-x^2'), 'x')] },
      { m: [sin(aTan('x')), '=', F('x', R('1+x^2'))] },
      { m: [cos(aTan('x')), '=', F('1', R('1+x^2'))] }] }
  ];

  var TRIANGLES = [
    { h: 'Labels', items: [{ text: 'Angles α, β and γ are opposite sides a, b and c. R is the radius of the circle through the three corners.' }] },
    { h: 'Law of sines', items: [
      { m: [F(sin(al), 'a'), '=', F(sin(be), 'b'), '=', F(sin(ga), 'c')] },
      { m: [F('a', sin(al)), '=', '2R'] }] },
    { h: 'Law of cosines', items: [
      { m: ['a^2', '=', 'b^2+c^2-2bc' + cos(al)] },
      { m: ['b^2', '=', 'a^2+c^2-2ac' + cos(be)] },
      { m: ['c^2', '=', 'a^2+b^2-2ab' + cos(ga)] }] },
    { h: 'Law of tangents', items: [
      { m: [F('a-b', 'a+b'), '=', F(tan(F(al + '-' + be, '2')), tan(F(al + '+' + be, '2')))] },
      { m: [F('b-c', 'b+c'), '=', F(tan(F(be + '-' + ga, '2')), tan(F(be + '+' + ga, '2')))] },
      { m: [F('a-c', 'a+c'), '=', F(tan(F(al + '-' + ga, '2')), tan(F(al + '+' + ga, '2')))] }] },
    { h: 'Mollweide’s formula', items: [
      { m: [F('a+b', 'c'), '=', F(cos(F(al + '-' + be, '2')), sin(F(ga, '2')))] },
      { m: [F('a-b', 'c'), '=', F(sin(F(al + '-' + be, '2')), cos(F(ga, '2')))] }] },
    { h: 'Area', k: 'heron', items: [
      { m: ['\\text{Area}', '=', half + 'ab' + sin(ga)] },
      { m: ['\\text{Area}', '=', R('s' + P('s-a') + P('s-b') + P('s-c'))], n: 'Heron’s formula, with s = (a + b + c)/2' }] }
  ];

  // Exact values around the unit circle.
  var R2 = R('2'), R3 = R('3');
  var UNIT = [
    ['0', '0', '0', '1', '0'],
    [F('\\pi', '6'), '30', half, F(R3, '2'), F(R3, '3')],
    [F('\\pi', '4'), '45', F(R2, '2'), F(R2, '2'), '1'],
    [F('\\pi', '3'), '60', F(R3, '2'), half, R3],
    [F('\\pi', '2'), '90', '1', '0', '#undefined'],
    [F('2\\pi', '3'), '120', F(R3, '2'), '-' + half, '-' + R3],
    [F('3\\pi', '4'), '135', F(R2, '2'), '-' + F(R2, '2'), '-1'],
    [F('5\\pi', '6'), '150', half, '-' + F(R3, '2'), '-' + F(R3, '3')],
    ['\\pi', '180', '0', '-1', '0'],
    [F('7\\pi', '6'), '210', '-' + half, '-' + F(R3, '2'), F(R3, '3')],
    [F('5\\pi', '4'), '225', '-' + F(R2, '2'), '-' + F(R2, '2'), '1'],
    [F('4\\pi', '3'), '240', '-' + F(R3, '2'), '-' + half, R3],
    [F('3\\pi', '2'), '270', '-1', '0', '#undefined'],
    [F('5\\pi', '3'), '300', '-' + F(R3, '2'), half, '-' + R3],
    [F('7\\pi', '4'), '315', '-' + F(R2, '2'), F(R2, '2'), '-1'],
    [F('11\\pi', '6'), '330', '-' + half, F(R3, '2'), '-' + F(R3, '3')]
  ];
  var UNIT_CIRCLE = [
    { h: 'Exact values', k: 'special angles table', n: 'Click a value to type it', items: [
      { table: { cols: ['\\theta', '#degrees', sin(t), cos(t), tan(t)], rows: UNIT } }] }
  ];

  var sh = function (x) { return fn('sinh', x); };
  var ch = function (x) { return fn('cosh', x); };
  var th = function (x) { return fn('tanh', x); };
  var HYPERBOLIC = [
    { h: 'Definitions', items: [
      { m: [sh('x'), '=', F('e^x-e^{-x}', '2')] },
      { m: [ch('x'), '=', F('e^x+e^{-x}', '2')] },
      { m: [th('x'), '=', F(sh('x'), ch('x')), '=', F('e^x-e^{-x}', 'e^x+e^{-x}')] },
      { m: [fn('coth', 'x'), '=', F(ch('x'), sh('x'))] },
      { m: [fn('sech', 'x'), '=', F('1', ch('x'))] },
      { m: [fn('csch', 'x'), '=', F('1', sh('x'))] }] },
    { h: 'Pythagorean', items: [
      { m: [sq('cosh', 'x') + '-' + sq('sinh', 'x'), '=', '1'] },
      { m: ['1-' + sq('tanh', 'x'), '=', op('sech') + '^2x'] },
      { m: [sq('coth', 'x') + '-1', '=', op('csch') + '^2x'] }] },
    { h: 'Even and odd', items: [
      { m: [sh('-x'), '=', '-' + sh('x')] },
      { m: [ch('-x'), '=', ch('x')] },
      { m: [th('-x'), '=', '-' + th('x')] }] },
    { h: 'Sum and difference', items: [
      { m: [sh('x\\pm y'), '=', sh('x') + ch('y') + '\\pm ' + ch('x') + sh('y')] },
      { m: [ch('x\\pm y'), '=', ch('x') + ch('y') + '\\pm ' + sh('x') + sh('y')] },
      { m: [th('x\\pm y'), '=', F(th('x') + '\\pm ' + th('y'), '1\\pm ' + th('x') + th('y'))] }] },
    { h: 'Double angle', items: [
      { m: [sh('2x'), '=', '2' + sh('x') + ch('x')] },
      { m: [ch('2x'), '=', sq('cosh', 'x') + '+' + sq('sinh', 'x'), '=', '2' + sq('cosh', 'x') + '-1', '=', '1+2' + sq('sinh', 'x')] },
      { m: [th('2x'), '=', F('2' + th('x'), '1+' + sq('tanh', 'x'))] }] },
    { h: 'Half angle and power reducing', items: [
      { m: [sq('sinh', 'x'), '=', F(ch('2x') + '-1', '2')] },
      { m: [sq('cosh', 'x'), '=', F(ch('2x') + '+1', '2')] },
      { m: [sh(F('x', '2')), '=', '\\pm ' + R(F(ch('x') + '-1', '2'))], n: 'The sign is the sign of x' },
      { m: [ch(F('x', '2')), '=', R(F(ch('x') + '+1', '2'))] },
      { m: [th(F('x', '2')), '=', F(sh('x'), ch('x') + '+1')] }] },
    { h: 'Exponentials and the link to trig', k: 'complex osborn', items: [
      { m: [ch('x') + '+' + sh('x'), '=', 'e^x'] },
      { m: [ch('x') + '-' + sh('x'), '=', 'e^{-x}'] },
      { m: [P(ch('x') + '+' + sh('x')) + '^n', '=', ch('nx') + '+' + sh('nx')] },
      { m: [sh('ix'), '=', 'i' + sin('x')] },
      { m: [ch('ix'), '=', cos('x')] },
      { m: [th('ix'), '=', 'i' + tan('x')] }] },
    { h: 'Inverses as logarithms', k: 'arcsinh arccosh arctanh arcoth arsech arcsch', items: [
      { m: [fn('arcsinh', 'x'), '=', ln('x+' + R('x^2+1'))] },
      { m: [fn('arccosh', 'x'), '=', ln('x+' + R('x^2-1'))], n: 'For x ≥ 1' },
      { m: [fn('arctanh', 'x'), '=', half + ln(F('1+x', '1-x'))], n: 'For |x| < 1' },
      { m: [fn('arccoth', 'x'), '=', half + ln(F('x+1', 'x-1'))], n: 'For |x| > 1' },
      { m: [fn('arcsech', 'x'), '=', ln(F('1+' + R('1-x^2'), 'x'))], n: 'For 0 < x ≤ 1' },
      { m: [fn('arccsch', 'x'), '=', ln(F('1', 'x') + '+' + R(F('1', 'x^2') + '+1'))], n: 'For x ≠ 0' }] }
  ];

  var DERIVATIVES = [
    { h: 'Trig', items: [
      { m: [dx(sin('x')), '=', cos('x')] },
      { m: [dx(cos('x')), '=', '-' + sin('x')] },
      { m: [dx(tan('x')), '=', sq('sec', 'x')] },
      { m: [dx(cot('x')), '=', '-' + sq('csc', 'x')] },
      { m: [dx(sec('x')), '=', sec('x') + tan('x')] },
      { m: [dx(csc('x')), '=', '-' + csc('x') + cot('x')] },
      { m: [dx(sin('u')), '=', cos('u') + F('du', 'dx')], n: 'Chain rule: every derivative here works the same way with u in place of x' }] },
    { h: 'Inverse trig', items: [
      { m: [dx(aSin('x')), '=', F('1', R('1-x^2'))] },
      { m: [dx(aCos('x')), '=', '-' + F('1', R('1-x^2'))] },
      { m: [dx(aTan('x')), '=', F('1', '1+x^2')] },
      { m: [dx(fn('arccot', 'x')), '=', '-' + F('1', '1+x^2')] },
      { m: [dx(fn('arcsec', 'x')), '=', F('1', A('x') + R('x^2-1'))] },
      { m: [dx(fn('arccsc', 'x')), '=', '-' + F('1', A('x') + R('x^2-1'))] }] },
    { h: 'Hyperbolic', items: [
      { m: [dx(sh('x')), '=', ch('x')] },
      { m: [dx(ch('x')), '=', sh('x')] },
      { m: [dx(th('x')), '=', op('sech') + '^2x'] },
      { m: [dx(fn('coth', 'x')), '=', '-' + op('csch') + '^2x'] },
      { m: [dx(fn('sech', 'x')), '=', '-' + fn('sech', 'x') + th('x')] },
      { m: [dx(fn('csch', 'x')), '=', '-' + fn('csch', 'x') + fn('coth', 'x')] }] },
    { h: 'Inverse hyperbolic', items: [
      { m: [dx(fn('arcsinh', 'x')), '=', F('1', R('x^2+1'))] },
      { m: [dx(fn('arccosh', 'x')), '=', F('1', R('x^2-1'))], n: 'For x > 1' },
      { m: [dx(fn('arctanh', 'x')), '=', F('1', '1-x^2')], n: 'For |x| < 1' },
      { m: [dx(fn('arccoth', 'x')), '=', F('1', '1-x^2')], n: 'For |x| > 1' },
      { m: [dx(fn('arcsech', 'x')), '=', '-' + F('1', 'x' + R('1-x^2'))] },
      { m: [dx(fn('arccsch', 'x')), '=', '-' + F('1', A('x') + R('1+x^2'))] }] }
  ];

  var INTEGRALS = [
    { h: 'Trig', n: 'Each answer is + C, left off so it can be typed into a row', items: [
      { m: [I(sin('x')), '=', '-' + cos('x') + C] },
      { m: [I(cos('x')), '=', sin('x') + C] },
      { m: [I(tan('x')), '=', ln(A(sec('x'))) + C, '=', '-' + ln(A(cos('x'))) + C] },
      { m: [I(cot('x')), '=', ln(A(sin('x'))) + C] },
      { m: [I(sec('x')), '=', ln(A(sec('x') + '+' + tan('x'))) + C] },
      { m: [I(csc('x')), '=', '-' + ln(A(csc('x') + '+' + cot('x'))) + C, '=', ln(A(csc('x') + '-' + cot('x'))) + C] },
      { m: [I(sq('sec', 'x')), '=', tan('x') + C] },
      { m: [I(sq('csc', 'x')), '=', '-' + cot('x') + C] },
      { m: [I(sec('x') + tan('x')), '=', sec('x') + C] },
      { m: [I(csc('x') + cot('x')), '=', '-' + csc('x') + C] },
      { m: [I(sin('ax')), '=', '-' + F(cos('ax'), 'a') + C] },
      { m: [I(cos('ax')), '=', F(sin('ax'), 'a') + C] }] },
    { h: 'Powers of trig', items: [
      { m: [I(sq('sin', 'x')), '=', F('x', '2') + '-' + F(sin('2x'), '4') + C] },
      { m: [I(sq('cos', 'x')), '=', F('x', '2') + '+' + F(sin('2x'), '4') + C] },
      { m: [I(sq('tan', 'x')), '=', tan('x') + '-x' + C] },
      { m: [I(sq('cot', 'x')), '=', '-' + cot('x') + '-x' + C] },
      { m: [I(pw('sin', 3, 'x')), '=', F(pw('cos', 3, 'x'), '3') + '-' + cos('x') + C] },
      { m: [I(pw('cos', 3, 'x')), '=', sin('x') + '-' + F(pw('sin', 3, 'x'), '3') + C] },
      { m: [I(pw('sec', 3, 'x')), '=', half + P(sec('x') + tan('x') + '+' + ln(A(sec('x') + '+' + tan('x')))) + C] },
      { m: [I(pw('csc', 3, 'x')), '=', half + P('-' + csc('x') + cot('x') + '+' + ln(A(csc('x') + '-' + cot('x')))) + C] },
      { m: [I(sin('x') + cos('x')), '=', F(sq('sin', 'x'), '2') + C] }] },
    { h: 'Reduction formulas', k: 'power n', items: [
      { m: [I(pw('sin', 'n', 'x')), '=', '-' + F(pw('sin', 'n-1', 'x') + cos('x'), 'n') + '+' + F('n-1', 'n') + I(pw('sin', 'n-2', 'x'))] },
      { m: [I(pw('cos', 'n', 'x')), '=', F(pw('cos', 'n-1', 'x') + sin('x'), 'n') + '+' + F('n-1', 'n') + I(pw('cos', 'n-2', 'x'))] },
      { m: [I(pw('tan', 'n', 'x')), '=', F(pw('tan', 'n-1', 'x'), 'n-1') + '-' + I(pw('tan', 'n-2', 'x'))] },
      { m: [I(pw('sec', 'n', 'x')), '=', F(pw('sec', 'n-2', 'x') + tan('x'), 'n-1') + '+' + F('n-2', 'n-1') + I(pw('sec', 'n-2', 'x'))] }] },
    { h: 'Giving inverse trig', k: 'arcsin arctan arcsec trig substitution', n: 'a > 0', items: [
      { m: [I(F('1', R('a^2-x^2'))), '=', aSin(F('x', 'a')) + C] },
      { m: [I(F('1', 'a^2+x^2')), '=', F('1', 'a') + aTan(F('x', 'a')) + C] },
      { m: [I(F('1', 'x' + R('x^2-a^2'))), '=', F('1', 'a') + fn('arcsec', F(A('x'), 'a')) + C] },
      { m: [I(R('a^2-x^2')), '=', F('x', '2') + R('a^2-x^2') + '+' + F('a^2', '2') + aSin(F('x', 'a')) + C] }] },
    { h: 'Of inverse trig', items: [
      { m: [I(aSin('x')), '=', 'x' + aSin('x') + '+' + R('1-x^2') + C] },
      { m: [I(aCos('x')), '=', 'x' + aCos('x') + '-' + R('1-x^2') + C] },
      { m: [I(aTan('x')), '=', 'x' + aTan('x') + '-' + half + ln('1+x^2') + C] }] },
    { h: 'Hyperbolic', items: [
      { m: [I(sh('x')), '=', ch('x') + C] },
      { m: [I(ch('x')), '=', sh('x') + C] },
      { m: [I(th('x')), '=', ln(ch('x')) + C] },
      { m: [I(fn('coth', 'x')), '=', ln(A(sh('x'))) + C] },
      { m: [I(fn('sech', 'x')), '=', aTan(sh('x')) + C] },
      { m: [I(fn('csch', 'x')), '=', ln(A(th(F('x', '2')))) + C] },
      { m: [I(op('sech') + '^2x'), '=', th('x') + C] },
      { m: [I(op('csch') + '^2x'), '=', '-' + fn('coth', 'x') + C] },
      { m: [I(fn('sech', 'x') + th('x')), '=', '-' + fn('sech', 'x') + C] },
      { m: [I(fn('csch', 'x') + fn('coth', 'x')), '=', '-' + fn('csch', 'x') + C] }] },
    { h: 'Giving inverse hyperbolic', k: 'arcsinh arccosh arctanh', n: 'a > 0', items: [
      { m: [I(F('1', R('x^2+a^2'))), '=', fn('arcsinh', F('x', 'a')) + C, '=', ln('x+' + R('x^2+a^2')) + C] },
      { m: [I(F('1', R('x^2-a^2'))), '=', ln(A('x+' + R('x^2-a^2'))) + C], n: 'Equal to arccosh(x/a) + C for x > a' },
      { m: [I(F('1', 'a^2-x^2')), '=', F('1', '2a') + ln(A(F('a+x', 'a-x'))) + C], n: 'Equal to (1/a) arctanh(x/a) + C for |x| < a' }] }
  ];

  // ---- Series ----------------------------------------------------------------
  //
  // Sums are written the way the calculator reads them, so a side typed into a
  // row evaluates (or graphs, when it has x in it). An item's ex is the same
  // series written out term by term: shown for reading, never inserted.

  function S(lo, hi, body, v) { return '\\sum_{' + (v || 'n') + '=' + lo + '}^{' + hi + '}' + body; }
  function Sn(lo, body) { return S(lo, inf, body); }
  var neg = '\\left(-1\\right)';
  var lim = '\\lim_{n\\to\\infty}';
  function T(s) { return '\\text{' + s + '}'; }

  var SERIES = [
    { h: 'Geometric', k: 'geometric ratio common', n: 'a is the first term, r the common ratio', items: [
      { m: [S('0', 'N-1', 'ar^n'), '=', F('a' + P('1-r^N'), '1-r')], n: 'The first N terms, for r ≠ 1 (when r = 1 the sum is aN)', ex: 'a+ar+ar^2+\\cdots+ar^{N-1}' },
      { m: [Sn('0', 'ar^n'), '=', F('a', '1-r')], n: 'Converges for |r| < 1, diverges for |r| ≥ 1', ex: 'a+ar+ar^2+ar^3+\\cdots' },
      { m: [Sn('1', 'ar^n'), '=', F('ar', '1-r')], n: 'Starting at n = 1: first term over (1 − r), for |r| < 1' },
      { m: [Sn('0', 'nr^n'), '=', F('r', P('1-r') + '^2')], n: 'For |r| < 1; the derivative of the geometric series times r' },
      { m: [Sn('0', 'n^2r^n'), '=', F('r' + P('1+r'), P('1-r') + '^3')], n: 'For |r| < 1' }] },
    { h: 'Arithmetic', k: 'arithmetic difference common', n: 'a₁ is the first term, d the common difference', items: [
      { m: ['a_n', '=', 'a_1+' + P('n-1') + 'd'], n: 'The nth term' },
      { m: [S('1', 'n', P('a_1+' + P('k-1') + 'd'), 'k'), '=', F('n', '2') + P('2a_1+' + P('n-1') + 'd'), '=', F('n' + P('a_1+a_n'), '2')], n: 'n terms: the average of the first and last, times n', ex: 'a_1+\\left(a_1+d\\right)+\\left(a_1+2d\\right)+\\cdots+a_n' }] },
    { h: 'Sums of powers', k: 'faulhaber power sum integers squares cubes', items: [
      { m: [S('1', 'n', '1', 'k'), '=', 'n'] },
      { m: [S('1', 'n', 'k', 'k'), '=', F('n' + P('n+1'), '2')], ex: '1+2+3+\\cdots+n' },
      { m: [S('1', 'n', 'k^2', 'k'), '=', F('n' + P('n+1') + P('2n+1'), '6')], ex: '1+4+9+\\cdots+n^2' },
      { m: [S('1', 'n', 'k^3', 'k'), '=', P(F('n' + P('n+1'), '2')) + '^2'], n: 'The square of 1 + 2 + … + n', ex: '1+8+27+\\cdots+n^3' },
      { m: [S('1', 'n', 'k^4', 'k'), '=', F('n' + P('n+1') + P('2n+1') + P('3n^2+3n-1'), '30')] },
      { m: [S('1', 'n', P('2k-1'), 'k'), '=', 'n^2'], n: 'The first n odd numbers', ex: '1+3+5+\\cdots+\\left(2n-1\\right)' }] },
    { h: 'p-series and harmonic series', k: 'p series harmonic zeta basel', items: [
      { m: [Sn('1', F('1', 'n^p')), '=', zeta('p')], n: 'Converges for p > 1, diverges for p ≤ 1', ex: '1+\\frac{1}{2^p}+\\frac{1}{3^p}+\\cdots' },
      { m: [Sn('1', F('1', 'n'))], n: 'The harmonic series: diverges, though slowly', ex: '1+\\frac{1}{2}+\\frac{1}{3}+\\frac{1}{4}+\\cdots' },
      { m: ['H_n', '=', S('1', 'n', F('1', 'k'), 'k')], n: 'The nth harmonic number; Hₙ ≈ ln n + γ for large n (γ ≈ 0.5772156649)' },
      { m: [Sn('1', F('1', 'n^2')), '=', F('\\pi^2', '6')], n: 'The Basel problem' },
      { m: [Sn('1', F('1', 'n^4')), '=', F('\\pi^4', '90')] },
      { m: [Sn('1', F('1', P('2n-1') + '^2')), '=', F('\\pi^2', '8')], n: 'Odd squares only' }] },
    { h: 'Alternating series', k: 'alternating leibniz ln 2', items: [
      { m: [Sn('1', F(neg + '^{n+1}', 'n')), '=', ln('2')], n: 'The alternating harmonic series: converges, but not absolutely', ex: '1-\\frac{1}{2}+\\frac{1}{3}-\\frac{1}{4}+\\cdots' },
      { m: [Sn('0', F(neg + '^n', '2n+1')), '=', F('\\pi', '4')], n: 'The Leibniz series', ex: '1-\\frac{1}{3}+\\frac{1}{5}-\\frac{1}{7}+\\cdots' },
      { m: [Sn('1', F(neg + '^{n+1}', 'n^2')), '=', F('\\pi^2', '12')] },
      { m: [A('S-S_N'), '\\le', 'b_{N+1}'], n: 'Error after N terms of Σ(−1)ⁿbₙ when the alternating series test applies: at most the first term left out' }] },
    { h: 'Telescoping', k: 'telescoping partial fractions cancel', n: 'Split the term into a difference, often with partial fractions, so neighbouring terms cancel', items: [
      { m: [S('1', 'N', P('b_n-b_{n+1}')), '=', 'b_1-b_{N+1}'], n: 'So the infinite sum is b₁ − lim bₙ when that limit exists', ex: '\\left(b_1-b_2\\right)+\\left(b_2-b_3\\right)+\\cdots+\\left(b_N-b_{N+1}\\right)' },
      { m: [F('1', 'n' + P('n+1')), '=', F('1', 'n') + '-' + F('1', 'n+1')] },
      { m: [Sn('1', F('1', 'n' + P('n+1'))), '=', '1'] },
      { m: [Sn('1', F('1', 'n' + P('n+2'))), '=', F('3', '4')], n: 'Terms two apart: 1/(n(n+2)) = ½(1/n − 1/(n+2))' },
      { m: [Sn('2', F('1', 'n^2-1')), '=', F('3', '4')] },
      { m: [S('1', 'N', ln(F('n+1', 'n'))), '=', ln('N+1')], n: 'So Σ ln(1 + 1/n) diverges' }] },
    { h: 'Sums for e and π', k: 'constant euler number e pi', items: [
      { m: [Sn('0', F('1', 'n!')), '=', 'e'], ex: '1+1+\\frac{1}{2!}+\\frac{1}{3!}+\\cdots' },
      { m: [Sn('0', F(neg + '^n', 'n!')), '=', F('1', 'e')] },
      { m: [Sn('1', F('n', '2^n')), '=', '2'] },
      { m: [Sn('0', F('1', '2^n')), '=', '2'], ex: '1+\\frac{1}{2}+\\frac{1}{4}+\\frac{1}{8}+\\cdots' },
      { m: [Sn('1', F('1', 'n^2')), '=', F('\\pi^2', '6')] },
      { m: [Sn('0', F(neg + '^n', '2n+1')), '=', F('\\pi', '4')] }] },
    { h: 'Taylor series', k: 'taylor maclaurin polynomial remainder lagrange', n: 'f⁽ⁿ⁾ is the nth derivative of f; a Maclaurin series is a Taylor series at a = 0', items: [
      { m: ['f' + P('x'), '=', Sn('0', F('f^{\\left(n\\right)}' + P('a'), 'n!') + P('x-a') + '^n')], n: 'The Taylor series of f at a, where it converges to f', ex: 'f\\left(a\\right)+f\'\\left(a\\right)\\left(x-a\\right)+\\frac{f\'\'\\left(a\\right)}{2!}\\left(x-a\\right)^2+\\cdots' },
      { m: ['f' + P('x'), '=', Sn('0', F('f^{\\left(n\\right)}' + P('0'), 'n!') + 'x^n')], n: 'Maclaurin series', ex: 'f\\left(0\\right)+f\'\\left(0\\right)x+\\frac{f\'\'\\left(0\\right)}{2!}x^2+\\cdots' },
      { m: ['T_N' + P('x'), '=', S('0', 'N', F('f^{\\left(n\\right)}' + P('a'), 'n!') + P('x-a') + '^n')], n: 'The Taylor polynomial of degree N' },
      { m: ['R_N' + P('x'), '=', F('f^{\\left(N+1\\right)}' + P('c'), P('N+1') + '!') + P('x-a') + '^{N+1}'], n: 'Lagrange remainder f(x) − T_N(x), for some c between a and x' },
      { m: [A('R_N' + P('x')), '\\le', F('M', P('N+1') + '!') + A('x-a') + '^{N+1}'], n: 'Taylor’s inequality, where |f⁽ᴺ⁺¹⁾| ≤ M between a and x' }] },
    { h: 'Maclaurin series of common functions', k: 'maclaurin taylor expansion exp sin cos ln arctan sinh cosh binomial', n: 'Each grey note says where the series converges', items: [
      { m: ['e^x', '=', Sn('0', F('x^n', 'n!'))], n: 'Every x', ex: '1+x+\\frac{x^2}{2!}+\\frac{x^3}{3!}+\\cdots' },
      { m: [sin('x'), '=', Sn('0', F(neg + '^nx^{2n+1}', P('2n+1') + '!'))], n: 'Every x', ex: 'x-\\frac{x^3}{3!}+\\frac{x^5}{5!}-\\cdots' },
      { m: [cos('x'), '=', Sn('0', F(neg + '^nx^{2n}', P('2n') + '!'))], n: 'Every x', ex: '1-\\frac{x^2}{2!}+\\frac{x^4}{4!}-\\cdots' },
      { m: [F('1', '1-x'), '=', Sn('0', 'x^n')], n: '|x| < 1', ex: '1+x+x^2+x^3+\\cdots' },
      { m: [F('1', '1+x'), '=', Sn('0', neg + '^nx^n')], n: '|x| < 1', ex: '1-x+x^2-x^3+\\cdots' },
      { m: [F('1', P('1-x') + '^2'), '=', Sn('1', 'nx^{n-1}')], n: '|x| < 1', ex: '1+2x+3x^2+4x^3+\\cdots' },
      { m: [ln('1+x'), '=', Sn('1', F(neg + '^{n+1}x^n', 'n'))], n: '−1 < x ≤ 1', ex: 'x-\\frac{x^2}{2}+\\frac{x^3}{3}-\\cdots' },
      { m: ['-' + ln('1-x'), '=', Sn('1', F('x^n', 'n'))], n: '−1 ≤ x < 1', ex: 'x+\\frac{x^2}{2}+\\frac{x^3}{3}+\\cdots' },
      { m: [fn('arctan', 'x'), '=', Sn('0', F(neg + '^nx^{2n+1}', '2n+1'))], n: '|x| ≤ 1', ex: 'x-\\frac{x^3}{3}+\\frac{x^5}{5}-\\cdots' },
      { m: [fn('arcsin', 'x'), '=', Sn('0', F(P('2n') + '!', '4^n' + P('n!') + '^2' + P('2n+1')) + 'x^{2n+1}')], n: '|x| ≤ 1', ex: 'x+\\frac{1}{2}\\cdot\\frac{x^3}{3}+\\frac{1\\cdot3}{2\\cdot4}\\cdot\\frac{x^5}{5}+\\cdots' },
      { m: [fn('sinh', 'x'), '=', Sn('0', F('x^{2n+1}', P('2n+1') + '!'))], n: 'Every x', ex: 'x+\\frac{x^3}{3!}+\\frac{x^5}{5!}+\\cdots' },
      { m: [fn('cosh', 'x'), '=', Sn('0', F('x^{2n}', P('2n') + '!'))], n: 'Every x', ex: '1+\\frac{x^2}{2!}+\\frac{x^4}{4!}+\\cdots' },
      { m: [fn('arctanh', 'x'), '=', Sn('0', F('x^{2n+1}', '2n+1'))], n: '|x| < 1', ex: 'x+\\frac{x^3}{3}+\\frac{x^5}{5}+\\cdots' },
      { m: ['e^{-x^2}', '=', Sn('0', F(neg + '^nx^{2n}', 'n!'))], n: 'Every x; integrate term by term for erf', ex: '1-x^2+\\frac{x^4}{2!}-\\frac{x^6}{3!}+\\cdots' },
      { m: [P('1+x') + '^{\\alpha}', '=', Sn('0', '\\binom{\\alpha}{n}x^n')], n: 'Binomial series: |x| < 1, or every x when α is a whole number (then it stops). The coefficient (α choose n) = α(α−1)…(α−n+1)/n!, typed ;nck', ex: '1+\\alpha x+\\frac{\\alpha\\left(\\alpha-1\\right)}{2!}x^2+\\cdots' },
      { m: [R('1+x'), '=', '1+' + F('x', '2') + '-' + F('x^2', '8') + '+' + F('x^3', '16') + '-\\cdots'], n: 'α = ½ in the binomial series; |x| ≤ 1' },
      { m: [tan('x'), '=', 'x+' + F('x^3', '3') + '+' + F('2x^5', '15') + '+' + F('17x^7', '315') + '+\\cdots'], n: '|x| < π/2; no simple general term' }] },
    { h: 'Power series', k: 'power series radius interval convergence term by term', n: 'cₙ are the coefficients and a the centre', items: [
      { m: [Sn('0', 'c_n' + P('x-a') + '^n')], n: 'Converges for |x − a| < R and diverges for |x − a| > R; test the two endpoints x = a ± R on their own', ex: 'c_0+c_1\\left(x-a\\right)+c_2\\left(x-a\\right)^2+\\cdots' },
      { m: ['R', '=', lim + A(F('c_n', 'c_{n+1}'))], n: 'Radius of convergence, when the limit exists (∞ means every x)' },
      { m: [F('1', 'R'), '=', '\\limsup_{n\\to\\infty}' + A('c_n') + '^{' + F('1', 'n') + '}'], n: 'Cauchy–Hadamard formula: always works' },
      { m: [dx(Sn('0', 'c_nx^n')), '=', Sn('1', 'nc_nx^{n-1}')], n: 'Differentiate term by term; the radius stays R' },
      { m: [I(Sn('0', 'c_nx^n')), '=', Sn('0', F('c_nx^{n+1}', 'n+1'))], n: 'Integrate term by term (+ C); the radius stays R' }] },
    { h: 'Fourier series', k: 'fourier trigonometric periodic coefficients parseval', n: 'f has period 2L; for period 2π put L = π', items: [
      { m: ['f' + P('x'), '=', F('a_0', '2') + '+' + Sn('1', P('a_n' + cos(F('n\\pi x', 'L')) + '+b_n' + sin(F('n\\pi x', 'L'))))], n: 'Converges to f where f is smooth, and to the average of the two sides at a jump', ex: '\\frac{a_0}{2}+a_1\\cos\\frac{\\pi x}{L}+b_1\\sin\\frac{\\pi x}{L}+a_2\\cos\\frac{2\\pi x}{L}+\\cdots' },
      { m: ['a_n', '=', F('1', 'L') + DI('-L', 'L', 'f' + P('x') + cos(F('n\\pi x', 'L')), 'x')], n: 'n = 0, 1, 2, …; a₀/2 is the average of f' },
      { m: ['b_n', '=', F('1', 'L') + DI('-L', 'L', 'f' + P('x') + sin(F('n\\pi x', 'L')), 'x')], n: 'n = 1, 2, 3, …; an even f has every bₙ = 0, an odd f every aₙ = 0' },
      { m: ['f' + P('x'), '=', S('-' + inf, inf, 'c_ne^{' + F('in\\pi x', 'L') + '}')], n: 'Complex form' },
      { m: ['c_n', '=', F('1', '2L') + DI('-L', 'L', 'f' + P('x') + 'e^{-' + F('in\\pi x', 'L') + '}', 'x')] },
      { m: [F('1', 'L') + DI('-L', 'L', 'f' + P('x') + '^2', 'x'), '=', F('a_0^2', '2') + '+' + Sn('1', P('a_n^2+b_n^2'))], n: 'Parseval’s identity' }] },
    { h: 'Fourier series of common waves', k: 'fourier square sawtooth triangle wave', n: 'Period 2π. Insert a sum to graph it; change ∞ to a number such as 10 to see a partial sum', items: [
      { m: ['f' + P('x'), '=', F('4', '\\pi') + Sn('1', F(sin(P('2n-1') + 'x'), '2n-1'))], n: 'Square wave: 1 on (0, π) and −1 on (−π, 0)', ex: '\\frac{4}{\\pi}\\left(\\sin x+\\frac{\\sin 3x}{3}+\\frac{\\sin 5x}{5}+\\cdots\\right)' },
      { m: ['f' + P('x'), '=', '2' + Sn('1', F(neg + '^{n+1}' + sin('nx'), 'n'))], n: 'Sawtooth wave: x on (−π, π)', ex: '2\\left(\\sin x-\\frac{\\sin 2x}{2}+\\frac{\\sin 3x}{3}-\\cdots\\right)' },
      { m: ['f' + P('x'), '=', F('\\pi', '2') + '-' + F('4', '\\pi') + Sn('1', F(cos(P('2n-1') + 'x'), P('2n-1') + '^2'))], n: 'Triangle wave: |x| on (−π, π)', ex: '\\frac{\\pi}{2}-\\frac{4}{\\pi}\\left(\\cos x+\\frac{\\cos 3x}{9}+\\frac{\\cos 5x}{25}+\\cdots\\right)' },
      { m: ['f' + P('x'), '=', F('\\pi^2', '3') + '+4' + Sn('1', F(neg + '^n' + cos('nx'), 'n^2'))], n: 'Repeated parabola: x² on (−π, π); at x = π it gives Σ 1/n² = π²/6' }] }
  ];

  var an = 'a_n', sa = Sn('1', 'a_n'), sb = Sn('1', 'b_n');
  var CONVERGENCE = [
    { h: 'Divergence test (nth term test)', k: 'nth term divergence limit zero', items: [
      { m: [lim + an + '\\ne 0', '\\Rightarrow', sa + '\\ ' + T('diverges')], n: 'If the limit is 0 the test says nothing (the harmonic series has limit 0 and diverges)' }] },
    { h: 'Geometric series test', k: 'geometric ratio', items: [
      { m: [Sn('0', 'ar^n') + '\\ ' + T('converges'), '\\iff', A('r') + '<1'], n: 'To a/(1 − r), for a ≠ 0' }] },
    { h: 'p-series test', k: 'p series', items: [
      { m: [Sn('1', F('1', 'n^p')) + '\\ ' + T('converges'), '\\iff', 'p>1'], n: 'p ≤ 1 diverges, so Σ 1/n and Σ 1/√n diverge' }] },
    { h: 'Integral test', k: 'integral improper', n: 'f positive, continuous and decreasing for x ≥ 1, with aₙ = f(n)', items: [
      { m: [sa + '\\ ' + T('converges'), '\\iff', DI('1', inf, 'f' + P('x'), 'x') + '\\ ' + T('converges')] },
      { m: [DI('N+1', inf, 'f' + P('x'), 'x'), '\\le', 'S-S_N', '\\le', DI('N', inf, 'f' + P('x'), 'x')], n: 'Bounds on the error after N terms' }] },
    { h: 'Comparison test', k: 'direct comparison', n: '0 ≤ aₙ ≤ bₙ for every n past some point', items: [
      { m: [sb + '\\ ' + T('converges'), '\\Rightarrow', sa + '\\ ' + T('converges')] },
      { m: [sa + '\\ ' + T('diverges'), '\\Rightarrow', sb + '\\ ' + T('diverges')], n: 'Compare with geometric series and p-series' }] },
    { h: 'Limit comparison test', k: 'limit comparison', n: 'aₙ > 0 and bₙ > 0', items: [
      { m: [lim + F('a_n', 'b_n'), '=', 'c'], n: '0 < c < ∞: both converge or both diverge. c = 0 and Σbₙ converges: Σaₙ converges. c = ∞ and Σbₙ diverges: Σaₙ diverges' }] },
    { h: 'Ratio test', k: 'ratio factorial', items: [
      { m: [lim + A(F('a_{n+1}', 'a_n')), '=', 'L'], n: 'L < 1: converges absolutely. L > 1 or L = ∞: diverges. L = 1: no conclusion. Best for factorials and nth powers' }] },
    { h: 'Root test', k: 'root nth root', items: [
      { m: [lim + A(an) + '^{' + F('1', 'n') + '}', '=', 'L'], n: 'L < 1: converges absolutely. L > 1 or L = ∞: diverges. L = 1: no conclusion. Best when aₙ is something to the nth power' }] },
    { h: 'Alternating series test', k: 'alternating leibniz', n: 'bₙ > 0', items: [
      { m: ['b_{n+1}\\le b_n\\ ' + T('and') + '\\ ' + lim + 'b_n=0', '\\Rightarrow', Sn('1', neg + '^{n+1}b_n') + '\\ ' + T('converges')] },
      { m: [A('S-S_N'), '\\le', 'b_{N+1}'], n: 'The error after N terms is at most the next term, and has its sign' }] },
    { h: 'Absolute and conditional convergence', k: 'absolute conditional rearrangement', items: [
      { m: [Sn('1', A(an)) + '\\ ' + T('converges'), '\\Rightarrow', sa + '\\ ' + T('converges')], n: 'Σaₙ then converges absolutely, and any rearrangement has the same sum' },
      { m: [sa + '\\ ' + T('converges and') + '\\ ' + Sn('1', A(an)) + '\\ ' + T('diverges')], n: 'Conditional convergence: rearranging the terms can give any sum (Riemann rearrangement theorem). Example: Σ(−1)ⁿ⁺¹/n' }] },
    { h: 'Choosing a test', k: 'which test strategy', items: [
      { text: '1. Does aₙ fail to go to 0? Then it diverges (divergence test).' },
      { text: '2. Geometric or p-series? Read the answer off directly.' },
      { text: '3. Alternating? Try the alternating series test, and check absolute convergence.' },
      { text: '4. Factorials or nth powers? Ratio test; aₙ = (something)ⁿ: root test.' },
      { text: '5. A fraction of powers of n? Limit comparison with the p-series of the leading terms.' },
      { text: '6. aₙ = f(n) with f easy to integrate? Integral test.' },
      { text: 'To check numerically, type the sum with ∞ on top into a row: it shows the value, or says it diverges.' }] }
  ];

  var SPECIAL = [
    { h: 'Gamma function Γ(x)', type: 'Gamma(x)', domain: 'Every x except 0, −1, −2, … (and complex z in a z row)', k: 'factorial', items: [
      { m: [Gam('x'), '=', DI('0', inf, 't^{x-1}e^{-t}')], n: 'The definition, for x > 0; the rest follows from Γ(x + 1) = xΓ(x)' },
      { m: [Gam('x+1'), '=', 'x' + Gam('x')] },
      { m: [Gam('n'), '=', P('n-1') + '!'], n: 'n a positive integer; x! = Γ(x + 1) for any x' },
      { m: [Gam(half), '=', R('\\pi')] },
      { m: [Gam('x') + Gam('1-x'), '=', F('\\pi', sin('\\pi x'))], n: 'Reflection formula' }] },
    { h: 'Digamma function ψ(x)', type: 'psi(x)', domain: 'Every x except 0, −1, −2, …', k: 'psi polygamma harmonic', items: [
      { m: [psi('x'), '=', '\\frac{d}{dx}' + ln(Gam('x')), '=', F("\\Gamma '" + P('x'), Gam('x'))], n: 'The definition' },
      { m: [psi('x+1'), '=', psi('x') + '+' + F('1', 'x')] },
      { m: [psi('1'), '=', '-\\gamma'], n: 'γ ≈ 0.5772156649 is the Euler–Mascheroni constant (in the app, γ is an ordinary variable)' },
      { m: [psi('n'), '=', '\\sum_{k=1}^{n-1}' + F('1', 'k') + '-\\gamma'], n: 'n a positive integer' },
      { m: [psi('x'), '=', '-\\gamma+\\sum_{k=0}^{\\infty}' + P(F('1', 'k+1') + '-' + F('1', 'k+x'))] },
      { m: [psi('1-x') + '-' + psi('x'), '=', '\\pi' + cot('\\pi x')], n: 'Reflection formula' }] },
    { h: 'Beta function B(a, b)', type: 'B(a, b)', domain: 'a > 0 and b > 0 for the integral; elsewhere through Γ', items: [
      { m: ['B' + P('a,b'), '=', DI('0', '1', 't^{a-1}' + P('1-t') + '^{b-1}')], n: 'The definition' },
      { m: ['B' + P('a,b'), '=', F(Gam('a') + Gam('b'), Gam('a+b'))] },
      { m: ['B' + P('a,b'), '=', 'B' + P('b,a')] },
      { m: ['B' + P('a,b'), '=', '2' + DI('0', F('\\pi', '2'), pw('sin', '2a-1', t) + pw('cos', '2b-1', t), t)] }] },
    { h: 'Riemann zeta function ζ(s)', type: 'zeta(s)', domain: 'Every s except 1 (and complex s in a z row)', k: 'riemann basel', items: [
      { m: [zeta('s'), '=', '\\sum_{n=1}^{\\infty}' + F('1', 'n^s')], n: 'The definition, for s > 1; other s by analytic continuation' },
      { m: [zeta('s'), '=', '\\prod_{p\\ \\text{prime}}' + F('1', '1-p^{-s}')], n: 'Euler product, for s > 1' },
      { m: [zeta('s'), '=', '2^s\\pi^{s-1}' + sin(F('\\pi s', '2')) + Gam('1-s') + zeta('1-s')], n: 'Functional equation' },
      { m: [zeta('2'), '=', F('\\pi^2', '6')], n: 'The Basel problem' },
      { m: [zeta('4'), '=', F('\\pi^4', '90')] },
      { m: [zeta('0'), '=', '-' + half] },
      { m: [zeta('-1'), '=', '-' + F('1', '12')] },
      { m: [zeta('-2n'), '=', '0'], n: 'The trivial zeros, n a positive integer' }] },
    { h: 'Error functions erf, erfc, erfi', type: 'erf(x), erfc(x), erfi(x)', domain: 'Every x', k: 'gaussian normal', items: [
      { m: [erf('x'), '=', F('2', R('\\pi')) + DI('0', 'x', 'e^{-t^2}')], n: 'The definition' },
      { m: [op('erfc') + P('x'), '=', '1-' + erf('x'), '=', F('2', R('\\pi')) + DI('x', inf, 'e^{-t^2}')], n: 'Complementary error function' },
      { m: [op('erfi') + P('x'), '=', '-i' + erf('ix'), '=', F('2', R('\\pi')) + DI('0', 'x', 'e^{t^2}')], n: 'Imaginary error function' },
      { m: [erf('-x'), '=', '-' + erf('x')] },
      { m: ['\\lim_{x\\to\\infty}' + erf('x'), '=', '1'] },
      { m: [dx(erf('x')), '=', F('2', R('\\pi')) + 'e^{-x^2}'] },
      { m: [I('e^{-x^2}'), '=', F(R('\\pi'), '2') + erf('x') + C] },
      { m: ['\\Phi' + P('x'), '=', half + P('1+' + erf(F('x', R('2'))))], n: 'The standard normal CDF' }] },
    { h: 'Sine and cosine integrals Si, Ci', type: 'Si(x), Ci(x)', domain: 'Si: every x. Ci: x > 0', items: [
      { m: [op('Si') + P('x'), '=', DI('0', 'x', F(sin('t'), 't'))], n: 'The definition' },
      { m: [op('Ci') + P('x'), '=', '-' + DI('x', inf, F(cos('t'), 't')), '=', '\\gamma+' + ln('x') + '+' + DI('0', 'x', F(cos('t') + '-1', 't'))], n: 'The definition (γ ≈ 0.5772156649)' },
      { m: ['\\lim_{x\\to\\infty}' + op('Si') + P('x'), '=', F('\\pi', '2')] },
      { m: [dx(op('Si') + P('x')), '=', F(sin('x'), 'x')] },
      { m: [dx(op('Ci') + P('x')), '=', F(cos('x'), 'x')] }] },
    { h: 'Exponential and logarithmic integrals Ei, li', type: 'Ei(x), li(x)', domain: 'Ei: x ≠ 0. li: x > 0, x ≠ 1', k: 'prime counting', items: [
      { m: [op('Ei') + P('x'), '=', '-' + DI('-x', inf, F('e^{-t}', 't')), '=', DI('-\\infty', 'x', F('e^t', 't'))], n: 'The definition (principal value across t = 0)' },
      { m: [op('Ei') + P('x'), '=', '\\gamma+' + ln(A('x')) + '+\\sum_{n=1}^{\\infty}' + F('x^n', 'n\\cdot n!')] },
      { m: [op('li') + P('x'), '=', DI('0', 'x', F('1', ln('t')))], n: 'The definition (principal value across t = 1)' },
      { m: [op('li') + P('x'), '=', op('Ei') + P(ln('x'))] },
      { m: [dx(op('Ei') + P('x')), '=', F('e^x', 'x')] },
      { m: [dx(op('li') + P('x')), '=', F('1', ln('x'))] }] },
    { h: 'Polylogarithm Liₛ(x)', type: 'Li_2(x), or Li_s(x) for any real order s', domain: '|x| ≤ 1 for the series; real x < 1 by continuation', k: 'dilogarithm spence', items: [
      { m: [op('Li') + '_s' + P('x'), '=', '\\sum_{k=1}^{\\infty}' + F('x^k', 'k^s')], n: 'The definition' },
      { m: [op('Li') + '_1' + P('x'), '=', '-' + ln('1-x')] },
      { m: [op('Li') + '_2' + P('x'), '=', '-' + DI('0', 'x', F(ln('1-t'), 't'))], n: 'The dilogarithm' },
      { m: [op('Li') + '_s' + P('1'), '=', zeta('s')], n: 'For s > 1' },
      { m: [op('Li') + '_2' + P('1'), '=', F('\\pi^2', '6')] },
      { m: [op('Li') + '_2' + P('-1'), '=', '-' + F('\\pi^2', '12')] },
      { m: [dx(op('Li') + '_s' + P('x')), '=', F(op('Li') + '_{s-1}' + P('x'), 'x')] }] },
    { h: 'Lambert W function W(x)', type: 'W(x)', domain: 'x ≥ −1/e (the principal branch, W ≥ −1)', k: 'product log omega', items: [
      { m: ['W' + P('x') + 'e^{W' + P('x') + '}', '=', 'x'], n: 'The definition: W is the inverse of x eˣ' },
      { m: ['W' + P('xe^x'), '=', 'x'], n: 'For x ≥ −1' },
      { m: ['W' + P('0'), '=', '0'] },
      { m: ['W' + P('e'), '=', '1'] },
      { m: ['W' + P('-' + F('1', 'e')), '=', '-1'] },
      { m: [dx('W' + P('x')), '=', F('W' + P('x'), 'x' + P('1+W' + P('x')))] },
      { m: ['x', '=', F(ln('a'), 'W' + P(ln('a')))], n: 'Solves xˣ = a' }] },
    { h: 'Complete elliptic integrals K(k), E(k)', type: 'K(k), E(k)', domain: 'Modulus k: |k| < 1 for K, |k| ≤ 1 for E', k: 'ellipse perimeter pendulum', items: [
      { m: ['K' + P('k'), '=', DI('0', F('\\pi', '2'), F('1', R('1-k^2' + sq('sin', t))), t)], n: 'The definition (K with a subscript is the Bessel Kₙ)' },
      { m: ['E' + P('k'), '=', DI('0', F('\\pi', '2'), R('1-k^2' + sq('sin', t)), t)], n: 'The definition' },
      { m: ['K' + P('0'), '=', 'E' + P('0'), '=', F('\\pi', '2')] },
      { m: ['E' + P('1'), '=', '1'] },
      { m: ['\\text{perimeter}', '=', '4aE' + P(R('1-' + F('b^2', 'a^2')))], n: 'Ellipse with semi-axes a ≥ b' },
      { m: ['T', '=', '4' + R(F('L', 'g')) + 'K' + P(sin(F('\\theta_0', '2')))], n: 'Period of a pendulum swinging to angle θ₀' }] },
    { h: 'Bessel functions Jₙ, Yₙ', type: 'J_n(x), Y_n(x); J_{n+1}(x) for an order that is an expression', domain: 'J: every x (x ≥ 0 for non-integer n). Y: x > 0. Any real order n', k: 'bessel first second kind', items: [
      { m: ['J_n' + P('x'), '=', '\\sum_{m=0}^{\\infty}' + F('\\left(-1\\right)^m', 'm!' + Gam('m+n+1')) + P(F('x', '2')) + '^{2m+n}'], n: 'The definition' },
      { m: ['J_n' + P('x'), '=', F('1', '\\pi') + DI('0', '\\pi', cos('n\\tau-x' + sin('\\tau')), '\\tau')], n: 'For integer n' },
      { m: ['Y_n' + P('x'), '=', F('J_n' + P('x') + cos('n\\pi') + '-J_{-n}' + P('x'), sin('n\\pi'))], n: 'The definition (its limit at integer n)' },
      { m: ['x^2y\'\'+xy\'+' + P('x^2-n^2') + 'y', '=', '0'], n: 'Bessel’s equation: y = Jₙ(x) and Yₙ(x) solve it' },
      { m: ['J_{-n}' + P('x'), '=', '\\left(-1\\right)^nJ_n' + P('x')], n: 'For integer n' },
      { m: ['J_{n-1}' + P('x') + '+J_{n+1}' + P('x'), '=', F('2n', 'x') + 'J_n' + P('x')] },
      { m: [dx('J_0' + P('x')), '=', '-J_1' + P('x')] }] },
    { h: 'Modified Bessel functions Iₙ, Kₙ', type: 'I_n(x), K_n(x)', domain: 'I: every x (x ≥ 0 for non-integer n). K: x > 0. Any real order n', k: 'bessel modified', items: [
      { m: ['I_n' + P('x'), '=', '\\sum_{m=0}^{\\infty}' + F('1', 'm!' + Gam('m+n+1')) + P(F('x', '2')) + '^{2m+n}', '=', 'i^{-n}J_n' + P('ix')], n: 'The definition' },
      { m: ['K_n' + P('x'), '=', F('\\pi', '2') + F('I_{-n}' + P('x') + '-I_n' + P('x'), sin('n\\pi'))], n: 'The definition (its limit at integer n)' },
      { m: ['x^2y\'\'+xy\'-' + P('x^2+n^2') + 'y', '=', '0'], n: 'The modified Bessel equation: y = Iₙ(x) and Kₙ(x) solve it' }] },
    { h: 'Airy functions Ai, Bi', type: 'Ai(x), Bi(x)', domain: 'Every x', items: [
      { m: [op('Ai') + P('x'), '=', F('1', '\\pi') + DI('0', inf, cos(F('t^3', '3') + '+xt'))], n: 'The definition' },
      { m: [op('Bi') + P('x'), '=', F('1', '\\pi') + DI('0', inf, B('e^{-' + F('t^3', '3') + '+xt}+' + sin(F('t^3', '3') + '+xt')))], n: 'The definition' },
      { m: ['y\'\'', '=', 'xy'], n: 'Airy’s equation: y = Ai(x) and Bi(x) solve it' },
      { m: [op('Ai') + P('0'), '=', F('1', '3^{\\frac{2}{3}}' + Gam(F('2', '3')))] },
      { m: [op('Bi') + P('0'), '=', F('1', '3^{\\frac{1}{6}}' + Gam(F('2', '3')))] }] }
  ];

  // Statistics: the formulas behind the statistics functions, as a course
  // writes them.
  var xb = '\\bar{x}', ss = '\\sum_{i=1}^{n}', mu = '\\mu', sg = '\\sigma';
  var STATISTICS = [
    { h: 'Summaries', type: 'mean(L), stdev(L), var(L), mad(L), quartile(L, q)', k: 'average deviation spread', items: [
      { m: [xb, '=', F('1', 'n') + ss + 'x_i'], n: 'Mean' },
      { m: ['s^2', '=', F('1', 'n-1') + ss + P('x_i-' + xb) + '^2'], n: 'Sample variance, var; s = stdev' },
      { m: [sg + '^2', '=', F('1', 'n') + ss + P('x_i-' + mu) + '^2'], n: 'Population variance, varp; σ = stdevp' },
      { m: ['\\text{MAD}', '=', F('1', 'n') + ss + A('x_i-' + xb)], n: 'Mean absolute deviation, mad' },
      { m: ['\\text{IQR}', '=', 'Q_3-Q_1'], n: 'Outliers lie beyond Q₁ − 1.5 IQR or Q₃ + 1.5 IQR' },
      { m: ['z', '=', F('x-' + mu, sg)], n: 'z-score' }] },
    { h: 'Correlation and regression', type: 'corr(X, Y), cov(X, Y), y_1 ~ mx_1 + b', k: 'least squares line slope', items: [
      { m: ['r', '=', F(ss + P('x_i-' + xb) + P('y_i-\\bar{y}'), R(ss + P('x_i-' + xb) + '^2' + ss + P('y_i-\\bar{y}') + '^2'))], n: "Pearson's r, corr" },
      { m: ['m', '=', 'r' + F('s_y', 's_x')], n: 'Least-squares slope' },
      { m: ['b', '=', '\\bar{y}-m' + xb], n: 'Intercept: the line passes through the means' },
      { m: ['R^2', '=', '1-' + F(ss + P('y_i-\\hat{y}_i') + '^2', ss + P('y_i-\\bar{y}') + '^2')], n: 'Share of the variation the model explains' }] },
    { h: 'Probability rules', k: 'complement union conditional independent bayes', items: [
      { m: ['P' + P('A^c'), '=', '1-P' + P('A')] },
      { m: ['P' + P('A\\cup B'), '=', 'P' + P('A') + '+P' + P('B') + '-P' + P('A\\cap B')] },
      { m: ['P' + P('A|B'), '=', F('P' + P('A\\cap B'), 'P' + P('B'))], n: 'Conditional probability' },
      { m: ['P' + P('A|B'), '=', F('P' + P('B|A') + 'P' + P('A'), 'P' + P('B'))], n: "Bayes' rule" },
      { m: [op('nCr') + P('n,k'), '=', F('n!', 'k!' + P('n-k') + '!')], n: 'Combinations' }] },
    { h: 'Distributions', type: 'normaldist(μ, σ), binomialdist(n, p), poissondist(λ), …', k: 'pdf pmf density bell', items: [
      { m: ['f' + P('x'), '=', F('1', sg + R('2\\pi')) + 'e^{-' + F(P('x-' + mu) + '^2', '2' + sg + '^2') + '}'], n: 'Normal density' },
      { m: ['P' + P('X=k'), '=', op('nCr') + P('n,k') + 'p^k' + P('1-p') + '^{n-k}'], n: 'Binomial: mean np, variance np(1 − p)' },
      { m: ['P' + P('X=k'), '=', F('\\lambda^ke^{-\\lambda}', 'k!')], n: 'Poisson: mean and variance λ' },
      { m: ['P' + P('X=k'), '=', P('1-p') + '^{k-1}p'], n: 'Geometric (geodist): the first success on trial k; mean 1/p' },
      { m: ['f' + P('x'), '=', '\\lambda e^{-\\lambda x}'], n: 'Exponential (expdist), x ≥ 0; mean 1/λ' },
      { table: { cols: ['#Within', '#Share'], rows: [['#μ ± σ', '#68.27%'], ['#μ ± 2σ', '#95.45%'], ['#μ ± 3σ', '#99.73%']] } }] },
    { h: 'Sampling and inference', type: 'ttest(L, μ₀), tinterval(L, C), proptest(x, n, p₀), …', k: 'hypothesis test confidence interval standard error', items: [
      { m: ['\\text{SE}', '=', F('s', R('n'))], n: 'Standard error of a mean' },
      { m: ['t', '=', F(xb + '-' + mu + '_0', 's/' + R('n'))], n: 'One-sample t, with n − 1 degrees of freedom' },
      { m: ['z', '=', F('\\hat{p}-p_0', R(F('p_0' + P('1-p_0'), 'n')))], n: 'One-proportion z' },
      { m: [xb + '\\pm t^*' + F('s', R('n'))], n: 'Confidence interval for a mean' },
      { m: ['\\hat{p}\\pm z^*' + R(F('\\hat{p}' + P('1-\\hat{p}'), 'n'))], n: 'Confidence interval for a proportion' },
      { m: ['\\chi^2', '=', '\\sum' + F(P('O-E') + '^2', 'E')], n: 'Chi-square statistic' }] }
  ];

  // Statistics tools: every statistics function the calculator has, as it
  // is typed. Click one to put it in a row. L and M are lists.
  function c(name) {
    var args = [].slice.call(arguments, 1).join(',');
    return op(name) + P(args);
  }
  var L1 = B('1,2,3,4');
  var TOOLS = [
    { h: 'List summaries', k: 'statistics average spread center count', items: [
      { m: [c('mean', 'L')], n: 'Mean' },
      { m: [c('median', 'L')], n: 'Median' },
      { m: [c('stdev', 'L')], n: 'Sample standard deviation (stdevp: population)' },
      { m: [c('var', 'L')], n: 'Sample variance (varp: population)' },
      { m: [c('mad', 'L')], n: 'Mean absolute deviation' },
      { m: [c('quartile', 'L', '1')], n: 'Quartile 0 to 4' },
      { m: [c('quantile', 'L', '0.9')], n: 'Quantile between 0 and 1' },
      { m: [c('count', 'L')], n: 'Number of items (same as length)' },
      { m: [c('total', 'L')], n: 'Sum' },
      { m: [c('tscore', 'L', 'x')], n: 'How many stdevs x is from the mean' },
      { m: [c('stats', 'L')], n: 'On its own row: a table of min, Q1, median, Q3, max, mean, stdev, …' }] },
    { h: 'Two lists', k: 'correlation covariance relationship', items: [
      { m: [c('corr', 'L', 'M')], n: "Pearson's r" },
      { m: [c('spearman', 'L', 'M')], n: 'Rank correlation' },
      { m: [c('cov', 'L', 'M')], n: 'Sample covariance (covp: population)' },
      { m: ['y_1\\sim mx_1+b'], n: 'Regression with R²; a table’s Statistics… menu offers eight models' }] },
    { h: 'Distributions', k: 'probability pdf cdf normal binomial poisson', items: [
      { m: [c('normaldist', '\\mu', '\\sigma')], n: 'Normal; on its own row it draws the curve and offers Find Cumulative Probability' },
      { m: [c('tdist', '\\nu')], n: "Student's t" },
      { m: [c('binomialdist', 'n', 'p')], n: 'Binomial (drawn as bars)' },
      { m: [c('poissondist', '\\lambda')], n: 'Poisson (bars)' },
      { m: [c('uniformdist', 'a', 'b')], n: 'Uniform' },
      { m: [c('chisqdist', 'k')], n: 'Chi-square' },
      { m: [c('fdist', 'd_1', 'd_2')], n: 'F' },
      { m: [c('expdist', '\\lambda')], n: 'Exponential' },
      { m: [c('geodist', 'p')], n: 'Geometric (bars)' },
      { m: [c('gammadist', 'k', '\\theta')], n: 'Gamma' },
      { m: [c('betadist', '\\alpha', '\\beta')], n: 'Beta' }] },
    { h: 'Using a distribution', k: 'probability density cumulative inverse', type: 'D = normaldist(0, 1) on its own row', items: [
      { m: ['D.' + c('pdf', 'x')], n: 'Density (or P(X = x) for a discrete one)' },
      { m: ['D.' + c('cdf', 'x')], n: 'P(X ≤ x)' },
      { m: ['D.' + c('cdf', 'a', 'b')], n: 'P(a ≤ X ≤ b)' },
      { m: [c('quantile', 'D', '0.975')], n: 'Inverse cdf' },
      { m: [c('mean', 'D')], n: 'Mean (also median, stdev, var)' },
      { m: ['D.' + c('random', '10')], n: '10 random draws' }] },
    { h: 'Random', k: 'sample shuffle seed rerandomize', n: 'The ⚄ button on a row draws new numbers', items: [
      { m: [c('random')], n: 'A number from 0 to 1' },
      { m: [c('random', '5')], n: 'A list of 5' },
      { m: [c('random', '5', 's')], n: 'With a seed s, the same draws every time' },
      { m: [c('random', 'L')], n: 'One item picked from L (random(L, n) picks n)' },
      { m: [c('shuffle', 'L')], n: 'L in random order' }] },
    { h: 'Plots', k: 'histogram dotplot boxplot bins bars chart', n: 'Each goes on a row of its own; the row has options for bins, heights and offset', items: [
      { m: [c('histogram', 'L')], n: 'Histogram, bin width picked for you' },
      { m: [c('histogram', 'L', '2')], n: 'Bin width 2' },
      { m: [c('histogram', c('freq', 'L', 'M'))], n: 'Values L with frequencies M' },
      { m: [c('dotplot', 'L')], n: 'Dot plot' },
      { m: [c('boxplot', 'L')], n: 'Box plot, outliers as dots' },
      { m: [c('freq', 'L', 'M')], n: 'A frequency table: L repeated M times' }] },
    { h: 'Hypothesis tests', k: 'significance p-value null alternative tail', n: 'On a row of their own; pick the tail (<, ≠, >) under the table', items: [
      { m: [c('ttest', 'L', '\\mu_0')], n: 'One-sample t test (for paired data, test the differences: ttest(L − M, 0))' },
      { m: [c('ittest', 'L', 'M')], n: 'Two independent samples (Welch)' },
      { m: [c('ztest', 'L', '\\mu_0', '\\sigma')], n: 'z test with known σ' },
      { m: [c('proptest', 'x', 'n', 'p_0')], n: 'One proportion: x successes in n' },
      { m: [c('twoproptest', 'x_1', 'n_1', 'x_2', 'n_2')], n: 'Two proportions' },
      { m: [c('chisqgof', 'O', 'E')], n: 'Chi-square goodness of fit' },
      { m: [c('chisqtest', 'R_1', 'R_2')], n: 'Chi-square independence: one list of counts per row of the table' }] },
    { h: 'Confidence intervals', k: 'interval estimate margin level', items: [
      { m: [c('tinterval', 'L', '0.95')], n: 'Mean, σ unknown' },
      { m: [c('ittinterval', 'L', 'M', '0.95')], n: 'Difference of two means' },
      { m: [c('zinterval', 'L', '\\sigma', '0.95')], n: 'Mean, σ known' },
      { m: [c('propinterval', 'x', 'n', '0.95')], n: 'One proportion' },
      { m: [c('twopropinterval', 'x_1', 'n_1', 'x_2', 'n_2', '0.95')], n: 'Two proportions' }] },
    { h: 'Tables of data', k: 'paste excel sheets csv import', items: [
      { table: { cols: ['#To', '#Do'], rows: [
        ['#Paste from Excel or Sheets', '#Ctrl+V in a table cell or an empty row'],
        ['#Open a CSV file', '#☰ then Open CSV as table…'],
        ['#Summarize a column', '#The Σ stats button, or the table’s Statistics… menu'],
        ['#Plot or fit a column', '#The table’s Statistics… menu']] } }] }
  ];

  // Proof symbols: each one with the ; shortcut that types it, read from the
  // Shortcuts list when the panel is built so renamed ones show as renamed.
  var PROOF_EXAMPLES = {
    ';bcup': '\\bigcup_{i=1}^{n}A_i', ';bcap': '\\bigcap_{i=1}^{n}A_i',
    ';pow': '\\mathcal{P}\\left(A\\right)', ';card': '\\left|A\\right|',
    ';set': '\\left\\{x:P\\left(x\\right)\\right\\}',
    ';sup': '\\sup_{x\\in S}', ';lub': '\\sup_{x\\in S}', ';glb': '\\inf_{x\\in S}',
    ';lsup': '\\limsup_{n\\to \\infty }a_n', ';linf': '\\liminf_{n\\to \\infty }a_n',
    ';cl': '\\overline{A}', ';cmp': 'A^{c}', ';itr': 'A^{\\circ}'
  };
  var PROOF_GROUPS = ['Number systems', 'Logic', 'Sets', 'Analysis'];
  var PROOF_NOTES = {
    'Number systems': 'ℕ starts at 1. Add _0, ^+, ^- or ^n after one: ℕ_0, ℤ^+, ℝ^n. ℕ ℤ ℚ ℝ ℙ ℂ work in rows; the rest are for writing',
    'Logic': 'A row with ∀ or ∃ shows true or false, checked with numbers (ℕ and ℤ up to 10,000) and naming a counterexample or a witness. Solve tries a proof first',
    'Sets': 'In rows, a list is a set: A∪B, A∩B and A∖B give sorted lists without repeats, and 3∈A is true or false. |A| stays the absolute value; length(A) is the size'
  };
  var PROOF_EXTRA = {
    'Number systems': [
      { m: ['\\mathbb{N}_0'], n: 'Natural numbers with 0' },
      { m: ['\\mathbb{Z}^+'], n: 'Positive integers' },
      { m: ['\\mathbb{Z}_n'], n: 'Integers mod n' },
      { m: ['\\mathbb{Q}^+'], n: 'Positive rationals' },
      { m: ['\\mathbb{R}^+'], n: 'Positive reals (ℝ^- negative)' },
      { m: ['\\mathbb{R}^n'], n: 'n-dimensional space' },
      { m: ['\\mathbb{R}\\setminus \\mathbb{Q}'], n: 'Irrationals' }],
    'Logic': [
      { m: ['\\Rightarrow '], n: 'Implies', t: ';=>' },
      { m: ['\\Leftrightarrow '], n: 'If and only if', t: ';<=>' },
      { m: ['\\forall \\epsilon >0\\exists \\delta >0:\\delta \\le \\frac{\\epsilon }{2}'], n: 'Quantifiers nest; a bound like ε>0 is the set it runs over' },
      { m: ['\\forall n\\in \\mathbb{N}:\\sum_{k=1}^{n}k=\\frac{n\\left(n+1\\right)}{2}'], n: 'Press Solve to prove it' }],
    'Sets': [
      { m: ['y=x^2\\left\\{x\\in \\left[0,2\\right]\\right\\}'], n: 'Restrict a graph to an interval (any brackets: (0,2], [0,2), …)' },
      { m: ['y=x^2\\left\\{x\\in \\mathbb{Z}\\right\\}'], n: 'Over ℕ, ℤ or ℙ the graph is dots' },
      { m: ['x^2=4,x\\in \\mathbb{N}'], n: 'Solve keeps only answers in the set (or put x∈ℕ on its own row)' },
      { m: ['3x+5y=1,x,y\\in \\mathbb{Z}'], n: 'Solve in whole numbers' }]
  };

  function proofGroups() {
    var all = (window.shortcuts && window.shortcuts.list()) || [];
    var groups = PROOF_GROUPS.map(function (name) {
      var items = all.filter(function (s) { return s.group === name; }).map(function (s) {
        var key = s.base || s.trigger;
        var latex = PROOF_EXAMPLES[key] || s.latex.split(window.shortcuts.SLOT).join('A');
        return { m: [latex], n: s.note + '   ·   type ' + s.trigger };
      });
      (PROOF_EXTRA[name] || []).forEach(function (e) {
        var found = e.t && all.filter(function (s) { return (s.base || s.trigger) === e.t; })[0];
        items.push({ m: e.m, n: e.n + (found ? '   ·   type ' + found.trigger : '') });
      });
      return { h: name, k: 'proof real analysis symbol', n: PROOF_NOTES[name], items: items };
    });
    var blocks = all.filter(function (s) { return s.group === 'Proofs'; });
    groups.push({ h: 'Proof blocks', k: 'proof theorem lemma definition corollary qed', n: 'In a document, type one at the start of a text line. Theorem, Lemma, Definition and Corollary share one count. Enter on an empty line at the end leaves the block; Backspace at its start removes it',
      items: blocks.map(function (s) { return { text: s.trigger + '   ' + s.note }; }) });
    return groups;
  }

  var SECTIONS = [
    { title: 'Trig basics', groups: TRIG },
    { title: 'Trig identities', groups: IDENTITIES },
    { title: 'Unit circle', groups: UNIT_CIRCLE },
    { title: 'Inverse trig', groups: INVERSE },
    { title: 'Triangles', groups: TRIANGLES },
    { title: 'Hyperbolic', groups: HYPERBOLIC },
    { title: 'Derivatives', groups: DERIVATIVES },
    { title: 'Integrals', groups: INTEGRALS },
    { title: 'Series', groups: SERIES },
    { title: 'Convergence tests', groups: CONVERGENCE },
    { title: 'Special functions', groups: SPECIAL },
    { title: 'Statistics', groups: STATISTICS },
    { title: 'Statistics tools', groups: TOOLS },
    { title: 'Proof symbols', groups: proofGroups }
  ];

  // ---- The panel -------------------------------------------------------------

  var panel = document.getElementById('reference');
  var body = panel.querySelector('.ref-body');
  var search = panel.querySelector('.ref-search');
  var jump = panel.querySelector('.ref-jump');
  var built = false;
  var entries = []; // { el, text } for search

  function esc(s) {
    return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }

  // The words a formula is found by: its LaTeX without the backslashes.
  function words(latex) {
    return latex.replace(/\\operatorname\{([^}]*)\}/g, ' $1 ').replace(/\\(left|right|frac|sqrt|text)/g, ' ')
      .replace(/\\/g, ' ').replace(/[{}^_]/g, ' ');
  }

  // A formula's whole LaTeX: its sides with the relations between them.
  function whole(m) {
    return m.map(function (s, i) { return i % 2 ? s + ' ' : s; }).join('');
  }

  function mathSpan(latex, cls) {
    var span = document.createElement('span');
    span.className = cls;
    span.dataset.latex = latex;
    span.textContent = latex;
    return span;
  }

  function buildItem(item) {
    var el = document.createElement('div');
    var text = '';
    if (item.text) {
      el.className = 'ref-text';
      el.textContent = item.text;
      return { el: el, text: item.text };
    }
    if (item.table) {
      el.className = 'ref-table-wrap';
      var table = document.createElement('table');
      table.className = 'ref-table';
      var head = document.createElement('tr');
      item.table.cols.forEach(function (c) {
        var cell = document.createElement('th');
        if (c[0] === '#') cell.textContent = c.slice(1);
        else cell.appendChild(mathSpan(c, 'ref-static'));
        head.appendChild(cell);
        text += ' ' + (c[0] === '#' ? c.slice(1) : words(c));
      });
      table.appendChild(head);
      item.table.rows.forEach(function (r) {
        var tr = document.createElement('tr');
        r.forEach(function (c) {
          var td = document.createElement('td');
          if (c[0] === '#') td.textContent = c.slice(1);
          else td.appendChild(mathSpan(c, 'ref-side'));
          tr.appendChild(td);
          text += ' ' + (c[0] === '#' ? c.slice(1) : words(c));
        });
        table.appendChild(tr);
      });
      el.appendChild(table);
      return { el: el, text: text };
    }
    el.className = 'ref-item';
    var line = document.createElement('div');
    line.className = 'ref-line';
    item.m.forEach(function (part, i) {
      line.appendChild(mathSpan(part, i % 2 ? 'ref-rel' : 'ref-side'));
      text += ' ' + words(part);
    });
    el.appendChild(line);
    if (item.m.length > 1) {
      var all = document.createElement('button');
      all.className = 'ref-all';
      all.title = 'Insert the whole formula';
      all.textContent = '+';
      all.dataset.latex = whole(item.m);
      el.appendChild(all);
    }
    // The series written out term by term, for reading only.
    if (item.ex) {
      var ex = document.createElement('div');
      ex.className = 'ref-expanded';
      ex.appendChild(mathSpan('=' + item.ex, 'ref-static'));
      el.appendChild(ex);
      text += ' ' + words(item.ex);
    }
    if (item.n) {
      var note = document.createElement('div');
      note.className = 'ref-note';
      note.textContent = item.n;
      el.appendChild(note);
      text += ' ' + item.n;
    }
    return { el: el, text: text };
  }

  function build() {
    built = true;
    SECTIONS.forEach(function (s, si) {
      var sec = document.createElement('section');
      sec.className = 'ref-section';
      sec.id = 'ref-s' + si;
      var h = document.createElement('h2');
      h.textContent = s.title;
      sec.appendChild(h);
      var chip = document.createElement('button');
      chip.textContent = s.title;
      chip.dataset.section = sec.id;
      jump.appendChild(chip);
      var groups = typeof s.groups === 'function' ? s.groups() : s.groups;
      groups.forEach(function (g) {
        var group = document.createElement('div');
        group.className = 'ref-group';
        var gh = document.createElement('h3');
        gh.textContent = g.h;
        group.appendChild(gh);
        var label = s.title + ' ' + g.h + ' ' + (g.k || '') + ' ' + (g.type || '');
        var info = [];
        if (g.type) info.push('<div class="ref-info"><span>Type</span><code>' + esc(g.type) + '</code></div>');
        if (g.domain) info.push('<div class="ref-info"><span>Domain</span>' + esc(g.domain) + '</div>');
        if (g.n) info.push('<div class="ref-info plain">' + esc(g.n) + '</div>');
        if (info.length) group.insertAdjacentHTML('beforeend', info.join(''));
        g.items.forEach(function (item) {
          var made = buildItem(item);
          group.appendChild(made.el);
          entries.push({ el: made.el, group: group, section: sec, text: (label + ' ' + made.text).toLowerCase() });
        });
        sec.appendChild(group);
      });
      body.appendChild(sec);
    });
    body.querySelectorAll('[data-latex]').forEach(function (span) {
      if (span.tagName === 'BUTTON') return;
      try { MQ.StaticMath(span); } catch (e) { /* Left as its LaTeX. */ }
    });
  }

  function filter() {
    var query = search.value.toLowerCase().split(/\s+/).filter(Boolean);
    var shownGroups = new Set(), shownSections = new Set();
    entries.forEach(function (e) {
      var show = query.every(function (w) { return e.text.indexOf(w) >= 0; });
      e.el.style.display = show ? '' : 'none';
      if (show) { shownGroups.add(e.group); shownSections.add(e.section); }
    });
    body.querySelectorAll('.ref-group').forEach(function (g) { g.style.display = shownGroups.has(g) ? '' : 'none'; });
    body.querySelectorAll('.ref-section').forEach(function (s) { s.style.display = shownSections.has(s) ? '' : 'none'; });
    panel.querySelector('.ref-empty').style.display = shownSections.size ? 'none' : '';
  }

  // ---- Inserting -------------------------------------------------------------

  // The math field typed in last, in the graph list and in the document.
  var lastGraph = null, lastDoc = null;
  document.addEventListener('focusin', function (e) {
    var f = e.target.closest && e.target.closest('.mq-editable-field');
    if (!f || panel.contains(f)) return;
    if (f.closest('#doc')) lastDoc = f; else lastGraph = f;
  }, true);

  function docView() { return document.body.classList.contains('doc-view'); }

  function targetField() {
    var el = docView() ? lastDoc : lastGraph;
    if (el && document.body.contains(el)) return MQ(el);
    if (!docView() && window.panel && window.panel.currentField) return window.panel.currentField();
    return null;
  }

  function insert(latex) {
    var mf = targetField();
    if (!mf) {
      flash(docView() ? 'Click into a math line first' : 'Click into an expression first');
      return;
    }
    mf.focus();
    mf.write(latex);
  }

  var flashTimer = null;
  function flash(text) {
    var f = panel.querySelector('.ref-flash');
    f.textContent = text;
    f.classList.add('on');
    clearTimeout(flashTimer);
    flashTimer = setTimeout(function () { f.classList.remove('on'); }, 1600);
  }

  panel.addEventListener('mousedown', function (e) {
    var chip = e.target.closest('.ref-jump button');
    if (chip) {
      e.preventDefault();
      var target = document.getElementById(chip.dataset.section);
      if (target) body.scrollTop = target.offsetTop;
      return;
    }
    if (e.target.closest('.ref-close')) { e.preventDefault(); show(false); return; }
    var hit = e.target.closest('.ref-all') || e.target.closest('.ref-side');
    if (hit && hit.dataset.latex) {
      // Keep the focus (and cursor) in the math field being typed in.
      e.preventDefault();
      insert(hit.dataset.latex);
      return;
    }
    if (e.target !== search) e.preventDefault();
  });

  search.addEventListener('input', filter);
  search.addEventListener('keydown', function (e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      if (search.value) { search.value = ''; filter(); } else show(false);
    }
  });

  // ---- Opening and closing ---------------------------------------------------

  function isOpen() { return document.body.classList.contains('ref-open'); }

  function show(open) {
    if (open === isOpen()) return;
    if (open && !built) build();
    document.body.classList.toggle('ref-open', open);
    if (window.panel) window.panel.send({ type: 'reference', open: open });
    document.querySelectorAll('.ref-toggle').forEach(function (b) { b.classList.toggle('on', open); });
    if (open) {
      search.focus();
      search.select();
    } else if (panel.contains(document.activeElement)) {
      var mf = targetField();
      if (mf) mf.focus();
    }
  }

  document.querySelectorAll('.ref-toggle').forEach(function (b) {
    b.addEventListener('mousedown', function (e) { e.preventDefault(); show(!isOpen()); });
  });

  window.addEventListener('keydown', function (e) {
    var ctrl = e.ctrlKey || e.metaKey;
    if (ctrl && e.altKey && !e.shiftKey && e.key.toLowerCase() === 'r') {
      e.preventDefault();
      e.stopPropagation();
      show(!isOpen());
    } else if (e.key === 'Escape' && isOpen() && !ctrl && !e.altKey &&
        !document.querySelector('.overlay.open')) {
      e.preventDefault();
      e.stopPropagation();
      show(false);
    }
  }, true);

  // Typesetting a thousand formulas takes most of a second, so it happens
  // once the app has settled rather than when the panel is first opened.
  setTimeout(function () { if (!built) build(); }, 2500);

  window.reference = { show: show, isOpen: isOpen, sections: SECTIONS, whole: whole };
})();
