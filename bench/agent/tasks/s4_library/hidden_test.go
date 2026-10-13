package library

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

func TestHidden_title_key(t *testing.T) {
	if got := titleKey("The Hobbit"); got != "hobbit" {
		t.Fatalf("got %v", got)
	}
	if got := titleKey("  the Hobbit "); got != "hobbit" {
		t.Fatalf("got %v", got)
	}
	if got := titleKey("THE END"); got != "end" {
		t.Fatalf("got %v", got)
	}
	if got := titleKey("Theory"); got != "theory" {
		t.Fatalf("got %v", got)
	}
	if got := titleKey("Other Worlds"); got != "other worlds" {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_titles_used(t *testing.T) {
	if got := bookTitles([]Book{bookNew(1, "The Lost River", 2000, 1, Print), bookNew(2, "Archive", 2000, 1, Print), bookNew(3, "the Garden", 2000, 1, Print)}); !reflect.DeepEqual(got, []string{"archive", "garden", "lost river"}) {
		t.Fatalf("got %v", got)
	}
	if got := zineTitles([]Zine{zineNew(1, "The Zine", 2000, 1, Print), zineNew(2, "Theme", 2000, 1, Print)}); !reflect.DeepEqual(got, []string{"theme", "zine"}) {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_block(t *testing.T) {
	if got := fineBlock(); got != 500 {
		t.Fatalf("got %v", got)
	}
	if got := studentCanBorrow(studentCharge(studentNew(1, "a", 1), 500)); got != false {
		t.Fatalf("got %v", got)
	}
	if got := studentCanBorrow(studentCharge(studentNew(1, "a", 1), 499)); got != true {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_block_used(t *testing.T) {
	if got := familyBorrow(familyCharge(familyNew(1, "a", 2), 700)).loans; got != 0 {
		t.Fatalf("got %v", got)
	}
	if got := guestBorrow(guestCharge(guestNew(1, "a", 1), 120)).loans; got != 1 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_fine_notice(t *testing.T) {
	if got := fineNotice("Bo", 1205); got != "Dear Bo, you owe $12.05." {
		t.Fatalf("got %v", got)
	}
	if got := fineNotice("Al", 7); got != "Dear Al, you owe $0.07." {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_reminders(t *testing.T) {
	if got := studentReminder(studentCharge(studentNew(1, "Al", 1), 250)); got != "Dear Al, you owe $2.50." {
		t.Fatalf("got %v", got)
	}
	if got := staffReminder(staffCharge(staffNew(1, "Bea", 1), 1000)); got != "Dear Bea, you owe $10.00." {
		t.Fatalf("got %v", got)
	}
	if got := facultyReminder(facultyCharge(facultyNew(1, "Cy", 1), 5)); got != "Dear Cy, you owe $0.05." {
		t.Fatalf("got %v", got)
	}
	if got := alumniReminder(alumniNew(1, "Di", 1)); got != "Dear Di, you owe $0.00." {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_thesis_advisor(t *testing.T) {
	if got := thesisNew(1, "On Rivers", 2010, 1, Print, "Dr. Lee").advisor; got != "Dr. Lee" {
		t.Fatalf("got %v", got)
	}
	if got := thesisLine(thesisNew(7, "Atlas", 1999, 3, Audio, "Kim")); got != "Atlas (1999) [THE-7] 3/3 audio / Kim" {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_thesis_plain(t *testing.T) {
	if got := thesisLine(thesisNew(7, "Atlas", 1999, 3, Audio, "")); got != "Atlas (1999) [THE-7] 3/3 audio" {
		t.Fatalf("got %v", got)
	}
	if got := pluck(thesisSample(), func(x Thesis) string { return x.advisor }); !reflect.DeepEqual(got, []string{"", "", ""}) {
		t.Fatalf("got %v", got)
	}
	if got := thesisAvailable(thesisCheckout(thesisNew(1, "a", 2000, 2, Print, "X"))); got != 1 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_staff_can_take(t *testing.T) {
	if got := staffCanTake(staffNew(1, "a", 1), bookNew(1, "b", 2000, 1, Print)); got != true {
		t.Fatalf("got %v", got)
	}
	if got := staffCanTake(staffNew(1, "a", 1), bookCheckout(bookNew(1, "b", 2000, 1, Print))); got != false {
		t.Fatalf("got %v", got)
	}
	if got := staffCanTake(staffSuspend(staffNew(1, "a", 1)), bookNew(1, "b", 2000, 1, Print)); got != false {
		t.Fatalf("got %v", got)
	}
	if got := staffCanTake(staffCharge(staffNew(1, "a", 1), 600), bookNew(1, "b", 2000, 1, Print)); got != false {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_unchanged(t *testing.T) {
	if got := bookTitles(bookSample()); !reflect.DeepEqual(got, []string{"lost atlas", "northern voyage", "silent river"}) {
		t.Fatalf("got %v", got)
	}
	if got := fineFor(Print, 10); got != 250 {
		t.Fatalf("got %v", got)
	}
	if got := maxLoans(3); got != 10 {
		t.Fatalf("got %v", got)
	}
}
