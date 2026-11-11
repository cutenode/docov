/** A line in a shopping cart. */
export interface Item {
  /** The product's SKU. */
  sku: string;
  quantity: number;
}

/**
 * Adds up a cart.
 *
 * @param items - What's in the cart.
 * @param prices - Each SKU's price, in cents.
 */
export function total(items: Item[], prices: Map<string, number>): number {
  return items.reduce((sum, item) => sum + (prices.get(item.sku) ?? 0) * item.quantity, 0);
}

export class Cart {
  #items: Item[] = [];

  /** Adds an item to the cart. */
  add(item: Item): void {
    this.#items.push(item);
  }

  get size(): number {
    return this.#items.length;
  }
}

// docov-ignore-next-line: kept for old callers
export const legacyTotal = total;
