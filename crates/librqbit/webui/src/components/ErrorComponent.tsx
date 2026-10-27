import { BsX } from "react-icons/bs";
import { ErrorWithLabel } from "../rqbit-web";

const AlertDanger: React.FC<{
  title: string;
  children: React.ReactNode;
  onClose?: () => void;
}> = ({ title, children, onClose }) => {
  return (
    <div
      role="alert"
      className="shrink-0 bg-error/10 p-4 rounded-2xl border border-error/30"
    >
      <div className="flex justify-between mb-2">
        <h2 className="text-lg font-semibold text-error">{title}</h2>
        {onClose && (
          <button
            aria-label="Dismiss error"
            onClick={onClose}
            className="min-w-11 flex items-center justify-center text-error hover:text-error/80 transition-colors cursor-pointer"
          >
            <BsX />
          </button>
        )}
      </div>
      {children}
    </div>
  );
};

export const ErrorComponent = (props: {
  error: ErrorWithLabel | null;
  remove?: () => void;
}) => {
  let { error, remove } = props;

  if (error == null) {
    return null;
  }

  return (
    <AlertDanger onClose={remove} title={error.text}>
      {error.details?.statusText && (
        <div className="pb-2 text-md">{error.details?.statusText}</div>
      )}
      <div className="whitespace-pre-wrap text-secondary">
        {error.details?.text}
      </div>
    </AlertDanger>
  );
};
