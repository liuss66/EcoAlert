import { defineConfig } from 'vite';

// Tauri 推荐配置：
// - 固定端口 1420（Tauri 开发模式默认）
// - 关闭 HMR overlay，避免遮挡 Tauri 窗口
// - HMR 通过 1421 端口走 WebSocket
export default defineConfig({
  root: 'webui',
  publicDir: 'public',
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    target: 'es2021',
    minify: false,
    sourcemap: true,
  },
  server: {
    port: 1420,
    strictPort: true,
    hmr: { port: 1421 },
    watch: { ignored: ['**/src-tauri/**'] },
  },
  clearScreen: false,
});
