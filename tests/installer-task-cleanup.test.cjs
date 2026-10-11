const assert = require('node:assert/strict');
const fs = require('node:fs');
const test = require('node:test');

test('uninstall retains the executable on task cleanup failure and preserves tasks during updates', () => {
  const template = fs.readFileSync('tools/windows/installer.nsi', 'utf8');
  const uninstall = template.slice(template.indexOf('Section Uninstall'));
  const remove = uninstall.indexOf('--remove-elevated-task');
  const deleteExe = uninstall.indexOf('Delete "$INSTDIR\\${MAINBINARYNAME}.exe"');
  assert.ok(remove > 0 && remove < deleteExe);
  const cleanup = uninstall.slice(0, deleteExe);
  assert.match(cleanup, /\$UpdateMode <> 1[^]*?--remove-elevated-task/);
  assert.match(cleanup, /\$0 != 0[^]*?SetErrorLevel 1\s+Abort/);
});
