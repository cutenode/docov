/**
 * Gets the first item of a list.
 *
 * @param items - The list.
 * @returns Its first item, or `undefined` when it's empty.
 */
export function first<T>(items: T[]): T | undefined {
  return items[0];
}
