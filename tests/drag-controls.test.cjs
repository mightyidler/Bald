const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const source = fs.readFileSync(path.join(__dirname, "../web/app.js"), "utf8");
const prefix = source.slice(0, source.indexOf("function esc("));

function controls(invoke) {
  const errors = [];
  let refreshes = 0;
  const context = vm.createContext({
    window: { __TAURI__: { core: { invoke } } },
    reportWindowError: error => errors.push(error),
    refresh: async () => { refreshes++; },
    closeMenus: () => {}
  });

  vm.runInContext(`${prefix}\nglobalThis.controls = { renderDragControl, changeDragMode, i18n };`, context);

  return { ...context.controls, errors, refreshes: () => refreshes };
}

test("only allow and block are offered; missing and removed modes are blocked", () => {
  const ui = controls(async () => {});
  for (const translations of Object.values(ui.i18n)) {
    for (const drag_mode of [undefined, "auto", "disabled"]) {
      const html = ui.renderDragControl({ id: "rule", drag_mode }, translations);
      assert.match(html, /dropdown-item is-selected[^>]*data-drag-mode="disabled"/);
      assert.doesNotMatch(html, /dropdown-item is-selected[^>]*data-drag-mode="enabled"/);
      assert.equal((html.match(/role="menuitemradio"/g) || []).length, 2);
      assert.doesNotMatch(html, /<select|<option|data-drag-mode="auto"/);
      assert.match(html, /aria-label=/);
      assert.match(html, /class="chevron"/);
      assert.match(html, /class="dropdown-menu"/);
    }
  }
});

test("allow and block controls have no hover tooltip in either language", () => {
  const ui = controls(async () => {});
  for (const translations of Object.values(ui.i18n)) {
    for (const drag_mode of ["enabled", "disabled"]) {
      const html = ui.renderDragControl({ id: "rule", drag_mode }, translations);
      assert.doesNotMatch(html, /\btitle\s*=/);
      assert.match(html, /aria-label=/);
    }
  }
});

test("manual permission is saved separately from border removal", async () => {
  const calls = [];
  const ui = controls(async (command, args) => { calls.push([command, args.id, args.mode]); });
  const select = { dataset: { dragRule: "rule", dragMode: "enabled" }, disabled: false };
  await ui.changeDragMode(select);
  assert.deepEqual(calls, [["set_application_drag_mode", "rule", "enabled"]]);
  assert.equal(select.disabled, false);
  assert.equal(ui.refreshes(), 1);
});

test("failed permission changes reenable the control and refresh saved state", async () => {
  const ui = controls(async () => { throw new Error("save failed"); });
  const select = { dataset: { dragRule: "rule", dragMode: "disabled" }, disabled: false };
  await ui.changeDragMode(select);
  assert.equal(select.disabled, false);
  assert.equal(ui.errors[0].message, "save failed");
  assert.equal(ui.refreshes(), 1);
});
