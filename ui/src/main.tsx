import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import App from "./App";
import "./app.css";

// basename mirrors the mount point (mica serves the SPA at /admin, and
// its asset handler falls back to index.html for extension-less paths,
// so deep links like /admin/quotas resolve to the app).
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BrowserRouter basename="/admin">
      <App />
    </BrowserRouter>
  </StrictMode>,
);
