import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const appRoot = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  plugins: [svelte()],
  server: {
    port: 4173
  },
  build: {
    rollupOptions: {
      input: {
        main: resolve(appRoot, "index.html"),
        docs: resolve(appRoot, "docs/index.html")
      }
    }
  }
});
