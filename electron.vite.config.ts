import { resolve } from 'path'
import { defineConfig, externalizeDepsPlugin } from 'electron-vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  main: {
    root: resolve('electron/main'),
    build: {
      lib: {
        entry: resolve('electron/main/index.ts')
      }
    },
    plugins: [externalizeDepsPlugin()]
  },
  preload: {
    root: resolve('electron/preload'),
    build: {
      lib: {
        entry: resolve('electron/preload/index.ts')
      }
    },
    plugins: [externalizeDepsPlugin()]
  },
  renderer: {
    root: resolve('.'),
    build: {
      rollupOptions: {
        input: resolve('index.html')
      }
    },
    resolve: {
      alias: {
        '@': resolve('src'),
        '@platform-impl': resolve('src/platform/electron.ts')
      }
    },
    plugins: [vue()]
  }
})
