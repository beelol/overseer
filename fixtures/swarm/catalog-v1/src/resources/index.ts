import { list as accounts } from "./accounts.ts";
import { list as addresses } from "./addresses.ts";
import { list as auditEvents } from "./auditEvents.ts";
import { list as badges } from "./badges.ts";
import { list as carts } from "./carts.ts";
import { list as categories } from "./categories.ts";
import { list as comments } from "./comments.ts";
import { list as coupons } from "./coupons.ts";
import { list as customers } from "./customers.ts";
import { list as deliveries } from "./deliveries.ts";
import { list as discounts } from "./discounts.ts";
import { list as documents } from "./documents.ts";
import { list as invoices } from "./invoices.ts";
import { list as items } from "./items.ts";
import { list as labels } from "./labels.ts";
import { list as locations } from "./locations.ts";
import { list as orders } from "./orders.ts";
import { list as payments } from "./payments.ts";
import { list as products } from "./products.ts";
import { list as returns } from "./returns.ts";
import { list as reviews } from "./reviews.ts";
import { list as shipments } from "./shipments.ts";
import { list as suppliers } from "./suppliers.ts";
import { list as warehouses } from "./warehouses.ts";

export const routes = new Map<string, (request: URLSearchParams) => ReturnType<typeof accounts>>([
  ["accounts", accounts],
  ["addresses", addresses],
  ["auditEvents", auditEvents],
  ["badges", badges],
  ["carts", carts],
  ["categories", categories],
  ["comments", comments],
  ["coupons", coupons],
  ["customers", customers],
  ["deliveries", deliveries],
  ["discounts", discounts],
  ["documents", documents],
  ["invoices", invoices],
  ["items", items],
  ["labels", labels],
  ["locations", locations],
  ["orders", orders],
  ["payments", payments],
  ["products", products],
  ["returns", returns],
  ["reviews", reviews],
  ["shipments", shipments],
  ["suppliers", suppliers],
  ["warehouses", warehouses]
]);
