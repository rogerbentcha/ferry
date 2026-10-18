import type { ThemeRegistration } from 'shiki';

/**
 * The colors of the code blocks (Shiki), light and dark: Vercel-style syntax colors (pink
 * keywords, purple functions and tags, green strings, blue values and flags, grey comments).
 * Used by the MDX pages (lib/source.ts) and the API reference (components/api-page.tsx). The
 * background comes from app/global.css (`figure.shiki`), not from the theme.
 *
 * Plain data, no runtime import: lib/source.ts is also evaluated by fumadocs-mdx at build time.
 */
export type Palette = {
  text: string;
  comment: string;
  keyword: string;
  fn: string;
  string: string;
  value: string;
  deleted: string;
};

const light: Palette = {
  text: '#171717',
  comment: '#666666',
  keyword: '#c41562',
  fn: '#7d00cc',
  string: '#107d32',
  value: '#005ff2',
  deleted: '#d42424',
};

const dark: Palette = {
  text: '#ededed',
  comment: '#a1a1a1',
  keyword: '#ff4d8d',
  fn: '#c472fb',
  string: '#62c073',
  value: '#52a8ff',
  deleted: '#ff6166',
};

function theme(name: string, type: 'light' | 'dark', c: Palette): ThemeRegistration {
  return {
    name,
    type,
    colors: { 'editor.background': '#00000000', 'editor.foreground': c.text },
    tokenColors: [
      { settings: { foreground: c.text } },
      { scope: ['comment', 'punctuation.definition.comment', 'string.comment'], settings: { foreground: c.comment } },
      {
        scope: [
          'keyword',
          'storage',
          'storage.type',
          'storage.modifier',
          'keyword.operator.new',
          'keyword.operator.expression',
          'keyword.operator.pipe.shell',
          'keyword.operator.logical.shell',
          'keyword.operator.redirect.shell',
          'variable.language.this',
          'markup.deleted',
        ],
        settings: { foreground: c.keyword },
      },
      // Operators and punctuation stay in the text color (after `keyword`, which they would inherit).
      { scope: ['keyword.operator', 'punctuation'], settings: { foreground: c.text } },
      {
        scope: [
          'keyword.operator.pipe.shell',
          'keyword.operator.logical.shell',
          'keyword.operator.redirect.shell',
          'keyword.operator.new',
          'keyword.operator.expression',
        ],
        settings: { foreground: c.keyword },
      },
      {
        scope: [
          'entity.name.function',
          'support.function',
          'meta.function-call.generic',
          'entity.name.command',
          'entity.name.tag',
          'support.class.component',
          'entity.name.type',
          'entity.name.class',
          'support.class',
          'support.type.primitive',
          'entity.other.attribute-name.class',
          'keyword.other.special-method.dockerfile',
          'markup.heading',
        ],
        settings: { foreground: c.fn },
      },
      {
        scope: [
          'string',
          'string.quoted',
          'string.template',
          'string.regexp',
          'punctuation.definition.string',
          'markup.inserted',
          'markup.inline.raw',
        ],
        settings: { foreground: c.string },
      },
      // Inside a string, the text of an interpolation is code again. A bare shell argument
      // (`ferry up node-hello`) is text, not a string.
      {
        scope: ['meta.embedded', 'source.shell.embedded', 'string.unquoted.argument.shell'],
        settings: { foreground: c.text },
      },
      {
        scope: [
          'constant',
          'constant.numeric',
          'constant.language',
          'constant.character',
          'constant.other.option',
          'support.constant',
          'variable.other.constant',
          'variable.other.readwrite',
          'variable.other.normal.shell',
          'variable.parameter',
          'punctuation.definition.variable',
          'support.type.property-name',
          'entity.name.tag.yaml',
          'entity.other.attribute-name',
          'markup.underline.link',
        ],
        settings: { foreground: c.value },
      },
      { scope: ['entity.name.function.shell'], settings: { foreground: c.fn } },
      { scope: ['invalid', 'markup.deleted.diff'], settings: { foreground: c.deleted } },
      { scope: ['markup.bold'], settings: { fontStyle: 'bold' } },
      { scope: ['markup.italic'], settings: { fontStyle: 'italic' } },
    ],
  };
}

/** The same colors for code that Shiki does not render (the command lines of the landing page). */
export const codePalette = { light, dark };

export const codeThemes = {
  light: theme('ferry-light', 'light', light),
  dark: theme('ferry-dark', 'dark', dark),
};
