// The reset the generated code is written against. Figma's code assumes one
// is in place, and which one decides where every box sits: this sets the root
// line-height to 1.5 and leaves images inline on the baseline, neither of
// which a browser does on its own.
import "@devup-ui/reset-css";
import { StrictMode, type ComponentType } from "react";
import { createRoot } from "react-dom/client";

// Every generated screen under `src/screens`, by file name. `acquire.py`
// writes them; they are not committed.
const screens = import.meta.glob<Record<string, unknown>>("./screens/*.tsx");

async function main() {
  const name = new URLSearchParams(location.search).get("screen");
  const loader = name ? screens[`./screens/${name}.tsx`] : undefined;
  const root = document.getElementById("root");
  if (!root) throw new Error("no root");
  if (!loader) {
    root.textContent = `no such screen: ${name}. known: ${Object.keys(screens).join(", ")}`;
    document.body.dataset.ready = "error";
    return;
  }
  const module = await loader();
  // A responsive module is a default export; a single frame is the one
  // named export the server gives it.
  const component = (module.default ??
    Object.values(module).find((value) => typeof value === "function")) as
    | ComponentType
    | undefined;
  if (!component) throw new Error(`no component in ${name}`);
  const Screen = component;
  createRoot(root).render(
    <StrictMode>
      <Screen />
    </StrictMode>,
  );
  await document.fonts.ready;
  await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  await Promise.all(
    Array.from(document.images).map((image) =>
      image.complete ? Promise.resolve() : new Promise((resolve) => {
        image.addEventListener("load", resolve, { once: true });
        image.addEventListener("error", resolve, { once: true });
      }),
    ),
  );
  document.body.dataset.ready = "1";
}

main().catch((error) => {
  console.error(error);
  document.body.dataset.ready = "error";
  document.body.dataset.error = String(error);
});
