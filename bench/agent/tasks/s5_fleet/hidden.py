from fleet.bus import bus_new, bus_trip_cost
from fleet.common import Fuel, eta_minutes, hhmm, toll, weight_surcharge
from fleet.courier import courier_arrival_str, courier_minutes, courier_new, courier_price
from fleet.flatbed import flatbed_haul_cost, flatbed_new
from fleet.freight import freight_minutes, freight_new
from fleet.reefer import reefer_haul_cost, reefer_new
from fleet.tanker import tanker_haul_cost, tanker_new
from fleet.tipper import tipper_haul_cost, tipper_new
from fleet.truck import truck_haul_cost, truck_new
from fleet.van import van_best_for, van_new, van_plates, van_retire, van_sample, van_trip_cost


def test_hidden_eta_up():
    assert eta_minutes(11, 40) == 17
    assert eta_minutes(30, 60) == 30
    assert eta_minutes(1, 70) == 1
    assert eta_minutes(0, 50) == 0
    assert eta_minutes(5, 0) == 0


def test_hidden_eta_used():
    assert courier_minutes(courier_new(1, "c", 0, 11, 0)) == 17
    assert courier_arrival_str(courier_new(1, "c", 0, 11, 480)) == "8:17"
    assert freight_minutes(freight_new(1, "c", 0, 7, 0)) == 9


def test_hidden_toll():
    assert toll(1) == 250
    assert toll(100) == 250
    assert toll(101) == 500
    assert toll(250) == 750
    assert toll(0) == 0


def test_hidden_toll_used():
    assert van_trip_cost(van_new(1, "V", Fuel.DIESEL, 0, 5), 150) == 2300
    assert bus_trip_cost(bus_new(1, "B", Fuel.PETROL, 0, 40), 50) == 1000


def test_hidden_surcharge():
    assert weight_surcharge(12, Fuel.DIESEL) == 800
    assert weight_surcharge(12, Fuel.ELECTRIC) == 400
    assert weight_surcharge(13, Fuel.ELECTRIC) == 600
    assert weight_surcharge(9, Fuel.ELECTRIC) == 0
    assert weight_surcharge(15, Fuel.PETROL) == 2000


def test_hidden_haul_callers():
    assert truck_haul_cost(truck_new(1, "T", Fuel.ELECTRIC, 0, 20), 100, 12) == 1150
    assert tanker_haul_cost(tanker_new(1, "T", Fuel.DIESEL, 0, 20), 100, 12) == 2250


def test_hidden_haul_rest():
    assert flatbed_haul_cost(flatbed_new(1, "F", Fuel.ELECTRIC, 0, 20), 200, 15) == 2500
    assert reefer_haul_cost(reefer_new(1, "R", Fuel.PETROL, 0, 20), 100, 11) == 2150
    assert tipper_haul_cost(tipper_new(1, "T", Fuel.ELECTRIC, 0, 20), 0, 20) == 2000


def test_hidden_best_van():
    assert van_best_for([van_retire(van_new(9, "Z", Fuel.ELECTRIC, 0, 5)), van_new(1, "A", Fuel.PETROL, 0, 5), van_new(2, "B", Fuel.DIESEL, 0, 5), van_new(3, "C", Fuel.DIESEL, 0, 9)], 100).id == 2
    assert van_best_for([van_new(4, "E", Fuel.ELECTRIC, 0, 5), van_new(5, "F", Fuel.ELECTRIC, 0, 5)], 100).id == 4
    assert van_best_for([van_retire(van_new(6, "G", Fuel.DIESEL, 0, 5)), van_new(7, "H", Fuel.PETROL, 0, 5), van_new(8, "I", Fuel.ELECTRIC, 0, 5)], 0).id == 7


def test_hidden_best_van_none():
    assert van_best_for([], 10) is None
    assert van_best_for([van_retire(van_new(1, "A", Fuel.DIESEL, 0, 5))], 10) is None


def test_hidden_unchanged():
    assert van_plates(van_sample()) == ["VA-101", "VA-201", "VA-301"]
    assert hhmm(605) == "10:05"
    assert courier_price(courier_new(1, "c", 0, 10, 0)) == 700
