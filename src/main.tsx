import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { SessionsProvider } from "./hooks/useSessions";
import "./styles/globals.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <SessionsProvider>
      <App />
    </SessionsProvider>
  </React.StrictMode>,
);
