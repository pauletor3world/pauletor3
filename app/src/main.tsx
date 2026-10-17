import "@fontsource-variable/figtree";
import "@fontsource/jersey-10";
import "@fontsource-variable/jetbrains-mono";
import "./styles.css";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { temaInicial } from "./lib/theme";

// aplica o tema antes do primeiro quadro (no useEffect a tela abria no tema errado e piscava)
document.documentElement.dataset.theme = temaInicial();

async function start() {
  // Só no `npm run dev`: ?mock=livre|aberto|preparando|host|assistente simula o núcleo.
  const mock = import.meta.env.DEV ? new URLSearchParams(location.search).get("mock") : null;
  if (mock) (await import("./dev/mock")).installMock(mock);
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

start();
