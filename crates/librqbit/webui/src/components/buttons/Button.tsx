import { ReactNode } from "react";

export const Button: React.FC<{
  onClick: () => void;
  variant?: "cancel" | "primary" | "secondary" | "danger" | "none";
  size?: "sm" | "default";
  className?: string;
  disabled?: boolean;
  children: ReactNode;
}> = ({ onClick, children, className, disabled, variant, size }) => {
  const sizeClassNames = {
    sm: "min-h-11 text-sm px-3 py-2",
    default: "min-h-12 px-5 py-3",
  }[size ?? "default"];

  const variantClassNames = {
    secondary:
      "bg-surface-raised border-divider hover:bg-primary-bg hover:text-white transition-colors",
    danger:
      "bg-error-bg text-white border-transparent hover:bg-red-600 transition-colors",
    primary:
      "bg-primary-bg text-white border-transparent hover:bg-primary-bg-hover disabled:opacity-50 transition-colors",
    cancel:
      "bg-surface-raised text-secondary border-divider hover:bg-surface-sunken transition-colors",
    none: "",
  }[variant ?? "secondary"];

  return (
    <button
      type="button"
      disabled={disabled}
      onClick={(e) => {
        e.preventDefault();
        onClick();
      }}
      className={`inline-flex items-center gap-2 border rounded-xl font-semibold cursor-pointer disabled:cursor-not-allowed ${sizeClassNames} ${variantClassNames} ${className ?? ""}`}
    >
      {children}
    </button>
  );
};
