const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const test = require('node:test');

const source = fs.readFileSync('web/app.js', 'utf8');
const css = fs.readFileSync('web/style.css', 'utf8');
const modalCode = source.slice(source.indexOf('const resetModal='), source.indexOf('\n', source.indexOf('function closeResetModal')));

test('reset focuses cancel without scrolling the app and can reopen during close', () => {
  const timers = new Map();
  let timerId = 0;
  let focusOptions;
  const element = () => {
    const classes = new Set();
    return {
      classes,
      classList: {
        add: (...names) => names.forEach(name => classes.add(name)),
        remove: (...names) => names.forEach(name => classes.delete(name)),
      },
      setAttribute() {},
    };
  };
  const card = element();
  const modal = element();
  modal.querySelector = () => card;
  const dim = element();
  const context = {
    $: selector => ({ '#resetModal': modal, '#dim': dim, '#resetCancel': {
      focus: options => { focusOptions = options; },
    } })[selector],
    document: { documentElement: {} },
    getComputedStyle: () => ({ getPropertyValue: () => '150ms' }),
    setTimeout: callback => { timers.set(++timerId, callback); return timerId; },
    clearTimeout: id => timers.delete(id),
  };
  vm.runInNewContext(modalCode, context);
  context.openResetModal();
  assert.equal(focusOptions.preventScroll, true);
  assert.equal(modal.classes.has('is-open'), true);
  context.closeResetModal();
  assert.equal(modal.classes.has('is-open'), false);
  assert.equal(card.classes.has('is-closing'), true);
  context.openResetModal();
  assert.equal(timers.size, 0);
  assert.equal(card.classes.has('is-closing'), false);
  assert.equal(dim.classes.has('is-reset'), true);
});

test('root cannot scroll hidden panels into view; closed surfaces stop painting after exit', () => {
  assert.match(css, /\.app\{overflow:clip\}/);
  for (const selector of ['.picker', '.dropdown-menu', '.modal-overlay']) {
    const escaped = selector.replace('.', '\\.');
    assert.match(css, new RegExp(`${escaped}\\{[^}]*visibility:hidden[^}]*visibility 0s`));
    assert.match(css, new RegExp(`${escaped}\\.is-open\\{[^}]*visibility:visible[^}]*transition-delay:0s`));
  }
});

test('reset background blur belongs to the dim overlay, not the scrollable main', () => {
  assert.match(css, /\.dim\.is-reset\{[^}]*backdrop-filter:blur\(8px\)/);
  assert.doesNotMatch(css, /\.app:has\(\.modal-overlay\.is-open\)>\.main/);
  assert.doesNotMatch(css, /\.header,\.main\{[^}]*filter/);
});
