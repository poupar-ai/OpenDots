import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'node:path';

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': 'http://127.0.0.1:37778',
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    assetsDir: 'assets',
    rollupOptions: {
      output: {
        // Separa vendors pesados em chunks próprios (cacheáveis e carregados
        // em paralelo). Ordem importa: checar libs específicas ANTES de
        // "react" (que casaria com react-markdown/react-dom).
        manualChunks(id) {
          if (!id.includes('node_modules')) return undefined;
          if (id.includes('pixi')) return 'pixi';
          if (
            id.includes('react-markdown') ||
            id.includes('remark') ||
            id.includes('micromark') ||
            id.includes('mdast') ||
            id.includes('hast') ||
            id.includes('unist') ||
            id.includes('property-information') ||
            id.includes('vfile')
          )
            return 'markdown';
          if (id.includes('@mantine')) return 'mantine';
          if (id.includes('/react') || id.includes('react-dom') || id.includes('scheduler'))
            return 'react-vendor';
          return undefined;
        },
      },
    },
  },
});
