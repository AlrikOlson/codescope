import type { Extension } from '@codemirror/state';

type LanguageLoader = () => Promise<Extension>;

const EXT_TO_LOADER: Record<string, LanguageLoader> = {
  // C / C++
  '.c':    () => import('@codemirror/lang-cpp').then(m => m.cpp()),
  '.cc':   () => import('@codemirror/lang-cpp').then(m => m.cpp()),
  '.cpp':  () => import('@codemirror/lang-cpp').then(m => m.cpp()),
  '.cxx':  () => import('@codemirror/lang-cpp').then(m => m.cpp()),
  '.h':    () => import('@codemirror/lang-cpp').then(m => m.cpp()),
  '.hpp':  () => import('@codemirror/lang-cpp').then(m => m.cpp()),
  '.hxx':  () => import('@codemirror/lang-cpp').then(m => m.cpp()),
  // C# (cpp approximation)
  '.cs':   () => import('@codemirror/lang-cpp').then(m => m.cpp()),
  // Shaders (cpp approximation — C-like syntax)
  '.hlsl': () => import('@codemirror/lang-cpp').then(m => m.cpp()),
  '.glsl': () => import('@codemirror/lang-cpp').then(m => m.cpp()),
  '.wgsl': () => import('@codemirror/lang-cpp').then(m => m.cpp()),
  '.usf':  () => import('@codemirror/lang-cpp').then(m => m.cpp()),
  '.ush':  () => import('@codemirror/lang-cpp').then(m => m.cpp()),
  // Python
  '.py':   () => import('@codemirror/lang-python').then(m => m.python()),
  // Rust
  '.rs':   () => import('@codemirror/lang-rust').then(m => m.rust()),
  // Go
  '.go':   () => import('@codemirror/lang-go').then(m => m.go()),
  // Java / Kotlin / Scala
  '.java': () => import('@codemirror/lang-java').then(m => m.java()),
  '.kt':   () => import('@codemirror/lang-java').then(m => m.java()),
  '.scala':() => import('@codemirror/lang-java').then(m => m.java()),
  // JavaScript / TypeScript
  '.js':   () => import('@codemirror/lang-javascript').then(m => m.javascript()),
  '.mjs':  () => import('@codemirror/lang-javascript').then(m => m.javascript()),
  '.cjs':  () => import('@codemirror/lang-javascript').then(m => m.javascript()),
  '.jsx':  () => import('@codemirror/lang-javascript').then(m => m.javascript({ jsx: true })),
  '.ts':   () => import('@codemirror/lang-javascript').then(m => m.javascript({ typescript: true })),
  '.tsx':  () => import('@codemirror/lang-javascript').then(m => m.javascript({ jsx: true, typescript: true })),
  // JSON
  '.json': () => import('@codemirror/lang-json').then(m => m.json()),
  // YAML
  '.yaml': () => import('@codemirror/lang-yaml').then(m => m.yaml()),
  '.yml':  () => import('@codemirror/lang-yaml').then(m => m.yaml()),
  // CSS
  '.css':  () => import('@codemirror/lang-css').then(m => m.css()),
  '.scss': () => import('@codemirror/lang-css').then(m => m.css()),
};

const cache = new Map<string, Extension>();

export async function loadLanguage(ext: string): Promise<Extension | null> {
  const loader = EXT_TO_LOADER[ext];
  if (!loader) return null;

  const cached = cache.get(ext);
  if (cached) return cached;

  const extension = await loader();
  cache.set(ext, extension);
  return extension;
}
