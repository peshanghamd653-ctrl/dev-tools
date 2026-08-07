import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";

const dirname = path.dirname(fileURLToPath(import.meta.url));

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [
    tanstackRouter({
      target: "react",
      routesDirectory: "src/app/routes",
      generatedRouteTree: "src/app/routeTree.gen.ts",
      autoCodeSplitting: true,
    }),
    react(),
    tailwindcss(),
  ],

  resolve: {
    alias: {
      "@": path.resolve(dirname, "src"),
    },
  },

  build: {
    // Rollup's default 500 kB warning fired on the entry chunk from the day the
    // shell was built. It was worth acting on once — deferring the command
    // palette and the three global dialogs off the boot path took the entry
    // chunk from 650 kB to 506 kB by moving zod, react-hook-form and cmdk into
    // chunks that load an idle tick after first paint (see AppShell.tsx and
    // docs/performance.md).
    //
    // What is left was measured, module by module, and is all of it required to
    // paint the shell: react-dom (~207 kB minified, 41% of the chunk on its
    // own), TanStack Router + Query, tailwind-merge behind every `cn()`, Radix
    // tooltip/menu + floating-ui behind Topbar and Sidebar, and sonner — whose
    // `toast` is imported by `useKernelEvents`, so the module is eager whatever
    // we do with `<Toaster/>`. None of it can be deferred without deferring
    // first paint itself.
    //
    // `manualChunks` was considered and deliberately not used. It would split
    // this below 500 kB, but every piece is statically imported by the entry,
    // so Vite `modulepreload`s all of them and the webview parses the same
    // bytes before the same first paint — it would move the number without
    // moving the work. Revisit if someone measures a real win from parallel
    // compilation.
    //
    // 560 kB is therefore "today's floor plus a little headroom", not a target:
    // ~54 kB above the current 506 kB, so ordinary shell work stays quiet but
    // another eager dependency the size of zod trips it. If it fires, the fix
    // is to find what joined the boot path and give it a lazy boundary — raise
    // this number only alongside a measurement showing the new weight is
    // genuinely needed for first paint.
    chunkSizeWarningLimit: 560,
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching the Rust side
      ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"],
    },
  },
}));
