import { read } from "../data.ts";
import { offsetPage, type CatalogPage } from "../pagination.ts";

export function list(request: URLSearchParams): CatalogPage {
  return offsetPage(read("discounts"), request);
}
