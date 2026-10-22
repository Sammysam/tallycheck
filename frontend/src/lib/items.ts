/** D38: products & services shared bits. */

export type ItemType = 'INVENTORY' | 'SERVICE' | 'NON_INVENTORY';

export const TYPE_LABEL: Record<ItemType, string> = { INVENTORY: 'Stock item', SERVICE: 'Service', NON_INVENTORY: 'Non-stock item' };

/** A quantity as people write it: 3, 2.5. */
export const qty = (n: number) => (Number.isInteger(n) ? String(n) : n.toFixed(2));

/** What the invoice and bill forms need to know about a product. */
export interface PickItem {
  id: string;
  name: string;
  type: ItemType;
  description?: string | null;
  salePrice?: number | null;
  purchaseCost?: number | null;
  incomeAccountId?: string | null;
  expenseAccountId?: string | null;
  inventoryAccountId?: string | null;
  taxCodeId?: string | null;
  quantityOnHand: number;
  isActive: boolean;
}
