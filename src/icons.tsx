import { getExtColor } from './colors';

export function FileIcon({ ext, size = 14 }: { ext: string; size?: number }) {
  const color = getExtColor(ext);

  // Header files
  if (ext === '.h' || ext === '.hpp') {
    return (
      <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
        <rect x="2" y="1" width="12" height="14" rx="1.5" stroke={color} strokeWidth="1.2" fill="none" />
        <text x="8" y="11" textAnchor="middle" fill={color} fontSize="7" fontWeight="bold" fontFamily="monospace">H</text>
      </svg>
    );
  }
  // C++ files
  if (ext === '.cpp' || ext === '.c') {
    return (
      <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
        <rect x="2" y="1" width="12" height="14" rx="1.5" stroke={color} strokeWidth="1.2" fill="none" />
        <text x="8" y="11" textAnchor="middle" fill={color} fontSize="6" fontWeight="bold" fontFamily="monospace">C+</text>
      </svg>
    );
  }
  // Shader files
  if (ext === '.usf' || ext === '.ush' || ext === '.hlsl') {
    return (
      <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
        <rect x="2" y="1" width="12" height="14" rx="1.5" stroke={color} strokeWidth="1.2" fill="none" />
        <path d="M5 5l3 3-3 3M9 11h3" stroke={color} strokeWidth="1.2" strokeLinecap="round" />
      </svg>
    );
  }
  // C#
  if (ext === '.cs') {
    return (
      <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
        <rect x="2" y="1" width="12" height="14" rx="1.5" stroke={color} strokeWidth="1.2" fill="none" />
        <text x="8" y="11" textAnchor="middle" fill={color} fontSize="6" fontWeight="bold" fontFamily="monospace">C#</text>
      </svg>
    );
  }
  // Config
  if (ext === '.ini') {
    return (
      <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
        <rect x="2" y="1" width="12" height="14" rx="1.5" stroke={color} strokeWidth="1.2" fill="none" />
        <path d="M5 5h6M5 8h4M5 11h5" stroke={color} strokeWidth="1" strokeLinecap="round" />
      </svg>
    );
  }
  // Python
  if (ext === '.py') {
    return (
      <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
        <rect x="2" y="1" width="12" height="14" rx="1.5" stroke={color} strokeWidth="1.2" fill="none" />
        <text x="8" y="11" textAnchor="middle" fill={color} fontSize="7" fontWeight="bold" fontFamily="monospace">Py</text>
      </svg>
    );
  }
  // Default
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" style={{ flexShrink: 0 }}>
      <rect x="2" y="1" width="12" height="14" rx="1.5" stroke={color} strokeWidth="1.2" fill="none" />
      <path d="M5 5h6M5 8h6M5 11h4" stroke={color} strokeWidth="1" strokeLinecap="round" />
    </svg>
  );
}
