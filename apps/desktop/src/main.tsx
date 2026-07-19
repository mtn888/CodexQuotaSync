import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { DesignPlayground } from "./components/DesignPlayground";
import { SettingsPage } from "./components/SettingsPage";
import { currentWindowLabel } from "./lib/bridge";
import "./styles.css";

function Root() {
  const [windowLabel, setWindowLabel] = useState<string | null>(null);

  useEffect(() => {
    void currentWindowLabel().then(setWindowLabel).catch(() => setWindowLabel("widget"));
  }, []);

  if (new URLSearchParams(window.location.search).has("designer")) return <DesignPlayground />;
  if (windowLabel === "settings") return <SettingsPage />;
  if (windowLabel === null) return null;
  return <App />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode><Root /></React.StrictMode>,
);
