import { ReactNode } from "react";

export const ModalFooter = ({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) => {
  return (
    <div className={`p-5 flex flex-wrap justify-end gap-3 ${className}`}>
      {children}
    </div>
  );
};
