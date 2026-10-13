import { test } from "node:test";
import assert from "node:assert/strict";
import { cartDeactivate, cartNew, cartSummary } from "../shop/cart";
import { labelOf, regions, taxRate } from "../shop/common";
import { customerNew, customerSample, customerTax, customerTotal } from "../shop/customer";
import { invoiceGross, invoiceNew } from "../shop/invoice";
import { lessonBucket, lessonNew } from "../shop/lesson";
import { productLabel, productLine, productNew } from "../shop/product";
import { routeSample, routeTotal } from "../shop/route";
import { subscriptionNew, subscriptionRenewalValue, subscriptionSample, subscriptionTotal, subscriptionValue } from "../shop/subscription";

test("hidden_ca_rate", () => {
  assert.equal(taxRate("CA"), 5);
  assert.equal(taxRate("EU"), 19);
  assert.equal(taxRate("UK"), 20);
  assert.equal(taxRate("APAC"), 0);
});

test("hidden_ca_used", () => {
  assert.equal(customerTax(customerNew(1, "a", 10, 100, "CA")), 50);
  assert.equal(invoiceGross(invoiceNew(1, "a", 1, 1000, "CA")), 1050);
});

test("hidden_regions", () => {
  assert.deepEqual(regions(), ["EU", "US", "UK", "APAC", "CA"]);
});

test("hidden_label_trim", () => {
  assert.equal(labelOf("  pen ", 4), "pen #4");
  assert.equal(labelOf("box", 12), "box #12");
});

test("hidden_label_used", () => {
  assert.equal(productLabel(productNew(4, " pen ", 1, 120, "US")), "pen #4");
  assert.equal(productLine(productNew(4, " pen", 1, 1234, "APAC")), "pen #4: 12.34");
});

test("hidden_sub_months", () => {
  assert.equal(subscriptionNew(1, "s", 2, 300, "US", 6).months, 6);
  assert.equal(subscriptionValue(subscriptionNew(1, "s", 2, 300, "US", 6)), 600);
});

test("hidden_sub_renewal", () => {
  assert.equal(subscriptionRenewalValue(subscriptionNew(1, "s", 2, 300, "US", 6)), 3600);
  assert.equal(subscriptionRenewalValue(subscriptionNew(2, "t", 1, 50, "UK", 1)), 50);
});

test("hidden_sub_sample", () => {
  assert.deepEqual(subscriptionSample().map((x) => x.months), [1, 1, 1]);
  assert.equal(subscriptionTotal(subscriptionSample()), 1799);
});

test("hidden_cart_summary", () => {
  assert.equal(cartSummary([cartNew(1, "a", 2, 500, "US"), cartNew(2, "b", 1, 1234, "EU"), cartDeactivate(cartNew(3, "c", 1, 999, "US")), cartNew(4, "d", 1, 234, "US"), cartNew(5, "e", 1, 1010, "UK")], "US"), "carts=2 total=13.20");
  assert.equal(cartSummary([cartNew(1, "a", 2, 500, "US"), cartNew(2, "b", 1, 1234, "EU"), cartDeactivate(cartNew(3, "c", 1, 999, "US")), cartNew(4, "d", 1, 234, "US"), cartNew(5, "e", 1, 1010, "UK")], "UK"), "carts=1 total=12.12");
});

test("hidden_unchanged", () => {
  assert.equal(customerTotal(customerSample()), 886);
  assert.equal(routeTotal(routeSample()), 1308);
  assert.equal(lessonBucket(lessonNew(1, "l", 1, 1, "US")), "small");
});
