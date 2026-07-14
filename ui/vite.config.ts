import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// The app is served by mica at /admin (embedded via rust-embed).
// `npm run dev` proxies API + artifact + VNC traffic to a locally
// running mica on :4444.
export default defineConfig({
  base: "/admin/",
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  // noVNC 1.7 uses top-level await; es2022 is the first target with it.
  build: { target: "es2022" },
  server: {
    proxy: {
      "/admin/api": "http://localhost:4444",
      "/status": "http://localhost:4444",
      "/logs": "http://localhost:4444",
      "/video": "http://localhost:4444",
      "/vnc": { target: "ws://localhost:4444", ws: true },
    },
  },
});
