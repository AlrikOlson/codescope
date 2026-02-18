export function getFilename(p: string): string {
  const i = p.lastIndexOf('/');
  return i >= 0 ? p.slice(i + 1) : p;
}

export function getExt(p: string): string {
  const i = p.lastIndexOf('.');
  return i >= 0 ? p.slice(i) : '';
}

export function getDir(p: string): string {
  const i = p.lastIndexOf('/');
  return i >= 0 ? p.slice(0, i) : '';
}

const EXT_TO_MARKDOWN_LANG: Record<string, string> = {
  '.h': 'cpp', '.hpp': 'cpp', '.hxx': 'cpp', '.cpp': 'cpp', '.c': 'c', '.cc': 'cpp', '.cxx': 'cpp',
  '.rs': 'rust', '.go': 'go', '.py': 'python', '.rb': 'ruby',
  '.java': 'java', '.kt': 'kotlin', '.scala': 'scala', '.cs': 'csharp',
  '.js': 'javascript', '.ts': 'typescript', '.jsx': 'jsx', '.tsx': 'tsx', '.mjs': 'javascript', '.cjs': 'javascript',
  '.usf': 'hlsl', '.ush': 'hlsl', '.hlsl': 'hlsl', '.glsl': 'glsl', '.wgsl': 'wgsl',
  '.swift': 'swift', '.lua': 'lua', '.sql': 'sql', '.sh': 'bash', '.bash': 'bash',
  '.css': 'css', '.scss': 'scss', '.html': 'html', '.xml': 'xml',
  '.json': 'json', '.toml': 'toml', '.yaml': 'yaml', '.yml': 'yaml', '.ini': 'ini', '.cfg': 'ini',
};

export function extToMarkdownLang(ext: string): string {
  return EXT_TO_MARKDOWN_LANG[ext] || '';
}
