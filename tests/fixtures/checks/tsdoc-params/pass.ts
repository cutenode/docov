/**
 * Joins two paths.
 *
 * @param base - The folder.
 * @param rest - The path inside it.
 */
export function join(base: string, rest: string): string {
  return `${base}/${rest}`;
}
