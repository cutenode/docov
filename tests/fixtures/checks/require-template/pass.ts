/**
 * Gets the first item of a list.
 *
 * @template T The type of the list's items.
 * @param items - The list.
 * @returns Its first item, or `undefined` when it's empty.
 */
export function first<T>(items: T[]): T | undefined {
  return items[0];
}
