const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const test = require('node:test');

const source = fs.readFileSync('web/app.js', 'utf8');
const reveal = source.slice(source.indexOf('let contentRevealGeneration ='), source.indexOf('// Keep content ready'));

function fixture(reduced = false) {
  const classes = new Set();
  const listeners = new Map(), timers = new Map(), frames = [];
  const last = {};
  const stack = {
    children: [{}, {}, {}, {}, last], lastElementChild: last, inert: false,
    classList: {
      add: (...names) => names.forEach(name => classes.add(name)),
      remove: (...names) => names.forEach(name => classes.delete(name)),
      contains: name => classes.has(name)
    },
    addEventListener: (name, callback) => listeners.set(name, callback),
    removeEventListener: name => listeners.delete(name)
  };
  let resolveFonts, nextTimer = 0;
  const ready = new Promise(resolve => { resolveFonts = resolve; });
  const loads = [];
  const context = vm.createContext({
    $: () => stack,
    document: { body: {}, fonts: { ready, load: font => { loads.push(font); return Promise.resolve(); } } },
    getComputedStyle: () => ({ fontFamily: '"Pretendard JP Variable", sans-serif',
      getPropertyValue: name => name === '--stagger-dur' ? '400ms' : '40ms' }),
    matchMedia: () => ({ matches: reduced }),
    requestAnimationFrame: callback => frames.push(callback),
    setTimeout: (callback, delay) => { const id = ++nextTimer; timers.set(id, { callback, delay }); return id; },
    clearTimeout: id => timers.delete(id)
  });
  vm.runInContext(reveal, context);
  return { context, stack, classes, listeners, timers, frames, loads, resolveFonts,
    flushFrames: () => { while (frames.length) frames.shift()(); } };
}

test('entrance waits for final font metrics and keeps all content inert until the last item finishes', async () => {
  const ui = fixture();
  const pending = ui.context.revealContent();
  assert.equal(ui.stack.inert, true);
  assert.equal(ui.classes.has('is-shown'), false);
  assert.equal(ui.frames.length, 0);
  ui.resolveFonts();
  await pending;
  assert.ok(ui.loads.every(font => font.includes('Pretendard JP Variable')));
  ui.flushFrames();
  assert.equal(ui.classes.has('is-shown'), true);
  assert.equal(ui.classes.has('is-settled'), false);
  assert.equal(ui.stack.inert, true);
  const finish = ui.listeners.get('transitionend');
  finish({ target: ui.stack.children[0], propertyName: 'filter' });
  assert.equal(ui.classes.has('is-settled'), false);
  finish({ target: ui.stack.lastElementChild, propertyName: 'filter' });
  assert.equal(ui.classes.has('is-settled'), true);
  assert.equal(ui.stack.inert, false);
  assert.equal(ui.timers.size, 0);
});

test('reopening during entrance cancels completion and immediately enables the prepared layout', async () => {
  const ui = fixture();
  ui.resolveFonts();
  await ui.context.revealContent();
  ui.flushFrames();
  assert.equal([...ui.timers.values()][0].delay, 640);
  const loads = ui.loads.length;
  await ui.context.revealContent();
  assert.equal(ui.timers.size, 0);
  assert.equal(ui.frames.length, 0);
  assert.equal(ui.loads.length, loads);
  assert.equal(ui.listeners.size, 0);
  assert.equal(ui.classes.has('is-settled'), true);
  assert.equal(ui.stack.inert, false);
});

test('a completed entrance stays interactive on repeated reopening without another font wait', async () => {
  const ui = fixture();
  ui.resolveFonts();
  await ui.context.revealContent();
  ui.flushFrames();
  [...ui.timers.values()][0].callback();
  const loads = ui.loads.length;
  for (let index = 0; index < 3; index++) await ui.context.revealContent();

  assert.equal(ui.stack.inert, false);
  assert.equal(ui.classes.has('is-settled'), true);
  assert.equal(ui.frames.length, 0);
  assert.equal(ui.timers.size, 0);
  assert.equal(ui.loads.length, loads);
});

test('overlapping font waits schedule only the latest entrance', async () => {
  const ui = fixture();
  const previous = ui.context.revealContent();
  const latest = ui.context.revealContent();
  ui.resolveFonts();
  await Promise.all([previous, latest]);
  assert.equal(ui.frames.length, 1);
  ui.flushFrames();
  assert.equal(ui.timers.size, 1);
});

test('reduced motion reveals the final layout without timers or animation frames', async () => {
  const ui = fixture(true);
  ui.resolveFonts();
  await ui.context.revealContent();
  assert.equal(ui.classes.has('is-settled'), true);
  assert.equal(ui.stack.inert, false);
  assert.equal(ui.timers.size, 0);
  assert.equal(ui.frames.length, 0);
});
