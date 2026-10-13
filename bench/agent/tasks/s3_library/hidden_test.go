package library

import (
	"reflect"
	"testing"
)

func TestHidden_cap(t *testing.T) {
	if got := fineCap(); got != 800 {
		t.Fatalf("got %v", got)
	}
	if got := fineFor(Print, 100); got != 800 {
		t.Fatalf("got %v", got)
	}
	if got := fineFor(Audio, 30); got != 300 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_cap_used(t *testing.T) {
	if got := bookFine(bookNew(1, "a", 2000, 1, Print), 0, 60); got != 800 {
		t.Fatalf("got %v", got)
	}
	if got := dvdFine(dvdNew(1, "d", 2000, 1, Audio), 0, 74); got != 600 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_renewal(t *testing.T) {
	if got := renewal(21, Print, 0); got != 31 {
		t.Fatalf("got %v", got)
	}
	if got := renewal(21, Print, 1); got != 31 {
		t.Fatalf("got %v", got)
	}
	if got := renewal(21, Print, 2); got != 21 {
		t.Fatalf("got %v", got)
	}
	if got := renewal(14, Ebook, 5); got != 14 {
		t.Fatalf("got %v", got)
	}
	if got := renewal(7, Audio, 0); got != 14 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_renewed_callers(t *testing.T) {
	if got := bookRenewedDue(bookNew(1, "a", 2000, 1, Print), 0, 0); got != 31 {
		t.Fatalf("got %v", got)
	}
	if got := journalRenewedDue(journalNew(1, "j", 2000, 1, Ebook), 10, 1); got != 31 {
		t.Fatalf("got %v", got)
	}
	if got := scoreRenewedDue(scoreNew(1, "s", 2000, 1, Print), 0, 2); got != 21 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_renewed_rest(t *testing.T) {
	if got := comicRenewedDue(comicNew(1, "c", 2000, 1, Audio), 0, 3); got != 14 {
		t.Fatalf("got %v", got)
	}
	if got := atlasRenewedDue(atlasNew(1, "a", 2000, 1, Print), 5, 1); got != 36 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_microfilm(t *testing.T) {
	if got := loanDays(Microfilm); got != 7 {
		t.Fatalf("got %v", got)
	}
	if got := dailyFine(Microfilm); got != 50 {
		t.Fatalf("got %v", got)
	}
	if got := formatName(Microfilm); got != "microfilm" {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_formats(t *testing.T) {
	if got := formats(); !reflect.DeepEqual(got, []Format{Print, Ebook, Audio, Microfilm}) {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_microfilm_used(t *testing.T) {
	if got := bookLine(bookNew(7, "Atlas", 1999, 3, Microfilm)); got != "Atlas (1999) [BOO-7] 3/3 microfilm" {
		t.Fatalf("got %v", got)
	}
	if got := bookFine(bookNew(1, "a", 2000, 1, Microfilm), 0, 10); got != 150 {
		t.Fatalf("got %v", got)
	}
	if got := bookDue(bookNew(1, "a", 2000, 1, Microfilm), 3); got != 10 {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_overdue(t *testing.T) {
	if got := journalOverdue([]Journal{journalCheckout(journalNew(1, "Quiet River", 2001, 2, Print)), journalNew(2, "Alpha", 2002, 1, Print), journalCheckout(journalNew(3, " Bright Atlas", 2003, 1, Ebook)), journalCheckout(journalNew(4, "Atlas Engine", 1990, 1, Audio))}, 0, 30); !reflect.DeepEqual(got, []string{"atlas engine", "quiet river"}) {
		t.Fatalf("got %v", got)
	}
}

func TestHidden_overdue_edges(t *testing.T) {
	if got := journalOverdue([]Journal{journalCheckout(journalNew(1, "a", 2000, 1, Print))}, 0, 21); len(got) != 0 {
		t.Fatalf("got %v", got)
	}
	if got := journalOverdue([]Journal{}, 0, 99); len(got) != 0 {
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
	if got := studentLine(studentCharge(studentNew(3, "Ann", 1), 250)); got != "Ann (tier 1): 0/3 loans, owes $2.50" {
		t.Fatalf("got %v", got)
	}
}
