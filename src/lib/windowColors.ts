/** Distinct fill colors for usage windows (overlay + details). */
export const WINDOW_BAR_COLORS = ["#2dd4bf", "#60a5fa", "#fbbf24"] as const;

export function windowBarColor(index: number, fallback = "#9ca3af"): string {
  return WINDOW_BAR_COLORS[index] ?? fallback;
}
