/**
 * A folder and what it's called.
 *
 * @typedef {object} Folder
 * @property {string} path - Where it is.
 * @property {string} [name] - What it's called.
 */

/**
 * Joins a folder and the paths inside it.
 *
 * @template {string} T
 * @param {T} base - The folder.
 * @param {string[]} [rest] - The paths inside it.
 * @param {{ sep?: string }} [options={}] - How to join them.
 * @param {string} [options.sep='/'] - What goes between two parts.
 * @returns {Map<string, Folder>} Each part, by its path.
 * @throws {TypeError} When `base` is empty.
 */
export function join(base, rest = [], options = {}) {
  if (!base) {
    throw new TypeError('base is empty');
  }
  const sep = options.sep ?? '/';
  return new Map([base, ...rest].map((path) => [path, { path: `${base}${sep}${path}` }]));
}

/**
 * The folder every path starts from.
 *
 * @type {Folder}
 */
export const root = { path: '/' };
