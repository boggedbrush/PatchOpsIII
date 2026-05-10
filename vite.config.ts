import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  root: "src/renderer",
  base: "./",
  server: {
    port: 5173,
    strictPort: true
  },
  build: {
    outDir: "../../dist/renderer",
    emptyOutDir: true
  }
});
