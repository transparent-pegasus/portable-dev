import { defineConfig } from "astro/config";

// Static build; the Rust server in ../src/main.rs serves dist/ and /api/servers.
export default defineConfig({ outDir: "./dist" });
