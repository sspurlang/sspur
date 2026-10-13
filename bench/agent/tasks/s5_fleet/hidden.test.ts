import { test } from "node:test";
import assert from "node:assert/strict";
import { busNew, busTripCost } from "../fleet/bus";
import { etaMinutes, hhmm, toll, weightSurcharge } from "../fleet/common";
import { courierArrivalStr, courierMinutes, courierNew, courierPrice } from "../fleet/courier";
import { flatbedHaulCost, flatbedNew } from "../fleet/flatbed";
import { freightMinutes, freightNew } from "../fleet/freight";
import { reeferHaulCost, reeferNew } from "../fleet/reefer";
import { tankerHaulCost, tankerNew } from "../fleet/tanker";
import { tipperHaulCost, tipperNew } from "../fleet/tipper";
import { truckHaulCost, truckNew } from "../fleet/truck";
import { vanBestFor, vanNew, vanPlates, vanRetire, vanSample, vanTripCost } from "../fleet/van";

test("hidden_eta_up", () => {
  assert.equal(etaMinutes(11, 40), 17);
  assert.equal(etaMinutes(30, 60), 30);
  assert.equal(etaMinutes(1, 70), 1);
  assert.equal(etaMinutes(0, 50), 0);
  assert.equal(etaMinutes(5, 0), 0);
});

test("hidden_eta_used", () => {
  assert.equal(courierMinutes(courierNew(1, "c", 0, 11, 0)), 17);
  assert.equal(courierArrivalStr(courierNew(1, "c", 0, 11, 480)), "8:17");
  assert.equal(freightMinutes(freightNew(1, "c", 0, 7, 0)), 9);
});

test("hidden_toll", () => {
  assert.equal(toll(1), 250);
  assert.equal(toll(100), 250);
  assert.equal(toll(101), 500);
  assert.equal(toll(250), 750);
  assert.equal(toll(0), 0);
});

test("hidden_toll_used", () => {
  assert.equal(vanTripCost(vanNew(1, "V", "diesel", 0, 5), 150), 2300);
  assert.equal(busTripCost(busNew(1, "B", "petrol", 0, 40), 50), 1000);
});

test("hidden_surcharge", () => {
  assert.equal(weightSurcharge(12, "diesel"), 800);
  assert.equal(weightSurcharge(12, "electric"), 400);
  assert.equal(weightSurcharge(13, "electric"), 600);
  assert.equal(weightSurcharge(9, "electric"), 0);
  assert.equal(weightSurcharge(15, "petrol"), 2000);
});

test("hidden_haul_callers", () => {
  assert.equal(truckHaulCost(truckNew(1, "T", "electric", 0, 20), 100, 12), 1150);
  assert.equal(tankerHaulCost(tankerNew(1, "T", "diesel", 0, 20), 100, 12), 2250);
});

test("hidden_haul_rest", () => {
  assert.equal(flatbedHaulCost(flatbedNew(1, "F", "electric", 0, 20), 200, 15), 2500);
  assert.equal(reeferHaulCost(reeferNew(1, "R", "petrol", 0, 20), 100, 11), 2150);
  assert.equal(tipperHaulCost(tipperNew(1, "T", "electric", 0, 20), 0, 20), 2000);
});

test("hidden_best_van", () => {
  assert.equal(vanBestFor([vanRetire(vanNew(9, "Z", "electric", 0, 5)), vanNew(1, "A", "petrol", 0, 5), vanNew(2, "B", "diesel", 0, 5), vanNew(3, "C", "diesel", 0, 9)], 100)?.id, 2);
  assert.equal(vanBestFor([vanNew(4, "E", "electric", 0, 5), vanNew(5, "F", "electric", 0, 5)], 100)?.id, 4);
  assert.equal(vanBestFor([vanRetire(vanNew(6, "G", "diesel", 0, 5)), vanNew(7, "H", "petrol", 0, 5), vanNew(8, "I", "electric", 0, 5)], 0)?.id, 7);
});

test("hidden_best_van_none", () => {
  assert.equal(vanBestFor([], 10), undefined);
  assert.equal(vanBestFor([vanRetire(vanNew(1, "A", "diesel", 0, 5))], 10), undefined);
});

test("hidden_unchanged", () => {
  assert.deepEqual(vanPlates(vanSample()), ["VA-101", "VA-201", "VA-301"]);
  assert.equal(hhmm(605), "10:05");
  assert.equal(courierPrice(courierNew(1, "c", 0, 10, 0)), 700);
});
