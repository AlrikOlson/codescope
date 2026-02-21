import React from 'react';
import type { FindResponse } from './types';

export const EMPTY_FIND: FindResponse = { results: [], queryTime: 0, extCounts: {}, catCounts: {} };

export const HighlightedText = React.memo(function HighlightedText({ text, indices }: { text: string; indices: number[] }) {
  if (indices.length === 0) return <>{text}</>;
  const set = new Set(indices);
  const parts: JSX.Element[] = [];
  let run = '';
  let inMatch = false;

  for (let i = 0; i < text.length; i++) {
    const isMatch = set.has(i);
    if (isMatch !== inMatch) {
      if (run) {
        parts.push(inMatch ? <mark key={i}>{run}</mark> : <span key={i}>{run}</span>);
      }
      run = '';
      inMatch = isMatch;
    }
    run += text[i];
  }
  if (run) {
    parts.push(inMatch ? <mark key="end">{run}</mark> : <span key="end">{run}</span>);
  }
  return <>{parts}</>;
});
