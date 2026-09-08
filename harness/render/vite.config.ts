import { DevupUI } from "@devup-ui/vite-plugin";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), DevupUI()],
  // The renderer loads the built site over a local preview server, so the
  // asset paths the generated code uses — `/icons/x.svg` — resolve as they
  // would in a consumer.
  base: "/",
  build: { outDir: "dist", emptyOutDir: true },
  preview: { port: 4173, strictPort: true },
});
