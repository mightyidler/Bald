const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");

test("elevation starts a window-only service without replacing or touching the UI", () => {
  const source = fs.readFileSync("src/web_app.rs", "utf8");
  const command = source.slice(source.indexOf("async fn elevate_border_service("), source.indexOf("fn capture_ui_state("));
  assert.match(command, /spawn_blocking/);
  assert.match(command, /service\.elevate\(snapshot\)/);
  assert.doesNotMatch(command, /get_webview_window|app\.exit|capture_ui_state|\.hide\(|\.show\(|set_focus|restart_arguments/);
  const main = fs.readFileSync("src/main.rs", "utf8");
  assert.ok(main.indexOf("border_service::run_worker") < main.indexOf("SingleInstance::acquire"));
  assert.ok(main.indexOf("border_service::run_worker") < main.indexOf("launch_elevated_task"));
  const frontend = fs.readFileSync("web/app.js", "utf8");
  assert.match(frontend, /invoke\("elevate_border_service"\)/);
  assert.doesNotMatch(frontend, /restart_as_admin/);
});
