import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

const container = document.getElementById("root");

if (container) {
  ReactDOM.createRoot(container).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
} else {
  console.error("[sticky-harness] #root container was not found; UI not mounted.");
}
