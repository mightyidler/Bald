const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const test = require('node:test');

const source = fs.readFileSync('web/app.js', 'utf8');
const closing = source.slice(source.indexOf('function clearPickerDrag('), source.indexOf('function refresh('));
const dragging = source.slice(source.indexOf('const pickerDrag='), source.indexOf('$("#pickerClose").onclick'));

function fixture(reduced = false) {
  let now = 0, nextFrame = 0, captured = null;
  const frames = new Map(), media = { matches: reduced };
  const classes = new Set(['is-open']), dimClasses = new Set(['is-open']);
  const classList = names => ({
    add: (...values) => values.forEach(value => names.add(value)),
    remove: (...values) => values.forEach(value => names.delete(value)),
    contains: value => names.has(value)
  });
  const properties = new Map();
  const picker = {
    offsetHeight: 600, classList: classList(classes), hidden: false,
    style: { setProperty: (name, value) => properties.set(name, value), removeProperty: name => properties.delete(name) },
    setAttribute: (name, value) => { if (name === 'aria-hidden') picker.hidden = value === 'true'; }
  };
  const dim = { classList: classList(dimClasses), style: { opacity: '' } };
  const head = {
    setPointerCapture: id => { captured = id; }, hasPointerCapture: id => captured === id,
    releasePointerCapture: () => { captured = null; }
  };
  const y = () => parseFloat(properties.get('--picker-drag-y') || '0');
  const context = vm.createContext({
    $: selector => selector === '#picker' ? picker : selector === '#dim' ? dim : head,
    document: { documentElement: {} }, performance: { now: () => now },
    matchMedia: () => media,
    DOMMatrixReadOnly: class { constructor(transform) { this.m42 = parseFloat(transform); } },
    getComputedStyle: element => element === picker ? { transform: String(y()) } : {
      getPropertyValue: name => name === '--duration-fast' ? '250ms' : '380ms'
    },
    requestAnimationFrame: callback => { const id = ++nextFrame; frames.set(id, callback); return id; },
    cancelAnimationFrame: id => frames.delete(id)
  });
  vm.runInContext(`${closing}\n${dragging}`, context);
  const event = (clientY, type = 'pointermove', pointerId = 1) => ({
    button: 0, clientY, type, pointerId, target: { closest: () => null }, currentTarget: head, preventDefault: () => {}
  });
  const advance = ms => {
    now += ms;
    const pending = [...frames];
    frames.clear();
    pending.forEach(([, callback]) => callback(now));
  };
  const settle = () => {
    for (let index = 0; index < 180 && frames.size; index++) advance(1000 / 60);
    assert.equal(frames.size, 0, 'motion must come to rest');
  };
  return { context, classes, dimClasses, dim, picker, frames, media, y, advance, settle,
    begin: at => context.beginPickerDrag(event(at, 'pointerdown')),
    move: at => context.movePickerDrag(event(at)),
    end: (at, type = 'pointerup') => context.endPickerDrag(event(at, type)),
    captured: () => captured };
}

test('release keeps the downward momentum of a short drag before returning to rest', () => {
  const ui = fixture();
  ui.begin(100);
  ui.advance(20);
  ui.move(124);
  ui.end(124);
  ui.advance(5);
  assert.ok(ui.y() > 24, 'the panel must continue with the release velocity');
  ui.settle();
  assert.equal(ui.classes.has('is-open'), true);
  assert.equal(ui.classes.has('is-dragging'), false);
  assert.equal(ui.dim.style.opacity, '');
});

test('catching a closing panel retains its visible position and cancels the old close', () => {
  const ui = fixture();
  ui.begin(100);
  ui.advance(100);
  ui.move(228);
  ui.end(228);
  ui.advance(16);
  const position = ui.y();
  ui.begin(200);
  assert.equal(ui.frames.size, 0);
  assert.equal(ui.y(), position);
  ui.advance(16);
  ui.move(200);
  assert.ok(Math.abs(ui.y() - position) < .001);
  ui.end(200, 'pointercancel');
  ui.settle();
  assert.equal(ui.classes.has('is-open'), true);
  assert.equal(ui.picker.hidden, false);
});

test('catching an upward resisted return does not apply the resistance twice', () => {
  const ui = fixture();
  ui.begin(100);
  ui.advance(50);
  ui.move(80);
  ui.end(80);
  ui.advance(50);
  const position = ui.y();
  assert.ok(position < 0);
  ui.begin(100);
  ui.advance(16);
  ui.move(100);
  assert.ok(Math.abs(ui.y() - position) < .001);
  ui.end(100);
  ui.settle();
});

test('holding after a flick removes stale throw velocity from the dismissal decision', () => {
  const ui = fixture();
  ui.begin(100);
  ui.advance(20);
  ui.move(160);
  ui.advance(100);
  ui.end(160);
  ui.settle();
  assert.equal(ui.classes.has('is-open'), true);
});

test('pointer cancellation returns even when the dismissal distance was crossed', () => {
  const ui = fixture();
  ui.begin(100);
  ui.advance(20);
  ui.move(260);
  ui.end(260, 'pointercancel');
  ui.settle();
  assert.equal(ui.classes.has('is-open'), true);
  assert.equal(ui.captured(), null);
});

test('an explicit close releases capture and cancels any unfinished return', () => {
  const ui = fixture();
  ui.begin(100);
  ui.context.closePicker();
  assert.equal(ui.captured(), null);
  assert.equal(ui.picker.hidden, true);
  ui.classes.add('is-open');
  ui.begin(100);
  ui.advance(20);
  ui.move(124);
  ui.end(124);
  assert.equal(ui.frames.size, 1);
  ui.context.closePicker();
  assert.equal(ui.frames.size, 0);
  assert.equal(ui.dimClasses.has('is-dragging'), false);
  ui.advance(1000);
  assert.equal(ui.classes.has('is-open'), false);
});

test('reduced motion dismisses immediately and also stops an in-progress return', () => {
  const closing = fixture(true);
  closing.begin(100);
  closing.advance(100);
  closing.move(228);
  closing.end(228);
  assert.equal(closing.picker.hidden, true);
  assert.equal(closing.frames.size, 0);
  const returning = fixture();
  returning.begin(100);
  returning.advance(20);
  returning.move(124);
  returning.end(124);
  returning.media.matches = true;
  returning.advance(16);
  assert.equal(returning.frames.size, 0);
  assert.equal(returning.classes.has('is-open'), true);
  assert.equal(returning.classes.has('is-dragging'), false);
});

test('the same elapsed time produces the same return at different frame rates', () => {
  const positions = [];
  for (const rate of [30, 60, 120]) {
    const ui = fixture();
    ui.begin(100);
    ui.advance(40);
    ui.move(124);
    ui.end(124);
    for (let frame = 0; frame < rate / 10; frame++) ui.advance(1000 / rate);
    positions.push(ui.y());
    ui.settle();
  }
  assert.ok(Math.max(...positions) - Math.min(...positions) < .001);
});
