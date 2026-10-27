import { ReactNode, MouseEvent } from "react";

export const IconButton = ({
  onClick,
  disabled,
  className = "",
  color,
  children,
  href,
  title,
}: {
  onClick: () => void;
  disabled?: boolean;
  className?: string;
  color?: string;
  children: ReactNode;
  href?: string;
  title?: string;
}) => {
  const classes = `inline-flex min-h-11 min-w-11 shrink-0 items-center justify-center rounded-xl border border-transparent p-3 text-secondary hover:bg-surface-sunken hover:text-text transition-colors ${color ? `text-${color}` : ""} ${className}`;
  const handleClick = (event: MouseEvent<HTMLElement>) => {
    event.stopPropagation();
    if (disabled) {
      event.preventDefault();
      return;
    }
    if (!href) event.preventDefault();
    onClick();
  };
  if (href)
    return (
      <a
        href={disabled ? undefined : href}
        aria-disabled={disabled || undefined}
        aria-label={title}
        title={title}
        className={classes}
        onClick={handleClick}
      >
        {children}
      </a>
    );
  return (
    <button
      type="button"
      disabled={disabled}
      aria-label={title}
      title={title}
      className={classes}
      onClick={handleClick}
    >
      {children}
    </button>
  );
};
