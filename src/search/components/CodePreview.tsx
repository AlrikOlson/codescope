import { useMemo, useRef, useEffect } from 'react';
import CodeMirror, { type ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { EditorView } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { useFileContent } from '../../hooks/useFileContent';
import { useCodeMirrorLang } from '../useCodeMirrorLang';
import { catppuccinTheme } from '../cmTheme';
import { getExtColor } from '../../colors';
import { getFilename, getExt } from '../../utils';

interface Props {
  path: string | null;
}

export function CodePreview({ path }: Props) {
  const { data, loading, error } = useFileContent(path);
  const cmRef = useRef<ReactCodeMirrorRef>(null);

  const filename = path ? getFilename(path) : '';
  const ext = path ? getExt(path) : '';
  const color = getExtColor(ext);
  const langExtension = useCodeMirrorLang(ext);

  const extensions = useMemo(() => {
    const exts: import('@codemirror/state').Extension[] = [
      EditorView.editable.of(false),
      EditorState.readOnly.of(true),
      catppuccinTheme,
    ];
    if (langExtension) exts.push(langExtension);
    return exts;
  }, [langExtension]);

  // Reset scroll to top when file changes
  useEffect(() => {
    const view = cmRef.current?.view;
    if (view) {
      view.dispatch({
        effects: EditorView.scrollIntoView(0, { y: 'start' }),
      });
    }
  }, [path]);

  if (!path) {
    return (
      <div className="sw-preview">
        <div className="sw-preview-empty">
          <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1" opacity="0.2">
            <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>
          </svg>
          <span>Select a file to preview</span>
        </div>
      </div>
    );
  }

  return (
    <div className="sw-preview">
      <div className="preview-header">
        <div className="preview-header-top">
          <span className="preview-ext-badge" style={{ color, borderColor: color }}>{ext}</span>
          <span className="preview-filename">{filename}</span>
          {data && (
            <span className="sw-preview-lines">{data.lines.toLocaleString()} lines</span>
          )}
        </div>
      </div>
      <div className="preview-body">
        {loading && (
          <div className="preview-loading">
            <div className="spinner" />
            Loading...
          </div>
        )}
        {error && (
          <div className="preview-error">Failed to load: {error}</div>
        )}
        {data && !loading && (
          <CodeMirror
            ref={cmRef}
            value={data.content}
            extensions={extensions}
            theme="none"
            readOnly={true}
            editable={false}
            basicSetup={{
              lineNumbers: true,
              foldGutter: false,
              dropCursor: false,
              allowMultipleSelections: false,
              indentOnInput: false,
              bracketMatching: false,
              closeBrackets: false,
              autocompletion: false,
              rectangularSelection: false,
              crosshairCursor: false,
              highlightActiveLine: false,
              highlightSelectionMatches: false,
              closeBracketsKeymap: false,
              searchKeymap: false,
              foldKeymap: false,
              completionKeymap: false,
              lintKeymap: false,
            }}
            className="cm-preview"
          />
        )}
      </div>
    </div>
  );
}
