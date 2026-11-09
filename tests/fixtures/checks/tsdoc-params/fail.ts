/**
 * Joins two paths.
 *
 * @param {string} base - The folder.
 * @param rest The path inside it.
 */
export function join(base: string, rest: string): string {
  return `${base}/${rest}`;
}
