const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const test = require('node:test');
const source = fs.readFileSync('web/app.js', 'utf8');

function refreshFixture(invoke) {
  const errors = [];
  const timers = new Map();
  let timerId = 0, renders = 0, languages = 0;
  const automatic = { checked: false }, startup = { checked: false };
  const context = vm.createContext({
    invoke, reportWindowError: error => errors.push(error),
    applyLanguage: () => { languages++; }, renderApplications: async () => { renders++; },
    setTimeout: callback => { timers.set(++timerId, callback); return timerId; },
    clearTimeout: id => timers.delete(id),
    revealContent: async () => {},
    $: selector => selector === '#automaticToggle' ? automatic : selector === '#picker' ? {classList: {contains: () => false}} : startup
  });
  const refresh = source.slice(source.indexOf('function refresh('), source.indexOf('let openDD='));
  vm.runInContext(`let refreshRequest=null,state=null,language='korean',theme='system',initialized=false,elevationRequested=false,
    visibilityGeneration=0,uiVisible=false,refreshTimer=null,elevationRetry=false; ${refresh}
    globalThis.refresh = refresh; globalThis.savedState = () => state;`, context);
  return { refresh: context.refresh, savedState: context.savedState, setVisible: context.setUiVisibility, errors, automatic, startup,
    timers, renders: () => renders, languages: () => languages,
    fireNext: () => { const [id, callback] = timers.entries().next().value; timers.delete(id); return callback(); } };
}

function snapshot() {
  return { revision: 1, config: { language: 'english', theme: 'dark', automatic_application: true }, startupEnabled: true, statuses: {}, uiVisible: false };
}

test('configuration commands wait for stale polling before reading the committed state', async () => {
  let calls = 0, resolve;
  const view = refreshFixture(() => { calls++; return new Promise(done => { resolve = done; }); });
  const old = view.refresh(), committed = view.refresh(true);
  assert.equal(calls, 1);
  resolve(snapshot()); await old; await Promise.resolve();
  assert.equal(calls, 2);
  resolve({...snapshot(), revision: 2, config: {...snapshot().config, automatic_application: false}});
  await committed;
  assert.equal(view.automatic.checked, false);
});

test('unchanged status responses preserve settings and skip language and application rendering', async () => {
  let calls = 0;
  const view = refreshFixture(async () => ++calls === 1 ? snapshot() : {revision: 1, statuses: {a: 'running'}, uiVisible: false});
  await view.refresh(); await view.refresh();
  assert.equal(view.savedState().config.theme, 'dark');
  assert.equal(view.savedState().statuses.a, 'running');
  assert.equal(view.renders(), 1); assert.equal(view.languages(), 1);
});

test('hidden UI stops polling, reopening refreshes immediately, and slow requests never overlap', async () => {
  let calls = 0, resolve;
  const view = refreshFixture(() => { calls++; return new Promise(done => { resolve = done; }); });
  view.setVisible(true);
  assert.equal(calls, 1);
  resolve({...snapshot(), uiVisible: true});
  await view.refresh(); await Promise.resolve();
  assert.equal(view.timers.size, 1);
  const polling = view.fireNext();
  assert.equal(calls, 2);
  view.setVisible(false);
  resolve({...snapshot(), uiVisible: true}); // A stale reply cannot restart a hidden timer.
  await polling;
  assert.equal(view.timers.size, 0);
  view.setVisible(true);
  assert.equal(calls, 3);
  resolve({...snapshot(), uiVisible: true});
  await view.refresh(); await Promise.resolve();
  assert.equal(view.timers.size, 1);
  view.setVisible(false);
  assert.equal(view.timers.size, 0);
});

test('concurrent refresh callers share one IPC request and can refresh again afterward', async () => {
  let calls = 0, resolve;
  const view = refreshFixture(() => { calls++; return new Promise(done => { resolve = done; }); });
  const first = view.refresh();
  for (let index = 0; index < 5; index++) assert.equal(view.refresh(), first);
  assert.equal(calls, 1);
  resolve(snapshot());
  await first;
  const second = view.refresh();
  assert.equal(calls, 2);
  resolve(snapshot());
  await second;
});

test('failed state loading is handled and the next request restores authoritative controls', async () => {
  let fail = true;
  const view = refreshFixture(async () => {
    if (fail) throw new Error('connection lost');
    return snapshot();
  });
  assert.equal(await view.refresh(), null);
  assert.equal(view.savedState(), null);
  assert.equal(view.errors[0].message, 'connection lost');
  fail = false;
  await view.refresh();
  assert.equal(view.automatic.checked, true);
  assert.equal(view.startup.checked, true);
});

test('failed configuration commands report errors and refresh instead of rejecting', async () => {
  const errors = [];
  let refreshes = 0;
  const context = vm.createContext({
    invoke: async () => { throw new Error('save denied'); },
    refresh: async () => { refreshes++; }, reportWindowError: error => errors.push(error)
  });
  vm.runInContext(source.slice(source.indexOf('async function runConfigCommand('), source.indexOf('async function checkForUpdates(')), context);
  await context.runConfigCommand('set_theme', { theme: 'dark' });
  assert.equal(errors[0].message, 'save denied');
  assert.equal(refreshes, 1);
});

test('startup registration failure is reported once while saved controls remain usable', async () => {
  let calls = 0;
  const message = 'startup registration denied';
  const view = refreshFixture(async () => ++calls === 1 ? {
    revision: 1, config: {language: 'korean', theme: 'system', automatic_application: true},
    statuses: {}, startupEnabled: true, uiVisible: false, startupError: message
  } : {revision: 1, statuses: {}, startupEnabled: true, uiVisible: false, startupError: message});
  await view.refresh();
  await view.refresh();
  assert.deepEqual(view.errors, [message]);
  assert.equal(view.automatic.checked, true);
  assert.equal(view.startup.checked, true);
  assert.equal(view.renders(), 1);
});

test('failed picker registration clears its pending state without changing the button', async () => {
  const errors = [];
  const button = { dataset: {}, classList: { contains: () => false } };
  const context = vm.createContext({ invoke: async () => { throw new Error('window closed'); }, findAddedRule: () => null, reportWindowError: error => errors.push(error) });
  vm.runInContext(source.slice(source.indexOf('async function togglePickerApplication('), source.indexOf('function renderPicker(')), context);
  await context.togglePickerApplication(button, { hwnd: 1, processId: 2 });
  assert.equal(button.dataset.pending, undefined);
  assert.equal(errors[0].message, 'window closed');
});

test('failed picker loading leaves its panel closed and reports the error', async () => {
  const errors = [];
  const context = vm.createContext({ invoke: async () => { throw new Error('enumeration failed'); }, reportWindowError: error => errors.push(error) });
  vm.runInContext(source.slice(source.indexOf('async function openPicker('), source.indexOf('function clearPickerDrag(')), context);
  await context.openPicker();
  assert.equal(errors[0].message, 'enumeration failed');
});
