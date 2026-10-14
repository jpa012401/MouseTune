import type { LoginItemError, LoginItemStatus } from "../bindings";

/** Waiting for approval still counts as on: MouseTune is registered. */
export function launchAtLoginOn(status: LoginItemStatus): boolean {
  return status === "On" || status === "RequiresApproval";
}

/** RequiresApproval only exists on macOS 13+, so it always names System Settings. */
export function launchAtLoginHelp(status: LoginItemStatus): string {
  switch (status) {
    case "RequiresApproval":
      return "Allow MouseTune in System Settings → General → Login Items.";
    case "Unavailable":
      return "Launch at login isn’t available for this copy of MouseTune.";
    default:
      return "Start MouseTune when you log in.";
  }
}

export function loginItemErrorMessage(err: LoginItemError | unknown, enabling: boolean): string {
  const reason = typeof err === "object" && err !== null && "message" in err ? String(err.message) : String(err);
  return `Couldn’t turn ${enabling ? "on" : "off"} launch at login: ${reason}`;
}
