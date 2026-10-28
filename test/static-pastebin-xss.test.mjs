#!/usr/bin/env node
// Regression test for the shared-link script-injection reported in issue #13.
//
// The rule this pins: a paste field (title, id, CID, anything out of a
// `#paste=` fragment) is data. It may reach the DOM as a text node, and must
// never reach an HTML sink (innerHTML / insertAdjacentHTML) where quotes in a
// title break out of an attribute or an inline handler and become code.
//
// The app is a classic browser script, so it is evaluated here against a small
// DOM stub instead of a real browser. That is deliberate: the assertion is
// about which sink the value was handed to, which the stub records faithfully,
// and it needs no browser or network to run.
//
//   node --test test/static-pastebin-xss.test.mjs
//   APP_JS=/path/to/other/app.js node --test test/static-pastebin-xss.test.mjs

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const APP = process.env.APP_JS || path.join(HERE, '..', 'static-pastebin', 'app.js');

// --- minimal DOM -----------------------------------------------------------

class Node {
  constructor(tagName) {
    this.tagName = (tagName || '#text').toUpperCase();
    this.children = [];
    this.style = {};
    this.listeners = {};
    this.text = '';
    this.html = ''; // every string ever handed to an HTML sink
  }
  appendChild(child) { this.children.push(child); child.parent = this; return child; }
  append(...nodes) {
    for (const n of nodes) this.appendChild(typeof n === 'string' ? Object.assign(new Node('#text'), { text: n }) : n);
  }
  remove() { this.removed = true; }
  addEventListener(event, fn) { this.listeners[event] = fn; }
  getContext() { return { fillRect() {}, fillStyle: '#000' }; }
  toBlob(cb) { cb(null); }
  get textContent() { return this.text + this.children.map((c) => c.textContent).join(''); }
  set textContent(value) { this.text = String(value); this.children = []; }
  get innerHTML() { return this.html; }
  set innerHTML(value) { this.html += String(value); /* stub: markup is recorded, not parsed */ }
  insertAdjacentHTML(_position, value) { this.html += String(value); }
  // every node in this subtree
  *walk() { yield this; for (const c of this.children) yield* c.walk(); }
}

function makeDom(hash = '') {
  const byId = new Map();
  const document = {
    body: new Node('body'),
    getElementById(id) {
      if (!byId.has(id)) byId.set(id, Object.assign(new Node('div'), { id }));
      return byId.get(id);
    },
    createElement(tag) { return new Node(tag); },
    createTextNode(text) { return Object.assign(new Node('#text'), { text: String(text) }); },
  };
  const window = {
    location: { hash, origin: 'https://example.test', pathname: '/pastebin/' },
    listeners: {},
    addEventListener(event, fn) { this.listeners[event] = fn; },
  };
  return { document, window, byId };
}

function loadApp(hash) {
  const { document, window, byId } = makeDom(hash);
  const context = vm.createContext({
    document,
    window,
    localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    indexedDB: {
      open: () => ({ onsuccess: null, onerror: null, onupgradeneeded: null, result: {} }),
    },
    console: { error() {}, log() {} },
    Date,
    Math,
    JSON,
    btoa: (s) => Buffer.from(s, 'binary').toString('base64'),
    atob: (s) => Buffer.from(s, 'base64').toString('binary'),
    encodeURIComponent,
    decodeURIComponent,
    parseInt,
    alert() {},
    navigator: {},
    // the QR library index.html loads as a <script>
    qrcode: () => ({ addData() {}, make() {}, getModuleCount: () => 1, isDark: () => false }),
  });
  vm.runInContext(fs.readFileSync(APP, 'utf8'), context, { filename: APP });
  return { context, document, window, byId };
}

// Joins every HTML sink ever written, across the whole fake document.
function allHtmlSinks(document) {
  return Array.from(document.body.walk()).map((n) => n.html).join('\n');
}

function sharedLink(title, extra = {}) {
  const payload = { title, content: 'body', keywords: ['k'], timestamp: 1700000000000, ...extra };
  return '#paste=' + Buffer.from(encodeURIComponent(JSON.stringify(payload)), 'binary').toString('base64');
}

// --- tests ----------------------------------------------------------------

test('a crafted shared-link title is rendered as text, never as markup', () => {
  const title = '<img src=x onerror="globalThis.__pwned=1">';
  const { document, window } = loadApp(sharedLink(title));

  window.listeners.load(); // the page's load handler = the shared-link loader

  const result = document.getElementById('result');
  assert.doesNotMatch(allHtmlSinks(document), /<img|onerror/i,
    'shared-link fields reached an HTML sink');
  const tags = Array.from(document.body.walk()).map((n) => n.tagName);
  assert.ok(!tags.includes('IMG') && !tags.includes('SCRIPT'), 'markup was parsed into elements');
  assert.ok(result.textContent.includes(title), 'the title should still be shown, as text');
});

test('a shared-link CID with angle brackets stays text', () => {
  const { document, window } = loadApp(sharedLink('ok', { ipfs_cid: '<script>x</script>' }));

  window.listeners.load();

  assert.doesNotMatch(allHtmlSinks(document), /<script/i);
  assert.ok(document.getElementById('result').textContent.includes('<script>x</script>'));
});

test('a title with a quote cannot break out of an inline handler', () => {
  const title = "'); globalThis.__pwned=1; //";
  const { context } = loadApp('');

  // showQR is what the paste list's QR button calls with a stored title.
  context.showQR('https://example.test/paste/1', title);

  const html = allHtmlSinks(context.document);
  assert.doesNotMatch(html, /onclick|onerror|onload/i, 'an inline handler was built from a title');
  const dialog = context.document.body.children.at(-1);
  const heading = Array.from(dialog.walk()).find((n) => n.tagName === 'H3');
  assert.equal(heading.textContent, title, 'the title must survive as text');
});

test('a javascript: URL never becomes an href', () => {
  const { context } = loadApp('');
  assert.equal(context.safeHttpUrl('javascript:alert(1)'), '');
  assert.equal(context.safeHttpUrl('http://ok.test/x'), 'http://ok.test/x');
  assert.equal(context.safeHttpUrl('https://ok.test/x'), 'https://ok.test/x');
});

test('escapeHtml neutralises every HTML metacharacter', () => {
  const { context } = loadApp('');
  assert.equal(
    context.escapeHtml(`<&>"'`),
    '&lt;&amp;&gt;&quot;&#39;'
  );
});
