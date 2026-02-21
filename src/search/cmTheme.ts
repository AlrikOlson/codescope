import { EditorView } from '@codemirror/view';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { tags } from '@lezer/highlight';
import type { Extension } from '@codemirror/state';

const editorTheme = EditorView.theme({
  '&': {
    backgroundColor: 'var(--bg)',
    color: 'var(--text)',
  },
  '.cm-content': {
    caretColor: 'transparent',
    fontFamily: 'var(--font-mono)',
  },
  '.cm-cursor, .cm-dropCursor': {
    display: 'none',
  },
  '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': {
    backgroundColor: 'transparent',
  },
  '.cm-activeLine': {
    backgroundColor: 'transparent',
  },
  '.cm-gutters': {
    backgroundColor: 'var(--bg)',
    color: 'var(--neon-cyan)',
    border: 'none',
  },
  '.cm-activeLineGutter': {
    backgroundColor: 'transparent',
  },
}, { dark: true });

const highlightStyle = HighlightStyle.define([
  { tag: [tags.keyword, tags.controlKeyword, tags.definitionKeyword, tags.moduleKeyword, tags.operatorKeyword],
    color: 'var(--mauve)', fontWeight: '500' },
  { tag: [tags.typeName, tags.className, tags.namespace, tags.bool],
    color: 'var(--yellow)' },
  { tag: [tags.string, tags.special(tags.string), tags.character],
    color: 'var(--green)' },
  { tag: [tags.lineComment, tags.blockComment, tags.docComment],
    color: 'var(--text3)', fontStyle: 'italic' },
  { tag: [tags.processingInstruction, tags.meta, tags.annotation],
    color: 'var(--peach)', fontWeight: '500' },
  { tag: [tags.number, tags.integer, tags.float],
    color: 'var(--peach)' },
  { tag: [tags.function(tags.variableName), tags.function(tags.definition(tags.variableName))],
    color: 'var(--text)' },
  { tag: [tags.propertyName, tags.definition(tags.propertyName)],
    color: 'var(--text)' },
  { tag: tags.operator,
    color: 'var(--text2)' },
  { tag: tags.punctuation,
    color: 'var(--text2)' },
]);

export const catppuccinTheme: Extension = [
  editorTheme,
  syntaxHighlighting(highlightStyle),
];
