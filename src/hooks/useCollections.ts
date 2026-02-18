import { useState, useCallback } from 'react';
import type { Collection } from '../types';

const STORAGE_KEY = 'codescope-collections';

function loadFromStorage(): Collection[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

function saveToStorage(collections: Collection[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(collections));
}

export function useCollections() {
  const [collections, setCollections] = useState<Collection[]>(loadFromStorage);

  const save = useCallback((name: string, paths: string[]) => {
    setCollections(prev => {
      const next = [...prev, {
        id: crypto.randomUUID(),
        name,
        paths,
        created: Date.now(),
      }];
      saveToStorage(next);
      return next;
    });
  }, []);

  const load = useCallback((id: string): string[] | null => {
    const c = collections.find(c => c.id === id);
    return c ? c.paths : null;
  }, [collections]);

  const remove = useCallback((id: string) => {
    setCollections(prev => {
      const next = prev.filter(c => c.id !== id);
      saveToStorage(next);
      return next;
    });
  }, []);

  const rename = useCallback((id: string, name: string) => {
    setCollections(prev => {
      const next = prev.map(c => c.id === id ? { ...c, name } : c);
      saveToStorage(next);
      return next;
    });
  }, []);

  return { collections, save, load, remove, rename };
}
