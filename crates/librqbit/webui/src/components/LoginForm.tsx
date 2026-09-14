import { FormEvent, useState } from "react";
import { ErrorDetails } from "../api-types";

// @ts-ignore
import Logo from "../../assets/logo.svg?react";

export const LoginForm = ({
  onLogin,
  initialError,
}: {
  onLogin: (username: string, password: string) => Promise<void>;
  initialError?: string;
}) => {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState(initialError ?? "");

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSubmitting(true);
    setError("");
    try {
      await onLogin(username, password);
    } catch (cause) {
      const details = cause as ErrorDetails;
      setError(
        details.status === 429
          ? "Too many attempts. Please try again later."
          : details.status === 401
            ? "Invalid username or password."
            : "Unable to connect to rqbit.",
      );
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <main className="bg-surface-sunken min-h-dvh flex items-center justify-center p-4">
      <form
        className="bg-surface-raised border border-divider rounded-xl shadow-lg w-full max-w-sm p-8"
        onSubmit={submit}
      >
        <div className="flex flex-col items-center mb-8">
          <Logo className="w-16 h-16 mb-3" aria-label="rqbit logo" />
          <h1 className="text-3xl font-bold">rqbit</h1>
          <p className="text-secondary mt-2">Sign in to the Web UI</p>
        </div>

        <label className="block mb-5" htmlFor="username">
          <span className="block mb-2 font-medium">Username</span>
          <input
            autoComplete="username"
            autoFocus
            className="bg-surface border border-divider-strong rounded w-full px-3 py-2 focus:outline-none focus:border-primary"
            disabled={submitting}
            id="username"
            name="username"
            onChange={(event) => setUsername(event.target.value)}
            required
            type="text"
            value={username}
          />
        </label>

        <label className="block mb-6" htmlFor="password">
          <span className="block mb-2 font-medium">Password</span>
          <input
            autoComplete="current-password"
            className="bg-surface border border-divider-strong rounded w-full px-3 py-2 focus:outline-none focus:border-primary"
            disabled={submitting}
            id="password"
            name="password"
            onChange={(event) => setPassword(event.target.value)}
            required
            type="password"
            value={password}
          />
        </label>

        {error && (
          <p
            aria-live="polite"
            className="bg-error-bg/10 text-error rounded mb-5 px-3 py-2"
            role="alert"
          >
            {error}
          </p>
        )}

        <button
          className="bg-primary-bg text-white hover:bg-primary-bg-hover disabled:opacity-50 rounded w-full py-2 font-medium cursor-pointer disabled:cursor-not-allowed transition-colors"
          disabled={submitting}
          type="submit"
        >
          {submitting ? "Signing in…" : "Sign in"}
        </button>
      </form>
    </main>
  );
};
