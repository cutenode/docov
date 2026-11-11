/**
 * Clamps a number to a range.
 *
 * @param value - The number to clamp.
 * @param min - The lowest it may be.
 * @returns `value`, or the bound it's past.
 */
export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}
