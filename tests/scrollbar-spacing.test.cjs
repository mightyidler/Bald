const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const css = fs.readFileSync(path.join(__dirname, "../web/style.css"), "utf8");

test("main and picker never show a horizontal scrollbar", () => {
  assert.match(css, /\.main,\s*\.picker-list\{[^}]*overflow-x:hidden;/);
});

test("main and picker scrollbar tracks leave 16px at the bottom", () => {
  for (const selector of [".main", ".picker-list"]) {
    const start = css.indexOf(`${selector}::-webkit-scrollbar-track{`);
    assert.notEqual(start, -1);
    const rule = css.slice(start, css.indexOf("}", start));
    assert.match(rule, /margin-bottom:\s*16px;/);
  }
});
