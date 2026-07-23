import { getCurrentWindow } from "@tauri-apps/api/window";
import type { MouseEvent as ReactMouseEvent } from "react";

/** Start a window drag from any nested label/text (avoids text-hit blocking data-tauri-drag-region). */
export function beginWindowDrag(e: ReactMouseEvent) {
  if (e.button !== 0) return;
  const el = e.target as HTMLElement | null;
  if (el?.closest?.("button, a, input, textarea, select, label, [data-no-drag]")) {
    return;
  }
  e.preventDefault();
  void getCurrentWindow().startDragging();
}
