import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  root: "frontend",
  base: "./",
  server: {
    port: 5173,
    strictPort: true
  },
  build: {
    outDir: "../dist/renderer",
    emptyOutDir: true
  }
});
