import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { MantineProvider, createTheme } from '@mantine/core';
import App from './App';
import '@mantine/core/styles.css';
import './index.css';

// Tema Mantine — base do redesign. Mantém paleta próxima ao slate/indigo
// que o resto da UI usa (Tailwind) pra coexistência sem choque visual
// enquanto a migração panel-a-panel acontece. Densidade média + tipografia
// system pra deixar o look mais "design system" e menos vibe.
const theme = createTheme({
  primaryColor: 'indigo',
  defaultRadius: 'md',
  fontFamily:
    'ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif',
  fontFamilyMonospace:
    'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
  headings: { fontWeight: '600' },
  cursorType: 'pointer',
  components: {
    Modal: {
      defaultProps: {
        centered: true,
        radius: 'lg',
        overlayProps: { backgroundOpacity: 0.55, blur: 3 },
      },
    },
    Button: { defaultProps: { size: 'sm' } },
    TextInput: { defaultProps: { size: 'sm' } },
  },
});

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <MantineProvider theme={theme} defaultColorScheme="light">
      <App />
    </MantineProvider>
  </StrictMode>,
);
