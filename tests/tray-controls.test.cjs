const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const source = fs.readFileSync(path.join(__dirname, "../src/web_app.rs"), "utf8");
const tray = source.slice(source.indexOf("TrayIconBuilder::new()"), source.indexOf(".build(app)?;"));

test("tray menu does not open on left click; only left double click opens Bald", () => {
  assert.match(tray, /\.show_menu_on_left_click\(false\)/);
  assert.match(tray, /TrayIconEvent::DoubleClick\s*\{\s*button: MouseButton::Left/);
  assert.doesNotMatch(tray, /TrayIconEvent::Click\s*\{/);
  assert.match(tray, /open_main_window\(tray\.app_handle\(\)\)/);
});

test("Open menu and double click share show, restore and focus handling", () => {
  assert.match(tray, /"show" => open_main_window\(app\)/);
  const open = source.slice(source.indexOf("fn open_main_window("), source.indexOf("pub fn run("));
  assert.match(open, /get_webview_window\("main"\)/);
  assert.match(open, /window\.show\(\);\s*let _ = window\.unminimize\(\);\s*let _ = window\.set_focus\(\);/);
});
