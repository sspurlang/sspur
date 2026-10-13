import { test } from "node:test";
import assert from "node:assert/strict";
import { ambulanceDrive, ambulanceDueService, ambulanceNew } from "../fleet/ambulance";
import { busLabel, busNew, busTripCost } from "../fleet/bus";
import { cargoBikeDrive, cargoBikeDueService, cargoBikeNew } from "../fleet/cargo_bike";
import { co2PerKm, costPerKm, fuelName, fuels, serviceInterval, toll } from "../fleet/common";
import { courierNew, courierPrice, courierQuote } from "../fleet/courier";
import { medicalLine, medicalNew, medicalNightPrice, medicalPrice, medicalSample } from "../fleet/medical";
import { scooterDrive, scooterDueService, scooterNew } from "../fleet/scooter";
import { sweeperDrive, sweeperDueList, sweeperNew } from "../fleet/sweeper";
import { taxiNew, taxiTripCo2 } from "../fleet/taxi";
import { truckDrive, truckDueService, truckNew } from "../fleet/truck";
import { vanDrive, vanDueService, vanNew, vanPlates, vanSample } from "../fleet/van";

test("hidden_hydrogen", () => {
  assert.equal(costPerKm("hydrogen"), 9);
  assert.equal(co2PerKm("hydrogen"), 0);
  assert.equal(fuelName("hydrogen"), "hydrogen");
});

test("hidden_fuels", () => {
  assert.deepEqual(fuels(), ["diesel", "petrol", "electric", "hydrogen"]);
});

test("hidden_hydrogen_used", () => {
  assert.equal(busTripCost(busNew(1, "H", "hydrogen", 0, 40), 100), 1150);
  assert.equal(busLabel(busNew(1, "H-1", "hydrogen", 0, 40)), "H-1 (hydrogen)");
  assert.equal(taxiTripCo2(taxiNew(1, "H", "hydrogen", 0, 4), 50), 0);
});

test("hidden_interval", () => {
  assert.equal(serviceInterval("electric"), 30000);
  assert.equal(serviceInterval("diesel"), 15000);
  assert.equal(serviceInterval("petrol"), 15000);
  assert.equal(serviceInterval("hydrogen"), 15000);
});

test("hidden_due_callers", () => {
  assert.equal(truckDueService(truckDrive(truckNew(1, "P", "electric", 0, 5), 20000)), false);
  assert.equal(truckDueService(truckDrive(truckNew(1, "P", "electric", 0, 5), 30000)), true);
  assert.equal(vanDueService(vanDrive(vanNew(1, "P", "diesel", 0, 5), 15000)), true);
  assert.deepEqual(sweeperDueList([sweeperDrive(sweeperNew(1, "P", "electric", 0, 5), 20000), sweeperDrive(sweeperNew(2, "P", "petrol", 0, 5), 20000)]).map((x) => x.id), [2]);
});

test("hidden_due_rest", () => {
  assert.equal(cargoBikeDueService(cargoBikeDrive(cargoBikeNew(1, "P", "electric", 0, 5), 16000)), false);
  assert.equal(ambulanceDueService(ambulanceDrive(ambulanceNew(1, "P", "petrol", 0, 5), 15000)), true);
  assert.equal(scooterDueService(scooterDrive(scooterNew(1, "P", "hydrogen", 0, 5), 15000)), true);
});

test("hidden_urgent", () => {
  assert.equal(medicalNew(1, "c", 0, 10, 600, true).urgent, true);
  assert.equal(medicalPrice(medicalNew(1, "c", 0, 10, 600, true)), 1125);
  assert.equal(medicalPrice(medicalNew(1, "c", 0, 10, 600, false)), 750);
});

test("hidden_urgent_line", () => {
  assert.equal(medicalLine(medicalNew(3, "Iris", 0, 120, 480, true)), "URGENT Iris #3: 120 km to zone B, arrives 9:30");
  assert.equal(medicalLine(medicalNew(3, "Iris", 0, 120, 480, false)), "Iris #3: 120 km to zone B, arrives 9:30");
});

test("hidden_urgent_rest", () => {
  assert.deepEqual(medicalSample().map((x) => x.urgent), [false, false, false]);
  assert.equal(medicalNightPrice(medicalNew(1, "c", 0, 10, 1400, true)), 1687);
  assert.equal(medicalNightPrice(medicalNew(1, "c", 0, 10, 1400, false)), 1125);
});

test("hidden_quote", () => {
  assert.equal(courierQuote(courierNew(1, "c", 0, 100, 480), vanNew(1, "V", "diesel", 0, 5)), 3950);
  assert.equal(courierQuote(courierNew(2, "c", 300, 150, 0), vanNew(2, "W", "electric", 0, 5)), 4500);
});

test("hidden_unchanged", () => {
  assert.deepEqual(vanPlates(vanSample()), ["VA-101", "VA-201", "VA-301"]);
  assert.equal(toll(300), 750);
  assert.equal(courierPrice(courierNew(1, "c", 0, 10, 0)), 700);
});
