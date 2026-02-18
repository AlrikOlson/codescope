export type TokenType = 'keyword' | 'type' | 'string' | 'comment' | 'preprocessor' | 'number' | 'plain';

export interface Token {
  type: TokenType;
  text: string;
}

interface LanguageConfig {
  keywords: Set<string>;
  types: Set<string>;
  commentLine: string;
  commentBlockOpen: string;
  commentBlockClose: string;
  preprocessor: RegExp | null;
  rawStringPattern: RegExp | null;
}

const LANGUAGES: Record<string, LanguageConfig> = {
  cpp: {
    keywords: new Set([
      'auto', 'break', 'case', 'catch', 'class', 'const', 'constexpr', 'continue',
      'default', 'delete', 'do', 'else', 'enum', 'explicit', 'export', 'extern',
      'false', 'final', 'for', 'friend', 'goto', 'if', 'inline', 'mutable',
      'namespace', 'new', 'noexcept', 'nullptr', 'operator', 'override', 'private',
      'protected', 'public', 'register', 'return', 'sizeof', 'static', 'static_cast',
      'dynamic_cast', 'reinterpret_cast', 'const_cast', 'struct', 'switch', 'template',
      'this', 'throw', 'true', 'try', 'typedef', 'typeid', 'typename', 'union',
      'using', 'virtual', 'void', 'volatile', 'while', 'concept', 'requires',
      'co_await', 'co_return', 'co_yield',
    ]),
    types: new Set([
      'int', 'float', 'double', 'char', 'bool', 'long', 'short', 'unsigned', 'signed',
      'size_t', 'int8_t', 'int16_t', 'int32_t', 'int64_t', 'uint8_t', 'uint16_t',
      'uint32_t', 'uint64_t', 'wchar_t', 'char8_t', 'char16_t', 'char32_t',
      'string', 'vector', 'map', 'set', 'array', 'optional', 'pair',
      'unique_ptr', 'shared_ptr', 'weak_ptr',
    ]),
    commentLine: '//',
    commentBlockOpen: '/*',
    commentBlockClose: '*/',
    preprocessor: /^\s*#\s*\w+/,
    rawStringPattern: /R"([^(]*)\([\s\S]*?\)\1"/g,
  },
  python: {
    keywords: new Set([
      'and', 'as', 'assert', 'async', 'await', 'break', 'class', 'continue',
      'def', 'del', 'elif', 'else', 'except', 'finally', 'for', 'from',
      'global', 'if', 'import', 'in', 'is', 'lambda', 'nonlocal', 'not',
      'or', 'pass', 'raise', 'return', 'try', 'while', 'with', 'yield',
    ]),
    types: new Set([
      'int', 'float', 'str', 'bool', 'list', 'dict', 'set', 'tuple', 'bytes',
      'type', 'None', 'True', 'False', 'object', 'range', 'complex',
      'frozenset', 'bytearray', 'memoryview',
    ]),
    commentLine: '#',
    commentBlockOpen: '"""',
    commentBlockClose: '"""',
    preprocessor: null,
    rawStringPattern: null,
  },
  rust: {
    keywords: new Set([
      'as', 'async', 'await', 'break', 'const', 'continue', 'crate', 'dyn',
      'else', 'enum', 'extern', 'false', 'fn', 'for', 'if', 'impl', 'in',
      'let', 'loop', 'match', 'mod', 'move', 'mut', 'pub', 'ref', 'return',
      'self', 'Self', 'static', 'struct', 'super', 'trait', 'true', 'type',
      'unsafe', 'use', 'where', 'while', 'yield',
    ]),
    types: new Set([
      'i8', 'i16', 'i32', 'i64', 'i128', 'u8', 'u16', 'u32', 'u64', 'u128',
      'f32', 'f64', 'bool', 'char', 'str', 'String', 'Vec', 'HashMap', 'HashSet',
      'Box', 'Rc', 'Arc', 'Option', 'Result', 'Ok', 'Err', 'Some', 'None',
    ]),
    commentLine: '//',
    commentBlockOpen: '/*',
    commentBlockClose: '*/',
    preprocessor: /^\s*#\[/,
    rawStringPattern: null,
  },
  go: {
    keywords: new Set([
      'break', 'case', 'chan', 'const', 'continue', 'default', 'defer', 'else',
      'fallthrough', 'for', 'func', 'go', 'goto', 'if', 'import', 'interface',
      'map', 'package', 'range', 'return', 'select', 'struct', 'switch', 'type', 'var',
    ]),
    types: new Set([
      'bool', 'byte', 'complex64', 'complex128', 'error', 'float32', 'float64',
      'int', 'int8', 'int16', 'int32', 'int64', 'rune', 'string',
      'uint', 'uint8', 'uint16', 'uint32', 'uint64', 'uintptr',
      'nil', 'true', 'false', 'iota',
    ]),
    commentLine: '//',
    commentBlockOpen: '/*',
    commentBlockClose: '*/',
    preprocessor: null,
    rawStringPattern: null,
  },
  java: {
    keywords: new Set([
      'abstract', 'assert', 'boolean', 'break', 'byte', 'case', 'catch', 'char',
      'class', 'continue', 'default', 'do', 'double', 'else', 'enum', 'extends',
      'final', 'finally', 'float', 'for', 'if', 'implements', 'import',
      'instanceof', 'int', 'interface', 'long', 'native', 'new', 'package',
      'private', 'protected', 'public', 'return', 'short', 'static', 'strictfp',
      'super', 'switch', 'synchronized', 'this', 'throw', 'throws', 'transient',
      'try', 'void', 'volatile', 'while',
    ]),
    types: new Set([
      'String', 'Object', 'Integer', 'Long', 'Double', 'Float', 'Boolean', 'Byte',
      'Short', 'Character', 'List', 'Map', 'Set', 'Array', 'ArrayList', 'HashMap',
      'HashSet', 'Optional', 'Stream', 'Comparable', 'Iterable', 'Iterator', 'Collection',
    ]),
    commentLine: '//',
    commentBlockOpen: '/*',
    commentBlockClose: '*/',
    preprocessor: /^\s*@\w+/,
    rawStringPattern: null,
  },
  javascript: {
    keywords: new Set([
      'async', 'await', 'break', 'case', 'catch', 'class', 'const', 'continue',
      'debugger', 'default', 'delete', 'do', 'else', 'export', 'extends', 'finally',
      'for', 'function', 'if', 'import', 'in', 'instanceof', 'let', 'new', 'of',
      'return', 'static', 'super', 'switch', 'this', 'throw', 'try', 'typeof',
      'var', 'void', 'while', 'with', 'yield',
    ]),
    types: new Set([
      'Array', 'Boolean', 'Date', 'Error', 'Function', 'JSON', 'Map', 'Math',
      'Number', 'Object', 'Promise', 'Proxy', 'RegExp', 'Set', 'String', 'Symbol',
      'WeakMap', 'WeakSet', 'undefined', 'null', 'NaN', 'Infinity', 'globalThis',
    ]),
    commentLine: '//',
    commentBlockOpen: '/*',
    commentBlockClose: '*/',
    preprocessor: null,
    rawStringPattern: null,
  },
  shader: {
    keywords: new Set([
      'cbuffer', 'struct', 'float', 'float2', 'float3', 'float4',
      'half', 'half2', 'half3', 'half4', 'int', 'int2', 'int3', 'int4',
      'uint', 'uint2', 'uint3', 'uint4', 'bool', 'void', 'return',
      'if', 'else', 'for', 'while', 'do', 'switch', 'case', 'break',
      'continue', 'discard', 'in', 'out', 'inout', 'uniform', 'varying',
      'const', 'static', 'extern', 'register', 'sampler',
      'Texture2D', 'Texture3D', 'TextureCube', 'SamplerState',
      'RWTexture2D', 'StructuredBuffer', 'RWStructuredBuffer',
      'Buffer', 'ByteAddressBuffer',
    ]),
    types: new Set([
      'matrix', 'float4x4', 'float3x3', 'float2x2',
    ]),
    commentLine: '//',
    commentBlockOpen: '/*',
    commentBlockClose: '*/',
    preprocessor: /^\s*#\s*\w+/,
    rawStringPattern: null,
  },
  config: {
    keywords: new Set<string>(),
    types: new Set<string>(),
    commentLine: '#',
    commentBlockOpen: '',
    commentBlockClose: '',
    preprocessor: /^\s*\[/,
    rawStringPattern: null,
  },
  csharp: {
    keywords: new Set([
      'abstract', 'as', 'base', 'bool', 'break', 'byte', 'case', 'catch', 'char',
      'checked', 'class', 'const', 'continue', 'decimal', 'default', 'delegate',
      'do', 'double', 'else', 'enum', 'event', 'explicit', 'extern', 'false',
      'finally', 'fixed', 'float', 'for', 'foreach', 'goto', 'if', 'implicit',
      'in', 'int', 'interface', 'internal', 'is', 'lock', 'long', 'namespace',
      'new', 'null', 'object', 'operator', 'out', 'override', 'params', 'private',
      'protected', 'public', 'readonly', 'ref', 'return', 'sbyte', 'sealed',
      'short', 'sizeof', 'stackalloc', 'static', 'string', 'struct', 'switch',
      'this', 'throw', 'true', 'try', 'typeof', 'uint', 'ulong', 'unchecked',
      'unsafe', 'ushort', 'using', 'var', 'virtual', 'void', 'volatile', 'while', 'yield',
    ]),
    types: new Set([
      'String', 'Object', 'Int32', 'Int64', 'Double', 'Boolean', 'Byte',
      'List', 'Dictionary', 'HashSet', 'Array', 'Task', 'IEnumerable',
      'IList', 'IDictionary',
    ]),
    commentLine: '//',
    commentBlockOpen: '/*',
    commentBlockClose: '*/',
    preprocessor: /^\s*\[/,
    rawStringPattern: null,
  },
  plain: {
    keywords: new Set<string>(),
    types: new Set<string>(),
    commentLine: '//',
    commentBlockOpen: '/*',
    commentBlockClose: '*/',
    preprocessor: null,
    rawStringPattern: null,
  },
};

const EXT_TO_LANG: Record<string, string> = {
  '.h': 'cpp', '.hpp': 'cpp', '.hxx': 'cpp', '.cpp': 'cpp', '.c': 'cpp', '.cc': 'cpp', '.cxx': 'cpp',
  '.py': 'python',
  '.rs': 'rust',
  '.go': 'go',
  '.java': 'java', '.kt': 'java', '.scala': 'java',
  '.cs': 'csharp',
  '.js': 'javascript', '.ts': 'javascript', '.jsx': 'javascript', '.tsx': 'javascript', '.mjs': 'javascript', '.cjs': 'javascript',
  '.usf': 'shader', '.ush': 'shader', '.hlsl': 'shader', '.glsl': 'shader', '.wgsl': 'shader',
  '.ini': 'config', '.toml': 'config', '.yaml': 'config', '.yml': 'config', '.cfg': 'config',
};

function detectLanguage(ext: string): LanguageConfig {
  const langId = EXT_TO_LANG[ext] || 'plain';
  return LANGUAGES[langId] || LANGUAGES['plain'];
}

// Match word boundaries for keyword/type detection
const WORD_RE = /[A-Za-z_]\w*/g;
const NUMBER_RE = /\b(?:0[xX][0-9a-fA-F]+|0[bB][01]+|\d+\.?\d*(?:[eE][+-]?\d+)?[fFlLuU]*)\b/g;

function buildStringRegex(lang: LanguageConfig): RegExp {
  // Base: double-quoted and single-quoted strings with escape support
  let pattern = `"(?:[^"\\\\]|\\\\.)*"|'(?:[^'\\\\]|\\\\.)*'`;
  // Add backtick template literals for JS/TS
  if (lang === LANGUAGES['javascript']) {
    pattern += '|`(?:[^`\\\\]|\\\\.)*`';
  }
  return new RegExp(pattern, 'g');
}

/**
 * Tokenize a block of code into lines of tokens.
 * Handles multi-line block comments and language-specific syntax.
 */
export function tokenizeCode(code: string, ext: string = ''): Token[][] {
  const lang = detectLanguage(ext);
  const lines = code.split('\n');
  const result: Token[][] = [];
  let inBlockComment = false;

  const hasBlockComments = lang.commentBlockOpen.length > 0 && lang.commentBlockClose.length > 0;

  for (const line of lines) {
    const tokens: Token[] = [];

    if (inBlockComment && hasBlockComments) {
      const endIdx = line.indexOf(lang.commentBlockClose);
      if (endIdx >= 0) {
        const endPos = endIdx + lang.commentBlockClose.length;
        tokens.push({ type: 'comment', text: line.slice(0, endPos) });
        inBlockComment = false;
        if (endPos < line.length) {
          tokens.push(...tokenizeLine(line.slice(endPos), lang));
        }
      } else {
        tokens.push({ type: 'comment', text: line });
      }
    } else {
      const blockStart = hasBlockComments ? line.indexOf(lang.commentBlockOpen) : -1;
      const lineCommentStart = line.indexOf(lang.commentLine);

      if (blockStart >= 0 && (lineCommentStart < 0 || blockStart < lineCommentStart)) {
        const blockEnd = line.indexOf(lang.commentBlockClose, blockStart + lang.commentBlockOpen.length);
        if (blockEnd >= 0) {
          // Block comment opens and closes on same line
          const closePos = blockEnd + lang.commentBlockClose.length;
          if (blockStart > 0) {
            tokens.push(...tokenizeLine(line.slice(0, blockStart), lang));
          }
          tokens.push({ type: 'comment', text: line.slice(blockStart, closePos) });
          if (closePos < line.length) {
            tokens.push(...tokenizeLine(line.slice(closePos), lang));
          }
        } else {
          // Block comment opens but doesn't close
          if (blockStart > 0) {
            tokens.push(...tokenizeLine(line.slice(0, blockStart), lang));
          }
          tokens.push({ type: 'comment', text: line.slice(blockStart) });
          inBlockComment = true;
        }
      } else {
        tokens.push(...tokenizeLine(line, lang));
      }
    }

    result.push(tokens);
  }

  return result;
}

function tokenizeLine(line: string, lang: LanguageConfig): Token[] {
  if (!line) return [];

  // Check preprocessor directive
  if (lang.preprocessor && lang.preprocessor.test(line)) {
    return [{ type: 'preprocessor', text: line }];
  }

  // Check line comment
  const commentIdx = line.indexOf(lang.commentLine);

  const tokens: Token[] = [];
  const segments: { start: number; end: number; type: TokenType }[] = [];

  // Find strings first (they take priority)
  const processLine = commentIdx >= 0 ? line.slice(0, commentIdx) : line;
  const stringRe = buildStringRegex(lang);
  stringRe.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = stringRe.exec(processLine)) !== null) {
    segments.push({ start: m.index, end: m.index + m[0].length, type: 'string' });
  }

  // Find numbers
  NUMBER_RE.lastIndex = 0;
  while ((m = NUMBER_RE.exec(processLine)) !== null) {
    if (!isInsideSegment(m.index, segments)) {
      segments.push({ start: m.index, end: m.index + m[0].length, type: 'number' });
    }
  }

  // Find words (keywords and types)
  WORD_RE.lastIndex = 0;
  while ((m = WORD_RE.exec(processLine)) !== null) {
    if (isInsideSegment(m.index, segments)) continue;
    const word = m[0];
    if (lang.keywords.has(word)) {
      segments.push({ start: m.index, end: m.index + word.length, type: 'keyword' });
    } else if (lang.types.has(word)) {
      segments.push({ start: m.index, end: m.index + word.length, type: 'type' });
    }
  }

  // Sort segments by position
  segments.sort((a, b) => a.start - b.start);

  // Build tokens from segments
  let pos = 0;
  for (const seg of segments) {
    if (seg.start > pos) {
      tokens.push({ type: 'plain', text: processLine.slice(pos, seg.start) });
    }
    tokens.push({ type: seg.type, text: processLine.slice(seg.start, seg.end) });
    pos = seg.end;
  }
  if (pos < processLine.length) {
    tokens.push({ type: 'plain', text: processLine.slice(pos) });
  }

  // Add line comment if present
  if (commentIdx >= 0) {
    tokens.push({ type: 'comment', text: line.slice(commentIdx) });
  }

  return tokens;
}

function isInsideSegment(idx: number, segments: { start: number; end: number }[]): boolean {
  return segments.some(s => idx >= s.start && idx < s.end);
}
