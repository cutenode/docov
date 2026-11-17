/**
 * Formats a price for display.
 *
 * @param cents - The price in cents.
 * @param currency - An ISO 4217 currency code, like `USD`.
 * @returns The formatted price, like `$4.20`.
 */
export function formatPrice(cents: number, currency: string): string {
  return new Intl.NumberFormat("en-US", { style: "currency", currency }).format(cents / 100);
}
