import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  root: '.',
  build: {
    outDir: 'dist-setup',
    emptyOutDir: true,
    rollupOptions: {
      input: 'setup.html',
    },
  },
  server: {
    port: 5174,
    strictPort: true,
  },
});
