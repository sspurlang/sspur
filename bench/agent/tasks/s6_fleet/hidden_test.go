package fleet

import (
	"reflect"
	"testing"
)

func pluck[T, U any](xs []T, f func(T) U) []U {
	out := []U{}
	for _, x := range xs {
		out = append(out, f(x))
	}
	return out
}

func TestHidden_hydrogen(t *testing.T) {
	if got := costPerKm(Hydrogen); got != 9 {
		t.Fatalf("got %v", got)
	}
	if got := co2PerKm(Hydrogen); got != 0 {
		t.Fatalf("got %v", got)
	}
	if got := fuelName(Hydrogen); got != "hydrogen" {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_fuels(t *testing.T) {
	if got := fuels(); !reflect.DeepEqual(got, []Fuel{Diesel, Petrol, Electric, Hydrogen}) {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_hydrogen_used(t *testing.T) {
	if got := busTripCost(busNew(1, "H", Hydrogen, 0, 40), 100); got != 1150 {
		t.Fatalf("got %v", got)
	}
	if got := busLabel(busNew(1, "H-1", Hydrogen, 0, 40)); got != "H-1 (hydrogen)" {
		t.Fatalf("got %v", got)
	}
	if got := taxiTripCo2(taxiNew(1, "H", Hydrogen, 0, 4), 50); got != 0 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_interval(t *testing.T) {
	if got := serviceInterval(Electric); got != 30000 {
		t.Fatalf("got %v", got)
	}
	if got := serviceInterval(Diesel); got != 15000 {
		t.Fatalf("got %v", got)
	}
	if got := serviceInterval(Petrol); got != 15000 {
		t.Fatalf("got %v", got)
	}
	if got := serviceInterval(Hydrogen); got != 15000 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_due_callers(t *testing.T) {
	if got := truckDueService(truckDrive(truckNew(1, "P", Electric, 0, 5), 20000)); got != false {
		t.Fatalf("got %v", got)
	}
	if got := truckDueService(truckDrive(truckNew(1, "P", Electric, 0, 5), 30000)); got != true {
		t.Fatalf("got %v", got)
	}
	if got := vanDueService(vanDrive(vanNew(1, "P", Diesel, 0, 5), 15000)); got != true {
		t.Fatalf("got %v", got)
	}
	if got := pluck(sweeperDueList([]Sweeper{sweeperDrive(sweeperNew(1, "P", Electric, 0, 5), 20000), sweeperDrive(sweeperNew(2, "P", Petrol, 0, 5), 20000)}), func(x Sweeper) int { return x.id }); !reflect.DeepEqual(got, []int{2}) {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_due_rest(t *testing.T) {
	if got := cargoBikeDueService(cargoBikeDrive(cargoBikeNew(1, "P", Electric, 0, 5), 16000)); got != false {
		t.Fatalf("got %v", got)
	}
	if got := ambulanceDueService(ambulanceDrive(ambulanceNew(1, "P", Petrol, 0, 5), 15000)); got != true {
		t.Fatalf("got %v", got)
	}
	if got := scooterDueService(scooterDrive(scooterNew(1, "P", Hydrogen, 0, 5), 15000)); got != true {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_urgent(t *testing.T) {
	if got := medicalNew(1, "c", 0, 10, 600, true).urgent; got != true {
		t.Fatalf("got %v", got)
	}
	if got := medicalPrice(medicalNew(1, "c", 0, 10, 600, true)); got != 1125 {
		t.Fatalf("got %v", got)
	}
	if got := medicalPrice(medicalNew(1, "c", 0, 10, 600, false)); got != 750 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_urgent_line(t *testing.T) {
	if got := medicalLine(medicalNew(3, "Iris", 0, 120, 480, true)); got != "URGENT Iris #3: 120 km to zone B, arrives 9:30" {
		t.Fatalf("got %v", got)
	}
	if got := medicalLine(medicalNew(3, "Iris", 0, 120, 480, false)); got != "Iris #3: 120 km to zone B, arrives 9:30" {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_urgent_rest(t *testing.T) {
	if got := pluck(medicalSample(), func(x Medical) bool { return x.urgent }); !reflect.DeepEqual(got, []bool{false, false, false}) {
		t.Fatalf("got %v", got)
	}
	if got := medicalNightPrice(medicalNew(1, "c", 0, 10, 1400, true)); got != 1687 {
		t.Fatalf("got %v", got)
	}
	if got := medicalNightPrice(medicalNew(1, "c", 0, 10, 1400, false)); got != 1125 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_quote(t *testing.T) {
	if got := courierQuote(courierNew(1, "c", 0, 100, 480), vanNew(1, "V", Diesel, 0, 5)); got != 3950 {
		t.Fatalf("got %v", got)
	}
	if got := courierQuote(courierNew(2, "c", 300, 150, 0), vanNew(2, "W", Electric, 0, 5)); got != 4500 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_unchanged(t *testing.T) {
	if got := vanPlates(vanSample()); !reflect.DeepEqual(got, []string{"VA-101", "VA-201", "VA-301"}) {
		t.Fatalf("got %v", got)
	}
	if got := toll(300); got != 750 {
		t.Fatalf("got %v", got)
	}
	if got := courierPrice(courierNew(1, "c", 0, 10, 0)); got != 700 {
		t.Fatalf("got %v", got)
	}
}
