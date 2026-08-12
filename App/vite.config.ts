import { fileURLToPath } from 'node:url';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// абсолютные пути: @sc/* лежат вне корня апа, bare-алиасы у них не резолвятся
const rnWeb = fileURLToPath(import.meta.resolve('react-native-web'));
const rnSvgWeb = fileURLToPath(
  import.meta.resolve('react-native-svg/lib/module/ReactNativeSVG.web.js'),
);

// web-бандл для Linux-шелла (Servo) и дев-превью в браузере
export default defineConfig({
  plugins: [react()],
  define: {
    // react-native-web и рантайм-гейты RN-экосистемы
    global: 'globalThis',
    __DEV__: JSON.stringify(process.env.NODE_ENV !== 'production'),
    'process.env.NODE_ENV': JSON.stringify(process.env.NODE_ENV ?? 'development'),
  },
  resolve: {
    alias: {
      'react-native': rnWeb,
      'react-native-svg': rnSvgWeb,
    },
    extensions: ['.web.tsx', '.web.ts', '.web.js', '.tsx', '.ts', '.js'],
  },
  optimizeDeps: {
    esbuildOptions: {
      resolveExtensions: ['.web.tsx', '.web.ts', '.web.js', '.tsx', '.ts', '.js'],
      loader: { '.js': 'jsx' },
    },
  },
});
