import { ReactNode } from "react";

export const ModalBody = ({ children }: { children: ReactNode }) => {
  return <div className="p-5 border-b border-divider">{children}</div>;
};
