from fleet.ambulance import ambulance_drive, ambulance_due_service, ambulance_new
from fleet.bus import bus_label, bus_new, bus_trip_cost
from fleet.cargo_bike import cargo_bike_drive, cargo_bike_due_service, cargo_bike_new
from fleet.common import Fuel, co2_per_km, cost_per_km, fuel_name, fuels, service_interval, toll
from fleet.courier import courier_new, courier_price, courier_quote
from fleet.medical import medical_line, medical_new, medical_night_price, medical_price, medical_sample
from fleet.scooter import scooter_drive, scooter_due_service, scooter_new
from fleet.sweeper import sweeper_drive, sweeper_due_list, sweeper_new
from fleet.taxi import taxi_new, taxi_trip_co2
from fleet.truck import truck_drive, truck_due_service, truck_new
from fleet.van import van_drive, van_due_service, van_new, van_plates, van_sample


def test_hidden_hydrogen():
    assert cost_per_km(Fuel.HYDROGEN) == 9
    assert co2_per_km(Fuel.HYDROGEN) == 0
    assert fuel_name(Fuel.HYDROGEN) == "hydrogen"


def test_hidden_fuels():
    assert fuels() == [Fuel.DIESEL, Fuel.PETROL, Fuel.ELECTRIC, Fuel.HYDROGEN]


def test_hidden_hydrogen_used():
    assert bus_trip_cost(bus_new(1, "H", Fuel.HYDROGEN, 0, 40), 100) == 1150
    assert bus_label(bus_new(1, "H-1", Fuel.HYDROGEN, 0, 40)) == "H-1 (hydrogen)"
    assert taxi_trip_co2(taxi_new(1, "H", Fuel.HYDROGEN, 0, 4), 50) == 0


def test_hidden_interval():
    assert service_interval(Fuel.ELECTRIC) == 30000
    assert service_interval(Fuel.DIESEL) == 15000
    assert service_interval(Fuel.PETROL) == 15000
    assert service_interval(Fuel.HYDROGEN) == 15000


def test_hidden_due_callers():
    assert truck_due_service(truck_drive(truck_new(1, "P", Fuel.ELECTRIC, 0, 5), 20000)) == False
    assert truck_due_service(truck_drive(truck_new(1, "P", Fuel.ELECTRIC, 0, 5), 30000)) == True
    assert van_due_service(van_drive(van_new(1, "P", Fuel.DIESEL, 0, 5), 15000)) == True
    assert [x.id for x in sweeper_due_list([sweeper_drive(sweeper_new(1, "P", Fuel.ELECTRIC, 0, 5), 20000), sweeper_drive(sweeper_new(2, "P", Fuel.PETROL, 0, 5), 20000)])] == [2]


def test_hidden_due_rest():
    assert cargo_bike_due_service(cargo_bike_drive(cargo_bike_new(1, "P", Fuel.ELECTRIC, 0, 5), 16000)) == False
    assert ambulance_due_service(ambulance_drive(ambulance_new(1, "P", Fuel.PETROL, 0, 5), 15000)) == True
    assert scooter_due_service(scooter_drive(scooter_new(1, "P", Fuel.HYDROGEN, 0, 5), 15000)) == True


def test_hidden_urgent():
    assert medical_new(1, "c", 0, 10, 600, True).urgent == True
    assert medical_price(medical_new(1, "c", 0, 10, 600, True)) == 1125
    assert medical_price(medical_new(1, "c", 0, 10, 600, False)) == 750


def test_hidden_urgent_line():
    assert medical_line(medical_new(3, "Iris", 0, 120, 480, True)) == "URGENT Iris #3: 120 km to zone B, arrives 9:30"
    assert medical_line(medical_new(3, "Iris", 0, 120, 480, False)) == "Iris #3: 120 km to zone B, arrives 9:30"


def test_hidden_urgent_rest():
    assert [x.urgent for x in medical_sample()] == [False, False, False]
    assert medical_night_price(medical_new(1, "c", 0, 10, 1400, True)) == 1687
    assert medical_night_price(medical_new(1, "c", 0, 10, 1400, False)) == 1125


def test_hidden_quote():
    assert courier_quote(courier_new(1, "c", 0, 100, 480), van_new(1, "V", Fuel.DIESEL, 0, 5)) == 3950
    assert courier_quote(courier_new(2, "c", 300, 150, 0), van_new(2, "W", Fuel.ELECTRIC, 0, 5)) == 4500


def test_hidden_unchanged():
    assert van_plates(van_sample()) == ["VA-101", "VA-201", "VA-301"]
    assert toll(300) == 750
    assert courier_price(courier_new(1, "c", 0, 10, 0)) == 700
