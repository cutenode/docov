/**
 * Formats cents as dollars.
 *
 * @param {number} cents
 * @returns {string}
 */
export function dollars(cents) {
  return `$${(cents / 100).toFixed(2)}`;
}

export const CURRENCY = 'USD';

function round(n) {
  return Math.round(n);
}
