import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { TrayMenuApp } from "./components/TrayMenuApp";
import "./index.css";

const params = new URLSearchParams(window.location.search);
const isTrayView = window.location.hash === "#tray" || params.get("view") === "tray";

if (isTrayView) {
  document.documentElement.classList.add("tray-surface");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{isTrayView ? <TrayMenuApp /> : <App />}</React.StrictMode>,
);
