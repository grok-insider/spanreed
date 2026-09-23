import { createRoot } from "react-dom/client";
import "@fabrials/ui/fonts.css";
import "@fabrials/ui/tokens.css";
import "@fabrials/ui/styles.css";
import "@fabrials/ai-ui/styles.css";
import "./desktop.css";
import { App } from "./app";

createRoot(document.getElementById("root")!).render(<App />);
