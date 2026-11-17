/**
 * Splits a full name into its parts.
 *
 * @param {string} name The full name.
 * @returns {string[]} Each word of the name, in order.
 */
export function splitName(name) {
  return name.trim().split(/\s+/);
}
