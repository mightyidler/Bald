const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const css = fs.readFileSync(path.join(__dirname, "../web/style.css"), "utf8").replace(/\r\n/g, '\n');

test("all scrollable surfaces share the designed thumb and reserve its width", () => {
  const surfaces = ':is(.main,.picker-list,.dropdown-menu,.modal-card)';
  const rule = selector => {
    const start = css.indexOf(`${selector}{`);
    assert.notEqual(start, -1, selector);
    return css.slice(start, css.indexOf('}', start));
  };
  assert.match(rule(surfaces), /overflow-x:hidden;/);
  assert.match(rule(surfaces), /scrollbar-gutter:stable;/);
  assert.match(rule(`${surfaces}::-webkit-scrollbar`), /width:7px;/);
  assert.match(rule(`${surfaces}::-webkit-scrollbar-thumb`), /border-right:4px solid transparent;/);
  assert.match(rule(`${surfaces}::-webkit-scrollbar-thumb`), /background-clip:padding-box;/);
});

test("track endpoints use each surface's radius and account for overlaid headers", () => {
  for (const [selector, top, bottom] of [
    ['.main', 'calc(var(--header-height) + var(--window-radius))', 'var(--window-radius)'],
    ['.picker-list', 'calc(var(--picker-header-height) + var(--picker-radius-top))', 'var(--picker-radius-bottom)'],
    ['.dropdown-menu', 'var(--menu-radius)', 'var(--menu-radius)'],
    ['.modal-card', 'var(--modal-radius)', 'var(--modal-radius)']
  ]) {
    const start = css.indexOf(`${selector}{\n  --scrollbar-track-start:`);
    assert.notEqual(start, -1);
    const rule = css.slice(start, css.indexOf("}", start));
    assert.ok(rule.includes(`--scrollbar-track-start:${top};`));
    assert.ok(rule.includes(`--scrollbar-track-end:${bottom};`));
  }
});
