// Mock entry point for testing UI with large number of torrents
// Run with: npm run dev:mock

import { StrictMode } from "react";
import ReactDOM from "react-dom/client";
import { RqbitWebUI } from "./rqbit-web";
import { LoginForm } from "./components/LoginForm";
import { APIContext } from "./context";
import { MockAPI } from "./mock-api";
import "./globals.css";

const RootWithMockAPI = () => {
  const loginStatus = new URLSearchParams(window.location.search).get("login");
  if (loginStatus !== null)
    return (
      <LoginForm
        initialError={
          loginStatus === "expired"
            ? "Your session has expired. Sign in again."
            : undefined
        }
        onLogin={async () => {
          throw { status: Number(loginStatus) || 401 };
        }}
      />
    );
  return (
    <APIContext.Provider value={MockAPI}>
      <RqbitWebUI title="rqbit (MOCK)" version="mock-1.0.0" />
    </APIContext.Provider>
  );
};

ReactDOM.createRoot(document.getElementById("app") as HTMLInputElement).render(
  <StrictMode>
    <RootWithMockAPI />
  </StrictMode>,
);
