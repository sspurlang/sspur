package shop

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

func TestHidden_ca_rate(t *testing.T) {
	if got := taxRate("CA"); got != 5 {
		t.Fatalf("got %v", got)
	}
	if got := taxRate("EU"); got != 19 {
		t.Fatalf("got %v", got)
	}
	if got := taxRate("UK"); got != 20 {
		t.Fatalf("got %v", got)
	}
	if got := taxRate("APAC"); got != 0 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_ca_used(t *testing.T) {
	if got := customerTax(customerNew(1, "a", 10, 100, "CA")); got != 50 {
		t.Fatalf("got %v", got)
	}
	if got := invoiceGross(invoiceNew(1, "a", 1, 1000, "CA")); got != 1050 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_regions(t *testing.T) {
	if got := regions(); !reflect.DeepEqual(got, []string{"EU", "US", "UK", "APAC", "CA"}) {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_label_trim(t *testing.T) {
	if got := labelOf("  pen ", 4); got != "pen #4" {
		t.Fatalf("got %v", got)
	}
	if got := labelOf("box", 12); got != "box #12" {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_label_used(t *testing.T) {
	if got := productLabel(productNew(4, " pen ", 1, 120, "US")); got != "pen #4" {
		t.Fatalf("got %v", got)
	}
	if got := productLine(productNew(4, " pen", 1, 1234, "APAC")); got != "pen #4: 12.34" {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_sub_months(t *testing.T) {
	if got := subscriptionNew(1, "s", 2, 300, "US", 6).months; got != 6 {
		t.Fatalf("got %v", got)
	}
	if got := subscriptionValue(subscriptionNew(1, "s", 2, 300, "US", 6)); got != 600 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_sub_renewal(t *testing.T) {
	if got := subscriptionRenewalValue(subscriptionNew(1, "s", 2, 300, "US", 6)); got != 3600 {
		t.Fatalf("got %v", got)
	}
	if got := subscriptionRenewalValue(subscriptionNew(2, "t", 1, 50, "UK", 1)); got != 50 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_sub_sample(t *testing.T) {
	if got := pluck(subscriptionSample(), func(x Subscription) int { return x.months }); !reflect.DeepEqual(got, []int{1, 1, 1}) {
		t.Fatalf("got %v", got)
	}
	if got := subscriptionTotal(subscriptionSample()); got != 1799 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_cart_summary(t *testing.T) {
	if got := cartSummary([]Cart{cartNew(1, "a", 2, 500, "US"), cartNew(2, "b", 1, 1234, "EU"), cartDeactivate(cartNew(3, "c", 1, 999, "US")), cartNew(4, "d", 1, 234, "US"), cartNew(5, "e", 1, 1010, "UK")}, "US"); got != "carts=2 total=13.20" {
		t.Fatalf("got %v", got)
	}
	if got := cartSummary([]Cart{cartNew(1, "a", 2, 500, "US"), cartNew(2, "b", 1, 1234, "EU"), cartDeactivate(cartNew(3, "c", 1, 999, "US")), cartNew(4, "d", 1, 234, "US"), cartNew(5, "e", 1, 1010, "UK")}, "UK"); got != "carts=1 total=12.12" {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_unchanged(t *testing.T) {
	if got := customerTotal(customerSample()); got != 886 {
		t.Fatalf("got %v", got)
	}
	if got := routeTotal(routeSample()); got != 1308 {
		t.Fatalf("got %v", got)
	}
	if got := lessonBucket(lessonNew(1, "l", 1, 1, "US")); got != "small" {
		t.Fatalf("got %v", got)
	}
}
