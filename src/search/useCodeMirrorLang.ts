import { useState, useEffect } from 'react';
import type { Extension } from '@codemirror/state';
import { loadLanguage } from './cmLanguages';

export function useCodeMirrorLang(ext: string): Extension | null {
  const [lang, setLang] = useState<Extension | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLang(null);

    loadLanguage(ext).then(result => {
      if (!cancelled) setLang(result);
    });

    return () => { cancelled = true; };
  }, [ext]);

  return lang;
}
