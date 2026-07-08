import React from "react";
import ReactDOM from "react-dom/client";
import "./theme/theme.css";
import "./styles/base.css";
import "./styles/ui.css";
import "./styles/features.css";
import App from "./app/App";
import { ThemeProvider } from "./theme/ThemeContext";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider>
      <App />
    </ThemeProvider>
  </React.StrictMode>
);
