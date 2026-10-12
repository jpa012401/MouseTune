import type { Action, Remap } from "../bindings";

const LEVELS = ["Click", "Double click", "Triple click"];

function buttonName(button: number): string {
  return button === 3 ? "Middle button" : `Button ${button}`;
}

function actionName(action: Action): string {
  switch (action.type) {
    case "SymbolicHotkey":
      return action.id === "MissionControl" ? "Mission Control" : "Show Desktop";
    case "Navigate":
      return action.direction;
  }
}

/** Plain-words label for one remap row. */
export function remapLabel({ trigger, effect }: Remap): { trigger: string; action: string } {
  const level = LEVELS[trigger.level - 1] ?? `${trigger.level}× click`;
  const press = trigger.duration === "click" ? level : trigger.level === 1 ? "Hold" : `${level} and hold`;
  return { trigger: `${buttonName(trigger.button)} · ${press}`, action: actionName(effect.action) };
}
