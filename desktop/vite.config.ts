import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { frontendNoticePlugin } from "./scripts/frontend-notices.mjs";
export default defineConfig({ plugins: [react(), frontendNoticePlugin(fileURLToPath(new URL("./package.json", import.meta.url)))], server: { port: 1420, strictPort: true }, clearScreen: false });
