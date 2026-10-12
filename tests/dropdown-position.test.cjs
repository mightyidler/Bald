const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const test = require('node:test');

const source = fs.readFileSync('web/app.js', 'utf8');
const dropdownCode = source.slice(source.indexOf('function toggleDropdown('), source.indexOf('const pickerDrag='));

function openMenu(id, itemCount, triggerTop, viewportHeight, padding = 16) {
  const classes = new Set(id === 'drag-rule' ? ['app-drag-control'] : []);
  const classList = {
    contains: name => classes.has(name),
    toggle: (name, enabled) => enabled ? classes.add(name) : classes.delete(name),
  };
  const menu = { style: {}, children: [1], offsetWidth: 160, querySelectorAll: () => Array(itemCount), classList };
  const trigger = {
    getBoundingClientRect: () => ({ top: triggerTop, bottom: triggerTop + 24, left: 200 }),
    setAttribute() {},
  };
  const wrap = { id, classList, querySelector: selector => selector === '.dropdown-menu' ? menu : trigger };
  const main = {};
  const context = {
    openDD: null,
    innerHeight: viewportHeight,
    innerWidth: 400,
    getComputedStyle: element => {
      assert.equal(element, main);
      return { paddingLeft: `${padding}px` };
    },
    $: selector => ({
      '.app': { getBoundingClientRect: () => ({ right: 396, bottom: viewportHeight - 4 }) },
      '.header': { getBoundingClientRect: () => ({ bottom: 74 }) },
      '.main': main,
      '#overlay': { classList },
    })[selector],
  };
  vm.runInNewContext(dropdownCode, context);
  context.toggleDropdown(wrap);
  const height = Math.min(itemCount * 42 + 8, parseFloat(menu.style.maxHeight));
  const top = menu.style.top === 'auto'
    ? viewportHeight - parseFloat(menu.style.bottom) - height
    : parseFloat(menu.style.top);
  return { menu, top, height, opensUp: classes.has('opens-up') };
}

test('upward language and theme menus keep the page padding below the fixed header', () => {
  for (const [id, count, triggerTop, viewportHeight] of [
    ['languageDropdown', 12, 460, 700],
    ['themeDropdown', 3, 180, 260],
  ]) {
    const result = openMenu(id, count, triggerTop, viewportHeight);
    assert.equal(result.opensUp, true);
    assert.ok(result.top >= 74 + 16, `${id} starts at ${result.top}`);
    assert.ok(result.height > 0);
    assert.ok(result.top + result.height <= triggerTop - 8);
  }
});

test('menus near or behind the header open below its safe boundary', () => {
  for (const id of ['languageDropdown', 'themeDropdown', 'drag-rule']) {
    for (const triggerTop of [22, 70, 90]) {
      const result = openMenu(id, 12, triggerTop, 240);
      assert.equal(result.opensUp, false);
      // Downward entry translates by -6px, so the gap must also hold during motion.
      assert.ok(result.top - 6 >= 74 + 16, `${id} starts at ${result.top}`);
      assert.ok(result.top + result.height <= 240 - 4 - 16);
    }
  }
});

test('header clearance follows the resolved page padding', () => {
  const result = openMenu('languageDropdown', 12, 460, 700, 24);
  assert.equal(result.top, 74 + 24);
});

test('a menu with enough room still opens below its trigger', () => {
  const result = openMenu('themeDropdown', 3, 250, 700);
  assert.equal(result.opensUp, false);
  assert.equal(result.top, 250 + 24 + 8);
  assert.equal(result.height, 3 * 42 + 8);
});

test('downward menus reserve the common padding above the app bottom', () => {
  for (const padding of [16, 24]) {
    const result = openMenu('languageDropdown', 12, 90, 400, padding);
    assert.equal(result.opensUp, false);
    assert.equal(result.top + result.height, 400 - 4 - padding);
  }
});

test('upward menus near the app bottom keep clearance during entry motion', () => {
  for (const padding of [16, 24]) {
    for (const id of ['languageDropdown', 'themeDropdown', 'drag-rule']) {
      const result = openMenu(id, 3, 390, 400, padding);
      assert.equal(result.opensUp, true);
      assert.ok(result.top >= 74 + padding);
      assert.ok(result.top + result.height + 6 <= 400 - 4 - padding);
    }
  }
});
