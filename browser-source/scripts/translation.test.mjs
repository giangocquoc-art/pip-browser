import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import test from 'node:test';

const root = new URL('../', import.meta.url);
const source = await readFile(new URL('crates/app/src/translate.rs', root), 'utf8');
const script = source.match(/PAGE_TRANSLATION_SCRIPT: &str = r#"([\s\S]*?)"#;/)?.[1];
assert.ok(script, 'translation script exists in the Rust host module');

function createPage(textValues) {
  const textNodes = textValues.map(value => {
    const node = {
      isConnected: true,
      parentElement: {
        isContentEditable: false,
        tagName: 'DIV',
        parentElement: null,
        closest: () => null,
      },
    };
    Object.defineProperty(node, 'nodeValue', {
      get: () => node.textContent,
      set: value => { node.textContent = value; },
      configurable: true,
    });
    node.textContent = value;
    return node;
  });
  let index = 0;
  const document = {
    body: {},
    documentElement: { appendChild(node) { this.overlay = node; } },
    createElement: () => ({ style: {}, setAttribute() {}, remove() {} }),
    getElementById(id) { return id === '__pip_translate_status' ? this.documentElement.overlay : null; },
    createTreeWalker() {
      return {
        currentNode: null,
        nextNode() {
          if (index >= textNodes.length) return false;
          this.currentNode = textNodes[index++];
          return true;
        },
      };
    },
  };
  return { document, textNodes };
}

async function runTranslation({ translate, detect } = {}) {
  const originalTexts = ['one', 'two', 'three', 'four', 'five'];
  const { document, textNodes } = createPage(originalTexts);
  const destroyed = { detector: false, translator: false };
  const context = {
    document,
    window: {},
    NodeFilter: { SHOW_TEXT: 4, FILTER_REJECT: 2, FILTER_ACCEPT: 1 },
    setTimeout() {},
    LanguageDetector: {
      async create() {
        return {
          async detect(sample) {
            if (detect) return detect(sample);
            return [{ detectedLanguage: 'en' }];
          },
          destroy() { destroyed.detector = true; },
        };
      },
    },
    Translator: {
      async availability() { return 'available'; },
      async create() {
        return {
          async translate(text) {
            if (translate) return translate(text);
            return `VI:${text}`;
          },
          destroy() { destroyed.translator = true; },
        };
      },
    },
  };
  vm.runInNewContext(script, context);
  await new Promise(resolve => setTimeout(resolve, 80));
  return { originalTexts, textNodes, window: context.window, destroyed };
}

test('a translation failure waits for outstanding workers before rollback and cleanup', async () => {
  const result = await runTranslation({
    translate: async text => {
      await new Promise(resolve => setTimeout(resolve, text === 'one' ? 2 : 20));
      if (text === 'one') throw new Error('simulated translation failure');
      return `VI:${text}`;
    },
  });

  assert.deepEqual(result.textNodes.map(node => node.nodeValue), result.originalTexts);
  assert.equal(result.window.__pipTranslation, undefined);
  assert.deepEqual(result.destroyed, { detector: true, translator: true });
});

test('language detector is released when detection rejects', async () => {
  const result = await runTranslation({ detect: async () => { throw new Error('simulated detection failure'); } });

  assert.deepEqual(result.textNodes.map(node => node.nodeValue), result.originalTexts);
  assert.equal(result.window.__pipTranslation, undefined);
  assert.deepEqual(result.destroyed, { detector: true, translator: false });
});
