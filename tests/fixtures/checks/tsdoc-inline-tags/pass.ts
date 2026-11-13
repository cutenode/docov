/**
 * Parses a date, as {@link Date.parse} does.
 *
 * @returns The time.
 */
export function parseDate(text: string): number {
  return Date.parse(text);
}
