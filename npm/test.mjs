// Smoke test for both builds: `npm run build && npm test`.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { render } from '@rosperitus/html-pdf'; // resolves to the Node.js build
import { initSync, render as renderWeb } from './web/html_pdf.js';

const font = readFileSync(new URL('../tests/fixtures/NotoSans-Regular.ttf', import.meta.url));
const text = (pdf) => Buffer.from(pdf).toString('latin1');
const pages = (pdf) => text(pdf).split('/Type /Page ').length - 1;

// Node.js build: Buffer fonts, defaults.
const pdf = render('<h1>Привет</h1><p>Hello, PDF.</p>', { fonts: [font] });
assert.ok(pdf instanceof Uint8Array);
assert.ok(text(pdf).startsWith('%PDF-1.7'));
assert.match(text(pdf), /\/MediaBox \[0 0 595\.28 841\.89\]/);

// Options: page size, margins, a forced break.
const two = render('<p>one</p><p style="break-before: page">two</p>', {
  fonts: [font],
  pageSize: [612, 792],
  margin: [36, 36, 36, 36],
});
assert.equal(pages(two), 2);
assert.match(text(two), /\/MediaBox \[0 0 612 792\]/);

// Images come from the resources callback; unknown ones are skipped.
const requested = [];
const png = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
  'base64',
);
const withImage = render('<img src="a.png"><img src="missing.png">', {
  fonts: [font],
  resources: (src) => (requested.push(src), src === 'a.png' ? png : undefined),
});
assert.deepEqual(requested, ['a.png', 'missing.png']);
assert.match(text(withImage), /\/Subtype \/Image/);

// Bad input throws a readable error.
assert.throws(() => render('x', { fonts: [] }), /no fonts/);
assert.throws(() => render('x', { fonts: [new Uint8Array(10)] }), /font/);
assert.throws(() => render('x', { fonts: 'nope' }), /options\.fonts/);
assert.throws(() => render('x', { fonts: [font], margin: [1, 2] }), /options\.margin/);

// Web build, initialized synchronously; fonts as an ArrayBuffer, as fetch() returns them.
initSync({ module: readFileSync(new URL('./web/html_pdf_bg.wasm', import.meta.url)) });
const buffer = font.buffer.slice(font.byteOffset, font.byteOffset + font.byteLength);
assert.ok(text(renderWeb('<p>web</p>', { fonts: [buffer] })).startsWith('%PDF-1.7'));

console.log('ok');
