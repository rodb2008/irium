
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    host: '127.0.0.1',
    port: 3401,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:3400',
        rewrite: path => path.replace(/^\/api/, ''),
      },
    },
  },
})
