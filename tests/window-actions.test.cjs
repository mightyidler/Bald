const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const test = require('node:test');

const source = fs.readFileSync('web/app.js', 'utf8');
const binding = source.slice(source.indexOf('function bindWindowAction('), source.indexOf('\nbindWindowAction($("#minimizeBtn")'));

function fixture(command = 'close_window', reduced = false, reject = false) {
  const listeners = new Map();
  const timers = new Map();
  const calls = [];
  const errors = [];
  let captured = null;
  let nextTimer = 0;
  const button = {
    dataset: {},
    addEventListener: (type, listener) => listeners.set(type, listener),
    removeEventListener: type => listeners.delete(type),
    setPointerCapture: id => { captured = id; },
  };
  const context = {
    matchMedia: () => ({ matches: reduced }),
    document: { documentElement: {} },
    getComputedStyle: () => ({ getPropertyValue: () => '220ms' }),
    setTimeout: callback => { timers.set(++nextTimer, callback); return nextTimer; },
    clearTimeout: id => timers.delete(id),
    invoke: command => { calls.push(command); return reject ? Promise.reject('hide failed') : Promise.resolve(); },
    reportWindowError: error => errors.push(error),
  };
  vm.runInNewContext(binding, context);
  context.bindWindowAction(button, command);
  return { button, listeners, timers, calls, errors, captured: () => captured };
}

for (const command of ['close_window', 'minimize_window']) {
  test(`${command} fires immediately without waiting for motion or a fallback`, () => {
    const view = fixture(command);
    view.button.onclick();
    assert.deepEqual(view.calls, [command]);
    assert.equal(view.timers.size, 0);
    assert.equal(view.listeners.has('transitionend'), false);
  });
}

test('reduced motion closes immediately and native errors are reported', async () => {
  const view = fixture('close_window', true, true);
  view.button.onclick();
  await Promise.resolve();
  assert.deepEqual(view.calls, ['close_window']);
  assert.deepEqual(view.errors, ['hide failed']);
});

test('press captures the button and does not reach header dragging', () => {
  const view = fixture();
  let stopped = 0;
  view.listeners.get('pointerdown')({ button: 0, pointerId: 42, stopPropagation: () => stopped++ });
  view.listeners.get('mousedown')({ stopPropagation: () => stopped++ });
  assert.equal(view.captured(), 42);
  assert.equal(stopped, 2);
  assert.doesNotMatch(fs.readFileSync('web/index.html', 'utf8'), /data-tauri-drag-region/);
});

test('native close and minimize reconcile visibility before their separate actions', () => {
  const native = fs.readFileSync('src/web_app.rs', 'utf8');
  assert.match(native, /fn close_window\([^]*?sync_visible_window\(&window\)\?;\s*window\.hide\(\)/);
  assert.match(native, /fn minimize_window\([^]*?sync_visible_window\(&window\)\?;\s*window\.minimize\(\)/);
  const sync = native.slice(native.indexOf('fn sync_visible_window('), native.indexOf('fn start_window_drag('));
  assert.match(sync, /if window\.is_visible\(\)[^]*?window\.show\(\)/);
  assert.doesNotMatch(sync, /set_focus|minimize\(\)/);
  assert.match(native, /WindowEvent::CloseRequested[^]*?close_window\(main\)/);
});
