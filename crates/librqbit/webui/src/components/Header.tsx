import { FileInput } from "./buttons/FileInput";
import { MagnetInput } from "./buttons/MagnetInput";

// @ts-ignore
import Logo from "../../assets/logo.svg?react";

export const Header = ({
  title,
  version,
  settingsSlot,
}: {
  title: string;
  version: string;
  settingsSlot?: React.ReactNode;
}) => {
  return (
    <header className="shrink-0 bg-surface border-b border-divider flex flex-wrap gap-3 px-4 py-4 sm:px-6 justify-between items-center">
      <div className="flex min-w-0 items-center gap-2">
        <Logo className="w-10 h-10 p-1" alt="logo" />
        <h1 className="flex items-center">
          <div className="text-3xl font-extrabold tracking-tight">{title}</div>
          <div className="bg-primary/10 text-primary text-xs font-semibold me-2 px-2.5 py-0.5 rounded ms-2">
            v{version}
          </div>
        </h1>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <MagnetInput className="grow justify-center" />
        <FileInput className="grow justify-center" />
        {settingsSlot && (
          <>
            <div className="hidden lg:block w-px h-6 bg-divider mx-2" />
            {settingsSlot}
          </>
        )}
      </div>
    </header>
  );
};
