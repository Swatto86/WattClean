import { defineConfig } from "vite";

// Vanilla TypeScript, no framework: a few pages over the Rust engine.
export default defineConfig({
  root: "ui",
  build: { outDir: "dist", emptyOutDir: true, target: "esnext" },
  server: { port: 5190, strictPort: true },
  clearScreen: false,
});
