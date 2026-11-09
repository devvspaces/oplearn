import { cpSync } from "node:fs";
import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";

const host = process.env.TAURI_DEV_HOST;

/** Copies pdf.js fonts and CMaps into public/ so the viewer can load them offline. */
const pdfjsAssets = () => ({
  name: "pdfjs-assets",
  buildStart() {
    for (const d of ["standard_fonts", "cmaps"])
      cpSync(`node_modules/pdfjs-dist/${d}`, `public/pdfjs/${d}`, { recursive: true });
  },
});

export default defineConfig({
  plugins: [
    tanstackRouter({
      target: "react",
      autoCodeSplitting: true,
      routesDirectory: "src/app/routes",
      generatedRouteTree: "src/app/routeTree.gen.ts",
    }),
    react(),
    tailwindcss(),
    pdfjsAssets(),
  ],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1431 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: { target: "es2022" },
  test: {
    environment: "jsdom",
    include: ["tests/unit/**/*.test.ts", "tests/unit/**/*.test.tsx"],
  },
});
