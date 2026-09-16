import { StrictMode, useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { RqbitWebUI } from "./rqbit-web";
import { customSetInterval } from "./helper/customSetInterval";
import { APIContext } from "./context";
import { API, AUTHENTICATION_REQUIRED_EVENT } from "./http-api";
import { LoginForm } from "./components/LoginForm";
import { Spinner } from "./components/Spinner";
import "./globals.css";

const RootWithVersion = ({
  onLogout,
  publicIp,
}: {
  onLogout?: () => Promise<void>;
  publicIp?: string | null;
}) => {
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
      <RqbitWebUI
        title="rqbit"
        version={version}
        onLogout={onLogout}
        publicIp={publicIp}
      />
    </APIContext.Provider>
  );
};

const Root = () => {
  const [authenticated, setAuthenticated] = useState<boolean | null>(null);
  const [authenticationRequired, setAuthenticationRequired] = useState(false);
  const [publicIp, setPublicIp] = useState<string | null>(null);
  const [statusError, setStatusError] = useState("");

  useEffect(() => {
    API.getAuthStatus().then(
      ({ authenticated, authentication_required }) => {
        setAuthenticated(authenticated);
        setAuthenticationRequired(authentication_required);
      },
      () => {
        setStatusError("Unable to connect to rqbit.");
        setAuthenticated(false);
      },
    );
  }, []);

  useEffect(() => {
    if (!authenticated) {
      setPublicIp(null);
      return;
    }

    let cancelled = false;
    API.getPublicIp().then(
      ({ public_ip }) => {
        if (!cancelled) setPublicIp(public_ip);
      },
      () => {
        if (!cancelled) setPublicIp(null);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [authenticated]);

  useEffect(() => {
    const requireAuthentication = () => {
      if (authenticated && authenticationRequired) {
        window.location.reload();
      } else {
        setAuthenticated(false);
      }
    };
    window.addEventListener(
      AUTHENTICATION_REQUIRED_EVENT,
      requireAuthentication,
    );
    return () =>
      window.removeEventListener(
        AUTHENTICATION_REQUIRED_EVENT,
        requireAuthentication,
      );
  }, [authenticated, authenticationRequired]);

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

  return (
    <RootWithVersion
      publicIp={publicIp}
      onLogout={
        authenticationRequired
          ? async () => {
              await API.logout();
              window.location.reload();
            }
          : undefined
      }
    />
  );
};

ReactDOM.createRoot(document.getElementById("app") as HTMLInputElement).render(
  <StrictMode>
    <Root />
  </StrictMode>,
);
