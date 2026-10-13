package fleet

import (
	"reflect"
	"testing"
)

func TestHidden_eta_up(t *testing.T) {
	if got := etaMinutes(11, 40); got != 17 {
		t.Fatalf("got %v", got)
	}
	if got := etaMinutes(30, 60); got != 30 {
		t.Fatalf("got %v", got)
	}
	if got := etaMinutes(1, 70); got != 1 {
		t.Fatalf("got %v", got)
	}
	if got := etaMinutes(0, 50); got != 0 {
		t.Fatalf("got %v", got)
	}
	if got := etaMinutes(5, 0); got != 0 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_eta_used(t *testing.T) {
	if got := courierMinutes(courierNew(1, "c", 0, 11, 0)); got != 17 {
		t.Fatalf("got %v", got)
	}
	if got := courierArrivalStr(courierNew(1, "c", 0, 11, 480)); got != "8:17" {
		t.Fatalf("got %v", got)
	}
	if got := freightMinutes(freightNew(1, "c", 0, 7, 0)); got != 9 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_toll(t *testing.T) {
	if got := toll(1); got != 250 {
		t.Fatalf("got %v", got)
	}
	if got := toll(100); got != 250 {
		t.Fatalf("got %v", got)
	}
	if got := toll(101); got != 500 {
		t.Fatalf("got %v", got)
	}
	if got := toll(250); got != 750 {
		t.Fatalf("got %v", got)
	}
	if got := toll(0); got != 0 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_toll_used(t *testing.T) {
	if got := vanTripCost(vanNew(1, "V", Diesel, 0, 5), 150); got != 2300 {
		t.Fatalf("got %v", got)
	}
	if got := busTripCost(busNew(1, "B", Petrol, 0, 40), 50); got != 1000 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_surcharge(t *testing.T) {
	if got := weightSurcharge(12, Diesel); got != 800 {
		t.Fatalf("got %v", got)
	}
	if got := weightSurcharge(12, Electric); got != 400 {
		t.Fatalf("got %v", got)
	}
	if got := weightSurcharge(13, Electric); got != 600 {
		t.Fatalf("got %v", got)
	}
	if got := weightSurcharge(9, Electric); got != 0 {
		t.Fatalf("got %v", got)
	}
	if got := weightSurcharge(15, Petrol); got != 2000 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_haul_callers(t *testing.T) {
	if got := truckHaulCost(truckNew(1, "T", Electric, 0, 20), 100, 12); got != 1150 {
		t.Fatalf("got %v", got)
	}
	if got := tankerHaulCost(tankerNew(1, "T", Diesel, 0, 20), 100, 12); got != 2250 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_haul_rest(t *testing.T) {
	if got := flatbedHaulCost(flatbedNew(1, "F", Electric, 0, 20), 200, 15); got != 2500 {
		t.Fatalf("got %v", got)
	}
	if got := reeferHaulCost(reeferNew(1, "R", Petrol, 0, 20), 100, 11); got != 2150 {
		t.Fatalf("got %v", got)
	}
	if got := tipperHaulCost(tipperNew(1, "T", Electric, 0, 20), 0, 20); got != 2000 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_best_van(t *testing.T) {
	if got, ok := vanBestFor([]Van{vanRetire(vanNew(9, "Z", Electric, 0, 5)), vanNew(1, "A", Petrol, 0, 5), vanNew(2, "B", Diesel, 0, 5), vanNew(3, "C", Diesel, 0, 9)}, 100); !ok || got.id != 2 {
		t.Fatalf("got %v %v", got, ok)
	}
	if got, ok := vanBestFor([]Van{vanNew(4, "E", Electric, 0, 5), vanNew(5, "F", Electric, 0, 5)}, 100); !ok || got.id != 4 {
		t.Fatalf("got %v %v", got, ok)
	}
	if got, ok := vanBestFor([]Van{vanRetire(vanNew(6, "G", Diesel, 0, 5)), vanNew(7, "H", Petrol, 0, 5), vanNew(8, "I", Electric, 0, 5)}, 0); !ok || got.id != 7 {
		t.Fatalf("got %v %v", got, ok)
	}
}

func TestHidden_best_van_none(t *testing.T) {
	if got, ok := vanBestFor([]Van{}, 10); ok {
		t.Fatalf("got %v", got)
	}
	if got, ok := vanBestFor([]Van{vanRetire(vanNew(1, "A", Diesel, 0, 5))}, 10); ok {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_unchanged(t *testing.T) {
	if got := vanPlates(vanSample()); !reflect.DeepEqual(got, []string{"VA-101", "VA-201", "VA-301"}) {
		t.Fatalf("got %v", got)
	}
	if got := hhmm(605); got != "10:05" {
		t.Fatalf("got %v", got)
	}
	if got := courierPrice(courierNew(1, "c", 0, 10, 0)); got != 700 {
		t.Fatalf("got %v", got)
	}
}
