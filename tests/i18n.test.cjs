const assert = require('node:assert/strict');
const fs = require('node:fs');
const crypto = require('node:crypto');
const vm = require('node:vm');
const test = require('node:test');

const catalog = fs.readFileSync('web/i18n.js', 'utf8');
const source = fs.readFileSync('web/app.js', 'utf8');
const html = fs.readFileSync('web/index.html', 'utf8');

function translations() {
  const context = vm.createContext({ window: {} });
  vm.runInContext(catalog, context);
  return context.window.BaldI18n;
}

test('all 12 locales translate every UI key, including accessibility and dynamic messages', () => {
  const { languages, i18n } = translations();
  assert.equal(Object.keys(languages).length, 12);
  assert.deepEqual(Object.keys(i18n), Object.keys(languages));
  const expected = Object.keys(i18n.english).sort();
  for (const [locale, strings] of Object.entries(i18n)) {
    assert.deepEqual(Object.keys(strings).sort(), expected, locale);
    for (const key of expected) assert.ok(strings[key].trim(), `${locale}.${key}`);
    for (const match of html.matchAll(/data-i18n(?:-placeholder|-label)?="([^"]+)"/g)) {
      assert.ok(strings[match[1]], `${locale}.${match[1]}`);
    }
  }
  assert.equal(languages.portuguese_brazil.tag, 'pt-BR');
  assert.equal(languages.simplified_chinese.tag, 'zh-Hans');
  assert.equal(languages.traditional_chinese.tag, 'zh-Hant');
});

test('language changes update labels and selection while keeping the brand untouched', () => {
  const { languages, i18n } = translations();
  const items = Object.keys(languages).map(key => ({
    dataset: { language: key }, selected: false,
    classList: { toggle(name, value) { this.owner.selected = value; } },
    setAttribute() {}
  }));
  for (const item of items) item.classList.owner = item;
  const label = { dataset: { i18n: 'automatic' }, textContent: '' };
  const input = { dataset: { i18nPlaceholder: 'search' }, placeholder: '' };
  const close = { dataset: { i18nLabel: 'close' }, setAttribute(key, value) { this[key] = value; } };
  const value = { textContent: '' }, theme = { textContent: '' };
  const update = { disabled: false, textContent: '' };
  let pickerRenders = 0;
  const context = vm.createContext({
    languages, i18n, state: null, language: null, theme: 'system',
    document: {
      documentElement: { dataset: {}, lang: '' },
      querySelectorAll: selector => ({
        '[data-i18n]': [label], '[data-i18n-placeholder]': [input],
        '[data-i18n-label]': [close], '[data-language]': items, '[data-theme]': []
      })[selector]
    },
    $: selector => ({ '#languageValue': value, '#themeValue': theme, '#btnCheckUpdate': update,
      '#picker': { classList: { contains: () => true } } })[selector],
    renderPicker: () => { pickerRenders++; }
  });
  vm.runInContext(source.slice(source.indexOf('function applyTheme('), source.indexOf('function applicationIcon(')), context);
  for (const [key, locale] of Object.entries(languages)) {
    context.language = key;
    context.state = { config: { language: key } };
    context.applyLanguage();
    assert.equal(context.document.documentElement.lang, locale.tag);
    assert.equal(value.textContent, locale.name);
    assert.equal(label.textContent, i18n[key].automatic);
    assert.equal(input.placeholder, i18n[key].search);
    assert.equal(close['aria-label'], i18n[key].close);
    assert.deepEqual(items.filter(item => item.selected).map(item => item.dataset.language), [key]);
  }
  assert.equal(pickerRenders, 12);
  update.disabled = true;
  context.applyLanguage();
  assert.equal(update.textContent, i18n.turkish.checking);
  const brand = html.match(/<div class="logo-text"[^>]*>(.*?)<\/div>/)[1];
  assert.equal(brand, '<strong>Bald</strong><span>Border Ablation Layout Decoupling</span>');
  assert.doesNotMatch(brand, /data-i18n/);
});

test('language menu offers exactly 12 concrete languages without a system option', () => {
  const { languages, i18n } = translations();
  const menu = { innerHTML: '' };
  const context = vm.createContext({ languages, i18n, $: () => menu });
  vm.runInContext(source.slice(source.indexOf('function renderLanguageMenu('), source.indexOf('function applyLanguage(')), context);
  context.renderLanguageMenu();
  assert.equal((menu.innerHTML.match(/data-language=/g) || []).length, 12);
  assert.doesNotMatch(menu.innerHTML, /data-language="system"|systemLanguage/);
  for (const locale of Object.values(languages)) assert.ok(menu.innerHTML.includes(locale.name));
});

test('bundled fonts are verified WOFF2 assets and obsolete OTF preloads are removed', () => {
  const provenance = JSON.parse(fs.readFileSync('web/fonts/sources.json', 'utf8'));
  let fonts = 0;
  for (const asset of provenance.files) {
    const bytes = fs.readFileSync(`web/fonts/${asset.file}`);
    assert.equal(bytes.length, asset.bytes);
    assert.equal(crypto.createHash('sha256').update(bytes).digest('hex'), asset.sha256);
    if (asset.file.endsWith('.woff2')) {
      fonts++;
      assert.equal(bytes.subarray(0, 4).toString(), 'wOF2');
    }
  }
  assert.equal(fonts, 3);
  assert.doesNotMatch(html, /\.otf/);
  assert.equal(fs.readdirSync('web/figma').filter(name => name.endsWith('.otf')).length, 0);
});
