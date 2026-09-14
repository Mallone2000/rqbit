import { StrictMode, useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { RqbitWebUI } from "./rqbit-web";
import { customSetInterval } from "./helper/customSetInterval";
import { APIContext } from "./context";
import { API, AUTHENTICATION_REQUIRED_EVENT } from "./http-api";
import { LoginForm } from "./components/LoginForm";
import { Spinner } from "./components/Spinner";
import "./globals.css";

const RootWithVersion = () => {
  let [version, setVersion] = useState<string>("");
  useEffect(() => {
    const refreshVersion = () =>
      API.getVersion().then(
        (version) => {
          setVersion((prev) => {
            if (prev == version) {
              return prev;
            }
            const title = `rqbit web - v${version}`;
            document.title = title;
            return version;
          });
          return 60000;
        },
        (e) => {
          return 1000;
        },
      );
    return customSetInterval(refreshVersion, 0);
  }, []);

  return (
    <APIContext.Provider value={API}>
      <RqbitWebUI title="rqbit" version={version} />
    </APIContext.Provider>
  );
};

const Root = () => {
  const [authenticated, setAuthenticated] = useState<boolean | null>(null);
  const [statusError, setStatusError] = useState("");

  useEffect(() => {
    API.getAuthStatus().then(
      ({ authenticated }) => setAuthenticated(authenticated),
      () => {
        setStatusError("Unable to connect to rqbit.");
        setAuthenticated(false);
      },
    );
  }, []);

  useEffect(() => {
    const requireAuthentication = () => setAuthenticated(false);
    window.addEventListener(
      AUTHENTICATION_REQUIRED_EVENT,
      requireAuthentication,
    );
    return () =>
      window.removeEventListener(
        AUTHENTICATION_REQUIRED_EVENT,
        requireAuthentication,
      );
  }, []);

  if (authenticated === null) {
    return (
      <div className="bg-surface min-h-dvh flex items-center justify-center">
        <Spinner label="Connecting" />
      </div>
    );
  }

  if (!authenticated) {
    return (
      <LoginForm
        initialError={statusError}
        onLogin={async (username, password) => {
          await API.login(username, password);
          setStatusError("");
          setAuthenticated(true);
        }}
      />
    );
  }

  return <RootWithVersion />;
};

ReactDOM.createRoot(document.getElementById("app") as HTMLInputElement).render(
  <StrictMode>
    <Root />
  </StrictMode>,
);
