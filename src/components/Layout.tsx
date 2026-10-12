import { useEffect } from "react";
import { NavLink, Outlet } from "react-router";
import { startConfigSync } from "../store/config";
import { startCaptureSync } from "../store/capture";
import { RecoveryNotice } from "./RecoveryNotice";

const NAV = [
  { to: "/buttons", label: "Buttons" },
  { to: "/scroll", label: "Scroll" },
  { to: "/general", label: "General" },
  { to: "/about", label: "About" },
] as const;

export function Layout() {
  // Synced here rather than on a page, so the recovery notice shows on whichever page opens first.
  useEffect(() => startConfigSync(), []);
  useEffect(() => startCaptureSync(), []);

  return (
    <div className="flex h-full">
      <nav
        aria-label="Settings sections"
        data-tauri-drag-region
        className="flex w-48 shrink-0 flex-col gap-1 border-r border-black/10 bg-black/[0.03] px-3 pb-3 pt-12 dark:border-white/10 dark:bg-white/[0.03]"
      >
        {NAV.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            className={({ isActive }) =>
              `rounded-md px-3 py-1.5 text-sm ${isActive ? "bg-blue-500 text-white" : "hover:bg-black/5 dark:hover:bg-white/10"}`
            }
          >
            {item.label}
          </NavLink>
        ))}
      </nav>
      <main className="flex-1 overflow-y-auto px-8 pb-8 pt-12">
        <RecoveryNotice />
        <Outlet />
      </main>
    </div>
  );
}
