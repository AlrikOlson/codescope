import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { codeScopeServer } from './src/scanner';

export default defineConfig({
  plugins: [react(), codeScopeServer()],
  optimizeDeps: {
    include: [
      'react',
      'react-dom',
      'react/jsx-runtime',
      'three',
      'three/src/math/Vector3',
      'three/src/math/Color',
      'three/src/scenes/Scene',
      'three/src/cameras/PerspectiveCamera',
      'three/src/renderers/WebGLRenderer',
      '@tanstack/react-virtual',
    ],
  },
  server: {
    port: 8432,
    strictPort: false,
    warmup: {
      clientFiles: [
        './src/main.tsx',
        './src/App.tsx',
        './src/App.css',
        './src/FileList.tsx',
        './src/TreeSidebar.tsx',
        './src/SearchPalette.tsx',
        './src/SelectionBar.tsx',
        './src/depgraph/DependencyGraph.tsx',
        './src/treemap/CodebaseMap.tsx',
      ],
    },
    // proxy is injected dynamically by the codeScopeServer plugin
  },
});
